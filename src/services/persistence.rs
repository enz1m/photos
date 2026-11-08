use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use photos_library::catalog::Catalog;

const HEADER: &str = "enzui-iced photos-catalog v1\nbody\n";
const MAX_CATALOG_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct CatalogSession {
    pub catalog: Catalog,
    pub authority: Arc<Mutex<enzui_iced_core::state_authority::StateAuthority>>,
}

pub(crate) fn open(path: &Path) -> Result<CatalogSession> {
    let parent = path.parent().context("catalog has no parent folder")?;
    std::fs::create_dir_all(parent).context("cannot create catalog folder")?;
    let authority = enzui_iced_core::state_authority::StateAuthority::open_or_create(path)
        .context("cannot retain private catalog file")?;
    let catalog = match authority.read_bounded(MAX_CATALOG_BYTES)? {
        enzui_iced_core::state_authority::CustodySnapshot::Absent => Catalog::default(),
        enzui_iced_core::state_authority::CustodySnapshot::Current {
            bytes,
            overflow: false,
            ..
        } => {
            let framed = std::str::from_utf8(&bytes).context("catalog is not valid UTF-8")?;
            let body = framed
                .strip_prefix(HEADER)
                .context("catalog format is damaged or incompatible")?;
            parse(body)?
        }
        _ => anyhow::bail!("catalog exceeds its size limit or has lost custody"),
    };
    Ok(CatalogSession {
        catalog,
        authority: Arc::new(Mutex::new(authority)),
    })
}

