use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::edit::EditRecipe;

/// Stable identity of a file's canonical location, independent of library order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PhotoId(pub String);

impl PhotoId {
    /// Hashes the platform-native pathname without lossy Unicode conversion.
    #[must_use]
    pub fn for_path(path: &Path) -> Self {
        Self(format!(
            "{:x}",
            Sha256::digest(path.as_os_str().as_encoded_bytes())
        ))
    }
}

/// Header and filesystem facts about a discovered photo; no pixels are retained.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Photo {
    pub id: PhotoId,
    pub path: PathBuf,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub modified_ns: u128,
    pub captured_at: i64,
    pub camera: Option<String>,
    pub has_location: bool,
    pub format: String,
}

/// A named, virtual collection; removing an album never removes its photos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Album {
    pub id: u64,
    pub name: String,
    pub photos: BTreeSet<PhotoId>,
}

/// Date headings inserted between gallery rows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GallerySections {
    #[default]
    None,
    Month,
    Day,
}

/// Folder structure for newly imported copies.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportLayout {
    #[default]
    Flat,
    PerImport,
    YearMonth,
}

/// Persistent gallery and import preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhotoSettings {
    pub sections: GallerySections,
    pub oldest_first: bool,
    pub contain_thumbnails: bool,
    pub show_names: bool,
    pub thumbnail_size: u8,
    pub shortcut_overrides: String,
    pub thumbnail_cache_mb: u32,
    pub import_destination: Option<PathBuf>,
    pub import_layout: ImportLayout,
}

impl Default for PhotoSettings {
    fn default() -> Self {
        Self {
            sections: GallerySections::None,
            oldest_first: false,
            contain_thumbnails: false,
            show_names: true,
            thumbnail_size: 4,
            shortcut_overrides: String::new(),
            thumbnail_cache_mb: 30,
            import_destination: None,
            import_layout: ImportLayout::Flat,
        }
    }
}

/// Additive transfer direction; deletions are never propagated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncDirection {
    Download,
    Upload,
    Both,
}

/// Folder and remote scope for additive photo-service transfers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceSync {
    pub folder: PathBuf,
    pub direction: SyncDirection,
    pub album: Option<String>,
}

/// Local media selected by an automatic upload rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferSource {
    Library,
    Album(u64),
    Folder,
}

/// An independently enabled, additive transfer between a local scope and a server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRule {
    pub account: u64,
    pub enabled: bool,
    pub source: TransferSource,
    pub remote_label: String,
    pub plan: ServiceSync,
    pub legacy_receipts: bool,
}

/// A saved photo-service connection, without its credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub id: u64,
    pub name: String,
    pub provider: ServiceProvider,
    pub url: String,
}

/// Remote photo protocols supported by the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceProvider {
    Immich,
}

/// Durable user choices, separate from disposable filesystem discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Catalog {
    pub version: u32,
    pub viewer_info: bool,
    pub settings: PhotoSettings,
    pub sources: Vec<PathBuf>,
    pub favorites: BTreeSet<PhotoId>,
    pub albums: Vec<Album>,
    pub edits: BTreeMap<PhotoId, EditRecipe>,
    pub imported: BTreeSet<PhotoId>,
    pub hidden: BTreeSet<PhotoId>,
    pub next_album: u64,
    pub services: Vec<ServiceAccount>,
    pub next_service: u64,
    pub service_sync: BTreeMap<u64, ServiceSync>,
    pub transfer_rules: BTreeMap<u64, TransferRule>,
    pub next_transfer_rule: u64,
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            viewer_info: false,
            settings: PhotoSettings::default(),
            sources: Vec::new(),
            favorites: BTreeSet::new(),
            albums: Vec::new(),
            edits: BTreeMap::new(),
            imported: BTreeSet::new(),
            hidden: BTreeSet::new(),
            next_album: 1,
            services: Vec::new(),
            next_service: 1,
            service_sync: BTreeMap::new(),
            transfer_rules: BTreeMap::new(),
            next_transfer_rule: 1,
        }
    }
}

impl Catalog {
    /// Adds a nonempty album name; duplicate names are permitted and identities are unique.
    ///
    /// # Errors
    /// Rejects blank or overlong names and exhausted identities.
    pub fn create_album(&mut self, name: &str) -> Result<u64, &'static str> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
            return Err("album name must contain 1–100 characters");
        }
        let id = self.next_album;
        let next = id.checked_add(1).ok_or("album identities exhausted")?;
        self.albums.push(Album {
            id,
            name: name.to_owned(),
            photos: BTreeSet::new(),
        });
        self.next_album = next;
        Ok(id)
    }

    /// Toggles one photo's favorite membership and returns its new state.
    pub fn toggle_favorite(&mut self, id: PhotoId) -> bool {
        if self.favorites.remove(&id) {
            false
        } else {
            self.favorites.insert(id);
            true
        }
    }

    /// Renames a collection without changing its identity or members.
    ///
    /// # Errors
    /// Rejects blank, overlong, or control-character names and unknown identities.
    pub fn rename_album(&mut self, id: u64, name: &str) -> Result<(), &'static str> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
            return Err("album name must contain 1–100 characters");
        }
        let album = self
            .albums
            .iter_mut()
            .find(|album| album.id == id)
            .ok_or("album no longer exists")?;
        album.name = name.to_owned();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_blank_name_and_exhausted_id_do_not_mutate() {
        let mut catalog = Catalog::default();
        assert!(catalog.create_album(" \n ").is_err());
        catalog.next_album = u64::MAX;
        assert!(catalog.create_album("Holiday").is_err());
        assert!(catalog.albums.is_empty());
    }
}
