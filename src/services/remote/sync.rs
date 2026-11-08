use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use enzui_iced_core::state_authority::{CustodySnapshot, StateAuthority};
use photos_library::catalog::{PhotoId, ServiceSync, SyncDirection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Asset, PhotoService};

#[derive(Default, Serialize, Deserialize)]
struct Ledger {
    version: u32,
    folder_device: u64,
    folder_inode: u64,
    downloaded: BTreeMap<String, Receipt>,
    uploaded: BTreeMap<PathBuf, Uploaded>,
}
#[derive(Serialize, Deserialize)]
struct Uploaded {
    digest: String,
    id: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    checksum: String,
    path: PathBuf,
    digest: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Report {
    pub paths: Vec<PathBuf>,
    pub downloaded: usize,
    pub uploaded: usize,
    pub unchanged: usize,
    pub conflicts: usize,
    pub error: Option<String>,
}

pub(crate) struct UploadScope {
    pub roots: Vec<PathBuf>,
    pub members: Option<BTreeSet<PhotoId>>,
    pub excluded: BTreeSet<PhotoId>,
}

pub(crate) fn digest(path: &Path, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<String> {
    let mut file = photos_library::raster::open_media(path)?;
    let before = file.metadata()?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        ensure!(!cancelled(), "Sync cancelled");
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    ensure!(
        before.len() == after.len()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.mtime() == after.mtime()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "Original changed while checking sync identity"
    );
    Ok(format!("{:x}", hash.finalize()))
}

fn checkpoint(authority: &mut StateAuthority, ledger: &Ledger) -> Result<()> {
    ensure!(
        ledger.downloaded.len() + ledger.uploaded.len() <= 100_000,
        "Sync history limit reached"
    );
    let bytes = serde_json::to_vec(ledger)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "Sync history size limit reached"
    );
    authority
        .publish(&bytes)
        .context("Cannot safely save sync progress")?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn run(
    provider: &dyn PhotoService,
    plan: &ServiceSync,
    state: &Path,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Report {
    run_scoped(provider, plan, state, None, cancelled)
}

pub(crate) fn run_scoped(
    provider: &dyn PhotoService,
    plan: &ServiceSync,
    state: &Path,
    scope: Option<&UploadScope>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Report {
    let mut report = Report::default();
    if let Err(error) = execute(provider, plan, state, scope, cancelled, &mut report) {
        report.error = Some(error.to_string());
    }
    report
}

fn execute(
    provider: &dyn PhotoService,
    plan: &ServiceSync,
    state: &Path,
    scope: Option<&UploadScope>,
    cancelled: &(dyn Fn() -> bool + Sync),
    report: &mut Report,
) -> Result<()> {
    ensure!(!cancelled(), "Sync cancelled");
    std::fs::create_dir_all(&plan.folder)?;
    let folder = std::fs::canonicalize(&plan.folder)?;
    let metadata = std::fs::metadata(&folder)?;
    ensure!(metadata.is_dir(), "Sync destination is not a folder");
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(state.with_extension("lock"))?;
    lease
        .try_lock()
        .context("Another Photos process is syncing this folder")?;
    let mut authority = StateAuthority::open_or_create(state)?;
    let mut ledger: Ledger = match authority.read_bounded(16 * 1024 * 1024)? {
        CustodySnapshot::Absent => Ledger {
            version: 1,
            folder_device: metadata.dev(),
            folder_inode: metadata.ino(),
            ..Default::default()
        },
        CustodySnapshot::Current {
            bytes,
            overflow: false,
            ..
        } => {
            serde_json::from_slice(&bytes).context("Sync history is damaged; transfers stopped")?
        }
        _ => anyhow::bail!("Sync history is unavailable; transfers stopped"),
    };
    ensure!(
        ledger.version == 1
            && ledger.folder_device == metadata.dev()
            && ledger.folder_inode == metadata.ino(),
        "Sync folder identity changed; transfers stopped"
    );
    ensure!(
        ledger
            .downloaded
            .values()
            .all(|entry| entry.path.parent() == Some(folder.as_path())),
        "Sync history contains an unexpected destination"
    );
    checkpoint(&mut authority, &ledger)?;
    for receipt in ledger.downloaded.values() {
        if std::fs::symlink_metadata(&receipt.path)
            .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        {
            report.paths.push(receipt.path.clone());
        }
    }
    if matches!(plan.direction, SyncDirection::Upload | SyncDirection::Both) {
        let scan = photos_library::scan::scan(
            scope.map_or(std::slice::from_ref(&folder), |scope| {
                scope.roots.as_slice()
            }),
            Default::default(),
            cancelled,
        );
        ensure!(
            !scan.cancelled && !scan.truncated && scan.warnings.is_empty(),
            "Sync folder scan incomplete; upload stopped"
        );
        if let Some(members) = scope.and_then(|scope| scope.members.as_ref()) {
            let found: BTreeSet<_> = scan.photos.iter().map(|photo| &photo.id).collect();
            ensure!(
                members.iter().all(|id| found.contains(id)),
                "An album original is unavailable; upload stopped"
            );
        }
        let downloaded: BTreeMap<_, _> = ledger
            .downloaded
            .values()
            .map(|receipt| (receipt.path.clone(), receipt.digest.clone()))
            .collect();
        for photo in scan.photos {
            if scope.is_some_and(|scope| {
                scope.excluded.contains(&photo.id)
                    || scope
                        .members
                        .as_ref()
                        .is_some_and(|members| !members.contains(&photo.id))
            }) {
                continue;
            }
            ensure!(!cancelled(), "Sync cancelled");
            let hash = digest(&photo.path, cancelled)?;
            if downloaded.get(&photo.path) == Some(&hash)
                || ledger
                    .uploaded
                    .get(&photo.path)
                    .is_some_and(|entry| entry.digest == hash)
            {
                report.unchanged += 1;
                continue;
            }
            let result = provider.send(&photo, plan.album.as_deref(), cancelled)?;
            ledger.uploaded.insert(
                photo.path,
                Uploaded {
                    digest: hash,
                    id: result.id,
                },
            );
            checkpoint(&mut authority, &ledger)?;
            report.uploaded += 1;
        }
    }
    if matches!(
        plan.direction,
        SyncDirection::Download | SyncDirection::Both
    ) {
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        let mut count = 0;
        loop {
            ensure!(!cancelled(), "Sync cancelled");
            let page = if let Some(album) = &plan.album {
                provider.list_album(album, cursor.as_deref(), "", cancelled)?
            } else {
                provider.list(cursor.as_deref(), "", cancelled)?
            };
            for asset in page.assets {
                count += 1;
                ensure!(count <= 100_000, "Sync media limit reached");
                ensure!(!cancelled(), "Sync cancelled");
                if ledger
                    .uploaded
                    .values()
                    .any(|entry| entry.id.as_deref() == Some(&asset.id))
                {
                    report.unchanged += 1;
                    continue;
                }
                if let Some(receipt) = ledger.downloaded.get(&asset.id) {
                    if receipt.checksum != asset.checksum {
                        report.conflicts += 1;
                    } else {
                        report.unchanged += 1;
                    }
                    // Missing or edited local copies are intentional local state, never restored over it.
                    continue;
                }
                let original = super::safe_name(&asset.name)?;
                let extension = Path::new(&original)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                ensure!(
                    extension.len() <= 20,
                    "Remote filename extension is too long"
                );
                let stem: String = Path::new(&original)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .chars()
                    .take(38)
                    .collect();
                let name = format!("{}-{}.{}", identity(&asset.id), stem, extension);
                let target = folder.join(&name);
                let download = Asset {
                    name,
                    ..asset.clone()
                };
                let path = if target.try_exists()? {
                    let temporary = tempfile::tempdir_in(&folder)?;
                    let copy = super::import_original(
                        provider,
                        &download,
                        temporary.path(),
                        false,
                        cancelled,
                    )?;
                    if digest(&copy, cancelled)? != digest(&target, cancelled)? {
                        report.conflicts += 1;
                        continue;
                    }
                    report.paths.push(target.clone());
                    target
                } else {
                    let path =
                        super::import_original(provider, &download, &folder, false, cancelled)?;
                    report.paths.push(path.clone());
                    report.downloaded += 1;
                    path
                };
                let receipt = Receipt {
                    checksum: asset.checksum,
                    digest: digest(&path, cancelled)?,
                    path,
                };
                ledger.downloaded.insert(asset.id, receipt);
                checkpoint(&mut authority, &ledger)?;
            }
            match page.next {
                Some(next) => {
                    ensure!(seen.insert(next.clone()), "Server repeated a sync cursor");
                    cursor = Some(next);
                }
                None => break,
            }
        }
    }
    Ok(())
}

fn identity(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