fn parse(body: &str) -> Result<Catalog> {
    let mut catalog: Catalog = serde_json::from_str(body)
        .context("photo catalog is damaged; it has not been overwritten")?;
    anyhow::ensure!(
        matches!(catalog.version, 1..=4),
        "photo catalog version is not supported"
    );
    let mut ids = std::collections::BTreeSet::new();
    for album in &catalog.albums {
        anyhow::ensure!(
            album.id > 0 && album.id < catalog.next_album && ids.insert(album.id),
            "catalog contains invalid album identities"
        );
        anyhow::ensure!(
            !album.name.trim().is_empty()
                && album.name.chars().count() <= 100
                && !album.name.chars().any(char::is_control),
            "catalog contains an invalid album name"
        );
    }
    anyhow::ensure!(catalog.next_album > 0, "catalog album sequence is invalid");
    anyhow::ensure!(
        catalog.next_service > 0,
        "catalog service sequence is invalid"
    );
    anyhow::ensure!(
        catalog.version >= 2 || catalog.services.is_empty(),
        "service connections require catalog version 2"
    );
    let mut service_ids = std::collections::BTreeSet::new();
    for account in &catalog.services {
        anyhow::ensure!(
            account.id > 0 && account.id < catalog.next_service && service_ids.insert(account.id),
            "catalog contains invalid service identities"
        );
        anyhow::ensure!(
            !account.name.trim().is_empty()
                && account.name.chars().count() <= 100
                && !account.name.chars().any(char::is_control),
            "catalog contains an invalid service name"
        );
        anyhow::ensure!(
            super::remote::normalized_url(&account.url)? == account.url,
            "catalog contains an invalid service URL"
        );
    }
    anyhow::ensure!(
        catalog.version >= 3 || catalog.service_sync.is_empty(),
        "sync plans require catalog version 3"
    );
    for (id, plan) in &catalog.service_sync {
        anyhow::ensure!(
            service_ids.contains(id) && plan.folder.is_absolute(),
            "invalid sync account or folder"
        );
        anyhow::ensure!(
            plan.album.as_ref().is_none_or(|id| !id.is_empty()
                && id.len() <= 128
                && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')),
            "invalid sync album"
        );
    }
    anyhow::ensure!(
        catalog.sources.iter().all(|source| source.is_absolute()),
        "catalog contains relative source folders"
    );
    anyhow::ensure!(
        catalog
            .sources
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == catalog.sources.len(),
        "catalog contains duplicate source folders"
    );
    for recipe in catalog.edits.values() {
        recipe.validate()?;
    }
    anyhow::ensure!(
        catalog.next_transfer_rule > 0 && catalog.transfer_rules.len() <= 512,
        "invalid automatic transfer rule sequence or count"
    );
    anyhow::ensure!(
        catalog.version >= 4 || catalog.transfer_rules.is_empty(),
        "automatic transfer rules require catalog version 4"
    );
    anyhow::ensure!(
        catalog.version < 4 || catalog.service_sync.is_empty(),
        "legacy synchronization plans must be migrated"
    );
    for (id, rule) in &catalog.transfer_rules {
        anyhow::ensure!(
            *id > 0
                && *id < catalog.next_transfer_rule
                && service_ids.contains(&rule.account)
                && rule.plan.folder.is_absolute()
                && rule.plan.direction != photos_library::catalog::SyncDirection::Both,
            "invalid automatic transfer rule"
        );
        anyhow::ensure!(
            rule.plan.album.as_ref().is_none_or(|id| !id.is_empty()
                && id.len() <= 128
                && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')),
            "invalid automatic transfer album"
        );
        anyhow::ensure!(
            rule.remote_label.chars().count() <= 512
                && !rule.remote_label.chars().any(char::is_control),
            "invalid remote source label"
        );
    }
    if !catalog.service_sync.is_empty() {
        use photos_library::catalog::{SyncDirection, TransferRule, TransferSource};
        for (account, plan) in std::mem::take(&mut catalog.service_sync) {
            let directions = match plan.direction {
                SyncDirection::Both => vec![SyncDirection::Upload, SyncDirection::Download],
                direction => vec![direction],
            };
            for direction in directions {
                let id = catalog.next_transfer_rule;
                catalog.next_transfer_rule += 1;
                catalog.transfer_rules.insert(
                    id,
                    TransferRule {
                        account,
                        enabled: true,
                        source: TransferSource::Folder,
                        remote_label: plan
                            .album
                            .clone()
                            .unwrap_or_else(|| "Entire server library".into()),
                        plan: photos_library::catalog::ServiceSync {
                            direction,
                            ..plan.clone()
                        },
                        legacy_receipts: true,
                    },
                );
            }
        }
        catalog.version = 4;
    }
    anyhow::ensure!(
        catalog.transfer_rules.len() <= 512,
        "too many automatic transfer rules"
    );
    Ok(catalog)
}

#[cfg(test)]
pub(crate) fn load(path: &Path) -> Result<Catalog> {
    if !path.try_exists()? {
        return Ok(Catalog::default());
    }
    let (state, _) =
        enzui_iced_core::atomic_state::load_bounded(path, "photos-catalog", 1, 16 * 1024 * 1024);
    match state {
        enzui_iced_core::atomic_state::StateLoad::Loaded(body) => parse(&body),
        enzui_iced_core::atomic_state::StateLoad::UsedDefaults { note } => {
            anyhow::bail!("cannot load photo catalog: {note}")
        }
    }
}

pub(crate) fn publish(
    authority: &Mutex<enzui_iced_core::state_authority::StateAuthority>,
    catalog: &Catalog,
) -> Result<()> {
    let body = serde_json::to_string(catalog)?;
    parse(&body)?;
    anyhow::ensure!(
        body.len() + HEADER.len() <= MAX_CATALOG_BYTES,
        "photo catalog is too large"
    );
    let framed = format!("{HEADER}{body}");
    let mut authority = authority
        .lock()
        .map_err(|_| anyhow::anyhow!("catalog authority was poisoned"))?;
    authority
        .publish(framed.as_bytes())
        .context("catalog save could not be confirmed; retain this session and check the catalog before retrying")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_service_migration_preserves_library_data_and_refuses_invalid_publication() {
        use photos_library::catalog::{ServiceAccount, ServiceProvider};
        let sandbox = tempfile::tempdir().expect("sandbox");
        let path = sandbox.path().join("catalog.json");
        let mut session = open(&path).expect("legacy session");
        session
            .catalog
            .create_album("Existing album")
            .expect("album");
        publish(&session.authority, &session.catalog).expect("legacy save");
        assert_eq!(load(&path).expect("legacy reload").version, 1);
        session.catalog.services.push(ServiceAccount {
            id: 1,
            name: "Home server".into(),
            provider: ServiceProvider::Immich,
            url: "https://photos.example.test".into(),
        });
        session.catalog.next_service = 2;
        session.catalog.version = 2;
        publish(&session.authority, &session.catalog).expect("service save");
        let reloaded = load(&path).expect("service reload");
        assert_eq!(reloaded.albums[0].name, "Existing album");
        assert_eq!(reloaded.services, session.catalog.services);
        let saved = std::fs::read(&path).expect("saved bytes");
        session.catalog.next_service = 1;
        assert!(publish(&session.authority, &session.catalog).is_err());
        assert_eq!(std::fs::read(&path).expect("preserved bytes"), saved);
        session.catalog.next_service = 2;
        session.catalog.version = 1;
        assert!(publish(&session.authority, &session.catalog).is_err());
        assert_eq!(std::fs::read(&path).expect("preserved bytes"), saved);
        session.catalog.version = 3;
        session.catalog.service_sync.insert(
            1,
            photos_library::catalog::ServiceSync {
                folder: sandbox.path().join("sync"),
                direction: photos_library::catalog::SyncDirection::Both,
                album: None,
            },
        );
        publish(&session.authority, &session.catalog).expect("sync plan save");
        let migrated = load(&path).expect("sync reload");
        assert_eq!(migrated.version, 4);
        assert!(migrated.service_sync.is_empty());
        assert_eq!(migrated.transfer_rules.len(), 2);
        assert!(
            migrated
                .transfer_rules
                .values()
                .all(|rule| rule.enabled && rule.legacy_receipts && rule.account == 1)
        );
        assert_eq!(
            migrated.transfer_rules[&1].plan.direction,
            photos_library::catalog::SyncDirection::Upload
        );
        assert_eq!(
            migrated.transfer_rules[&2].plan.direction,
            photos_library::catalog::SyncDirection::Download
        );
        let saved = std::fs::read(&path).expect("sync bytes");
        session.catalog.version = 2;
        assert!(publish(&session.authority, &session.catalog).is_err());
        assert_eq!(std::fs::read(&path).expect("preserved sync bytes"), saved);
        session.catalog.version = 3;
        session
            .catalog
            .service_sync
            .get_mut(&1)
            .expect("plan")
            .folder = "relative".into();
        assert!(publish(&session.authority, &session.catalog).is_err());
        assert_eq!(std::fs::read(&path).expect("preserved sync bytes"), saved);
    }

    #[test]
    fn persistence_competing_sessions_cannot_overwrite_each_other() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let path = sandbox.path().join("catalog.json");
        let mut first = open(&path).expect("session");
        let mut second = open(&path).expect("second session");
        first.catalog.create_album("first writer").expect("album");
        second.catalog.create_album("stale writer").expect("album");
        publish(&first.authority, &first.catalog).expect("first publish");
        assert!(publish(&second.authority, &second.catalog).is_err());
        assert_eq!(load(&path).expect("catalog").albums[0].name, "first writer");
        let third = open(&path).expect("third session");
        first.catalog.create_album("still owned").expect("album");
        publish(&first.authority, &first.catalog).expect("second publication");
        assert!(publish(&third.authority, &third.catalog).is_err());
        assert_eq!(load(&path).expect("catalog").albums.len(), 2);
    }

    #[test]
    fn persistence_damaged_catalog_and_substituted_file_remain_untouched() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let path = sandbox.path().join("catalog.json");
        let session = open(&path).expect("session");
        publish(&session.authority, &session.catalog).expect("publish");
        let damaged = format!("{HEADER}{{not json}}");
        let replacement = sandbox.path().join("replacement");
        std::fs::write(&replacement, damaged.as_bytes()).expect("damaged");
        std::fs::rename(replacement, &path).expect("substitution");
        assert!(publish(&session.authority, &Catalog::default()).is_err());
        assert_eq!(std::fs::read(&path).expect("read"), damaged.as_bytes());
        assert!(open(&path).is_err());
        assert_eq!(std::fs::read(&path).expect("read"), damaged.as_bytes());
    }

    #[test]
    fn persistence_invalid_album_identity_is_rejected_with_valid_control() {
        let mut catalog = Catalog::default();
        catalog.create_album("album").expect("album");
        assert!(parse(&serde_json::to_string(&catalog).expect("json")).is_ok());
        catalog.next_album = 1;
        assert!(parse(&serde_json::to_string(&catalog).expect("json")).is_err());
        catalog.next_album = 3;
        catalog.albums.push(catalog.albums[0].clone());
        assert!(parse(&serde_json::to_string(&catalog).expect("json")).is_err());
    }
}
