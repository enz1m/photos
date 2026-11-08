use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use enzui_services::credential_store::Secret;
use photos_library::catalog::{Photo, ServiceAccount, ServiceProvider};

mod immich;
pub(crate) mod sync;

pub(crate) const MAX_TRANSFER_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub(crate) fn reserve(path: &Path, bytes: u64) -> Result<()> {
    let space = rustix::fs::statvfs(path)?;
    ensure!(
        space.f_bavail.saturating_mul(space.f_frsize) > bytes.saturating_add(256 * 1024 * 1024),
        "Not enough free space to safely transfer this original"
    );
    Ok(())
}

struct BoundedFile<'a> {
    file: &'a mut std::fs::File,
    remaining: u64,
}
impl Write for BoundedFile<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other(
                "Original exceeds its media size limit",
            ));
        }
        self.file.write_all(bytes)?;
        self.remaining -= bytes.len() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(crate) struct ApiKey(Arc<Secret>);

impl ApiKey {
    pub fn new(value: String) -> Self {
        Self(Arc::new(Secret::from_string(value)))
    }

    pub fn text(&self) -> &str {
        std::str::from_utf8(self.0.as_bytes()).unwrap_or_default()
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey([redacted])")
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Asset {
    pub video: bool,
    pub checksum: String,
    pub id: String,
    pub name: String,
    pub taken: String,
    pub width: u32,
    pub height: u32,
}

impl Asset {
    pub fn label(&self) -> String {
        self.name
            .chars()
            .filter(|c| !c.is_control())
            .take(100)
            .collect()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Page {
    pub assets: Vec<Asset>,
    pub next: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RemoteAlbum {
    pub cover: Option<String>,
    pub id: String,
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct UploadReceipt {
    pub created: bool,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SearchKind {
    #[default]
    Filename,
    Smart,
}

pub(crate) trait PhotoService: fmt::Debug + Send + Sync {
    fn check(&self, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<String>;
    fn list(
        &self,
        cursor: Option<&str>,
        query: &str,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Page>;
    fn smart_search_available(&self, _: &(dyn Fn() -> bool + Sync)) -> Result<bool> {
        Ok(false)
    }
    fn search(
        &self,
        cursor: Option<&str>,
        query: &str,
        album: Option<&str>,
        kind: SearchKind,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Page> {
        ensure!(
            kind == SearchKind::Filename || query.trim().is_empty(),
            "AI search is unavailable on this service"
        );
        match album {
            Some(album) => self.list_album(album, cursor, query, cancelled),
            None => self.list(cursor, query, cancelled),
        }
    }
    fn albums(&self, _: &(dyn Fn() -> bool + Sync)) -> Result<Vec<RemoteAlbum>> {
        anyhow::bail!("This service does not support albums")
    }
    fn list_album(
        &self,
        _: &str,
        _: Option<&str>,
        _: &str,
        _: &(dyn Fn() -> bool + Sync),
    ) -> Result<Page> {
        anyhow::bail!("This service does not support albums")
    }
    fn create_album(&self, _: &str, _: &(dyn Fn() -> bool + Sync)) -> Result<RemoteAlbum> {
        anyhow::bail!("This service does not support album creation")
    }
    fn upload_into(
        &self,
        photo: &Photo,
        album: Option<&str>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<bool> {
        ensure!(album.is_none(), "This service cannot upload into albums");
        self.upload(photo, cancelled)
    }
    fn send(
        &self,
        photo: &Photo,
        album: Option<&str>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<UploadReceipt> {
        Ok(UploadReceipt {
            created: self.upload_into(photo, album, cancelled)?,
            id: None,
        })
    }
    fn download(
        &self,
        id: &str,
        output: &mut dyn Write,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<()> {
        output.write_all(&self.original(id, cancelled)?)?;
        Ok(())
    }
    fn thumbnail(&self, id: &str, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>>;
    fn preview(&self, id: &str, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>> {
        self.thumbnail(id, cancelled)
    }
    fn original(&self, id: &str, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<Vec<u8>>;
    fn upload(&self, photo: &Photo, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<bool>;
}

pub(crate) fn connect(account: &ServiceAccount, key: ApiKey) -> Result<Arc<dyn PhotoService>> {
    match account.provider {
        ServiceProvider::Immich => Ok(Arc::new(immich::Immich::new(&account.url, key)?)),
    }
}

pub(crate) fn normalized_url(value: &str) -> Result<String> {
    ensure!(value.len() <= 2048, "Server URL is too long");
    let mut url =
        url::Url::parse(value.trim()).context("Enter a complete http:// or https:// server URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "Use an HTTP or HTTPS server URL"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Server URL must not contain credentials, a query or a fragment"
    );
    let path = url
        .path()
        .trim_end_matches('/')
        .trim_end_matches("/api")
        .to_owned();
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

pub(crate) fn import_original(
    provider: &dyn PhotoService,
    asset: &Asset,
    destination: &Path,
    dated_folders: bool,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<PathBuf> {
    ensure!(!cancelled(), "Transfer cancelled");
    let name = safe_name(&asset.name)?;
    ensure!(!cancelled(), "Transfer cancelled");
    std::fs::create_dir_all(destination).context("Cannot create the import folder")?;
    let mut destination = std::fs::canonicalize(destination)?;
    let taken = chrono::DateTime::parse_from_rfc3339(&asset.taken)
        .map(|date| date.with_timezone(&chrono::Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&asset.taken, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|date| date.and_utc())
        });
    if dated_folders {
        let folder = taken
            .map(|date| date.format("%Y/%m").to_string())
            .unwrap_or_else(|| "Undated".into());
        destination = super::import::create_date_folder(&destination, Path::new(&folder))?;
    }
    let suffix = format!(
        ".{}",
        Path::new(&name)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
    );
    let mut temporary = tempfile::Builder::new()
        .prefix(".photos-service-")
        .suffix(&suffix)
        .tempfile_in(&destination)?;
    reserve(
        &destination,
        if asset.video {
            MAX_TRANSFER_BYTES
        } else {
            photos_library::raster::MAX_ENCODED_BYTES
        },
    )?;
    provider.download(
        &asset.id,
        &mut BoundedFile {
            file: temporary.as_file_mut(),
            remaining: if asset.video {
                MAX_TRANSFER_BYTES
            } else {
                photos_library::raster::MAX_ENCODED_BYTES
            },
        },
        cancelled,
    )?;
    ensure!(!cancelled(), "Transfer cancelled");
    if let Some(taken) = taken {
        let time: std::time::SystemTime = taken.into();
        temporary
            .as_file()
            .set_times(std::fs::FileTimes::new().set_modified(time))?;
    }
    temporary.as_file().sync_all()?;
    let photo = photos_library::scan::inspect_file(temporary.path())
        .map_err(|e| anyhow::anyhow!("Downloaded original is not a supported photo: {e}"))?;
    ensure!(
        photos_library::scan::is_video_path(&photo.path) == asset.video,
        "Server media type does not match its filename"
    );
    if asset.video {
        super::video::probe(temporary.path(), &|| cancelled())?;
    }
    let stem = Path::new(&name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let extension = Path::new(&name)
        .extension()
        .unwrap_or_default()
        .to_string_lossy();
    for collision in 0..10_000 {
        ensure!(!cancelled(), "Transfer cancelled");
        let output = destination.join(if collision == 0 {
            name.clone()
        } else {
            format!("{stem} ({collision}).{extension}")
        });
        match temporary.persist_noclobber(&output) {
            Ok(_) => {
                std::fs::File::open(&destination)?.sync_all()?;
                return Ok(output);
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temporary = error.file
            }
            Err(error) => return Err(error.error.into()),
        }
    }
    anyhow::bail!("Too many imported photos with the same name")
}

pub(crate) fn save_original_as(
    provider: &dyn PhotoService,
    asset: &Asset,
    path: &Path,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "Choose an absolute local destination");
    let parent = path.parent().context("The copy has no parent folder")?;
    let extension = Path::new(&asset.name)
        .extension()
        .context("The original has no file extension")?;
    let output = path.with_extension(extension);
    let name = output
        .file_name()
        .and_then(|n| n.to_str())
        .context("The copy filename is not supported")?;
    let mut copy = asset.clone();
    copy.name = safe_name(name)?;
    import_original(provider, &copy, parent, false, cancelled)
}

fn safe_name(value: &str) -> Result<String> {
    ensure!(
        !value.is_empty()
            && value.len() <= 240
            && value != "."
            && value != ".."
            && !value.contains(['/', '\\'])
            && !value.chars().any(char::is_control),
        "The server returned an unsafe photo filename"
    );
    Ok(value.to_owned())
}

pub(crate) fn import_batch(
    provider: &dyn PhotoService,
    assets: &[Asset],
    destination: &Path,
    layout: photos_library::catalog::ImportLayout,
    source_name: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<super::import::ImportReport> {
    use photos_library::catalog::ImportLayout;
    ensure!(!cancelled(), "Transfer cancelled");
    std::fs::create_dir_all(destination)?;
    let destination = std::fs::canonicalize(destination)?;
    let target = if layout == ImportLayout::PerImport {
        let label: String = source_name
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-'))
            .take(64)
            .collect();
        super::import::create_session_folder(
            &destination,
            &format!("{} {}", chrono::Utc::now().format("%Y-%m-%d %H%M%S"), label),
        )?
    } else {
        destination.clone()
    };
    let mut report = super::import::ImportReport {
        destination: Some(destination),
        ..Default::default()
    };
    for asset in assets {
        match import_original(
            provider,
            asset,
            &target,
            layout == ImportLayout::YearMonth,
            cancelled,
        ) {
            Ok(path) => report.paths.push(path),
            Err(error) => {
                report.errors.push(error.to_string());
                break;
            }
        }
    }
    report.cancelled = cancelled();
    report.skipped = assets.len().saturating_sub(report.paths.len());
    Ok(report)
}
