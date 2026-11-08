use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use iced::widget::image::Handle;
use image::{ImageEncoder, RgbaImage};
use photos_library::catalog::{Photo, PhotoId};
use photos_library::edit::EditRecipe;

const MAX_ENTRY: u64 = 2 * 1024 * 1024;
pub(crate) const DEFAULT_MB: u32 = 30;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Usage {
    pub bytes: u64,
    pub entries: usize,
}

pub(crate) struct Cache {
    root: PathBuf,
    budget: AtomicU64,
    generation: AtomicU64,
    visible: Mutex<HashSet<String>>,
    index: Mutex<Option<Index>>,
}
struct Entry {
    path: PathBuf,
    bytes: u64,
    used: SystemTime,
}
struct Index {
    entries: HashMap<String, Entry>,
    generation: u64,
    bytes: u64,
    _lock: File,
}

impl Cache {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            budget: AtomicU64::new(DEFAULT_MB as u64 * 1_000_000),
            generation: AtomicU64::new(0),
            visible: Mutex::new(HashSet::new()),
            index: Mutex::new(None),
        }
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub(crate) fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
    pub(crate) fn set_limit(&self, mb: u32) {
        self.budget
            .store(u64::from(mb.min(2048)) * 1_000_000, Ordering::Release);
    }
    pub(crate) fn protect(&self, keys: impl Iterator<Item = String>) -> bool {
        let next = keys.map(|key| name(&key)).collect();
        let mut visible = self.visible.lock().unwrap_or_else(|e| e.into_inner());
        if *visible == next {
            false
        } else {
            *visible = next;
            true
        }
    }
    fn with_index<T>(&self, operation: impl FnOnce(&mut Index) -> Result<T>) -> Result<T> {
        let mut held = self
            .index
            .lock()
            .map_err(|_| anyhow::anyhow!("thumbnail cache lock poisoned"))?;
        if held.is_none() {
            std::fs::create_dir_all(&self.root)?;
            anyhow::ensure!(
                std::fs::symlink_metadata(&self.root)?.file_type().is_dir(),
                "thumbnail cache must be a private directory"
            );
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700))?;
            let lock_path = self.root.join(".lock");
            let lock = match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&lock_path)
            {
                Ok(lock) => lock,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    photos_library::raster::open_media(&lock_path)?
                }
                Err(e) => return Err(e.into()),
            };
            lock.try_lock()
                .context("thumbnail cache is already in use")?;
            let mut entries = HashMap::new();
            for item in std::fs::read_dir(&self.root)? {
                let item = item?;
                if !item.file_type()?.is_file() {
                    continue;
                }
                let path = item.path();
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if stem.len() != 64
                    || !stem.bytes().all(|b| b.is_ascii_hexdigit())
                    || !matches!(
                        path.extension().and_then(|s| s.to_str()),
                        Some("jpg" | "png")
                    )
                {
                    continue;
                }
                let meta = item.metadata()?;
                entries.insert(
                    stem.to_owned(),
                    Entry {
                        path,
                        bytes: charge(meta.len()),
                        used: meta.modified().unwrap_or(UNIX_EPOCH),
                    },
                );
            }
            let bytes = entries.values().map(|entry| entry.bytes).sum();
            *held = Some(Index {
                entries,
                bytes,
                generation: 0,
                _lock: lock,
            });
        }
        let index = held.as_mut().context("thumbnail cache unavailable")?;
        if index.generation != self.generation() {
            for entry in index.entries.values() {
                match std::fs::remove_file(&entry.path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            index.entries.clear();
            index.bytes = 0;
            index.generation = self.generation();
        }
        let visible = self
            .visible
            .lock()
            .map_err(|_| anyhow::anyhow!("thumbnail visibility lock poisoned"))?
            .clone();
        let now = SystemTime::now();
        for key in visible.iter() {
            let Some(entry) = index.entries.get_mut(key) else {
                continue;
            };
            if now.duration_since(entry.used).unwrap_or_default().as_secs() >= 2 {
                if let Ok(file) = photos_library::raster::open_media(&entry.path) {
                    let _ = file.set_modified(now);
                }
                entry.used = now;
            }
        }
        prune(index, self.budget.load(Ordering::Acquire), &visible)?;
        let result = operation(index);
        let visible = self
            .visible
            .lock()
            .map_err(|_| anyhow::anyhow!("thumbnail visibility lock poisoned"))?
            .clone();
        prune(index, self.budget.load(Ordering::Acquire), &visible)?;
        result
    }
    pub(crate) fn maintain(&self) -> Result<Usage> {
        self.with_index(|index| Ok(usage(index)))
    }
    fn get(&self, key: &str) -> Result<Option<Handle>> {
        self.with_index(|index| {
            let key = name(key);
            let Some(entry) = index.entries.get_mut(&key) else {
                return Ok(None);
            };
            let result = (|| -> Result<Handle> {
                let mut bytes = Vec::new();
                photos_library::raster::open_media(&entry.path)?
                    .take(MAX_ENTRY + 1)
                    .read_to_end(&mut bytes)?;
                anyhow::ensure!(
                    bytes.len() as u64 <= MAX_ENTRY,
                    "cached thumbnail is too large"
                );
                let decoder = image::ImageReader::new(Cursor::new(&bytes))
                    .with_guessed_format()?
                    .into_decoder()?;
                use image::ImageDecoder;
                let (width, height) = decoder.dimensions();
                anyhow::ensure!(
                    width > 0 && height > 0 && width <= 512 && height <= 512,
                    "cached thumbnail dimensions are invalid"
                );
                let pixels = image::DynamicImage::from_decoder(decoder)?.into_rgba8();
                Ok(super::handle(pixels))
            })();
            match result {
                Ok(handle) => {
                    entry.used = SystemTime::now();
                    if let Ok(file) = photos_library::raster::open_media(&entry.path) {
                        let _ = file.set_modified(entry.used);
                    }
                    Ok(Some(handle))
                }
                Err(_) => {
                    if let Some(entry) = index.entries.remove(&key) {
                        index.bytes = index.bytes.saturating_sub(entry.bytes);
                        let _ = std::fs::remove_file(entry.path);
                    }
                    Ok(None)
                }
            }
        })
    }
    fn put(&self, key: &str, handle: &Handle, generation: u64) -> Result<()> {
        if generation != self.generation() || self.budget.load(Ordering::Acquire) == 0 {
            return Ok(());
        }
        let (extension, bytes) = encode(handle)?;
        self.with_index(|index| {
            if generation != self.generation()
                || charge(bytes.len() as u64) > self.budget.load(Ordering::Acquire)
            {
                return Ok(());
            }
            let key = name(key);
            if index.entries.contains_key(&key) {
                return Ok(());
            }
            let path = self.root.join(format!("{key}.{extension}"));
            enzui_iced_core::user_file::save_with(
                &path,
                enzui_iced_core::user_file::SavePolicy::CreateNew,
                |output| output.write_all(&bytes),
            )?;
            index.bytes += charge(bytes.len() as u64);
            index.entries.insert(
                key,
                Entry {
                    path,
                    bytes: charge(bytes.len() as u64),
                    used: SystemTime::now(),
                },
            );
            Ok(())
        })
    }
    pub(crate) fn thumbnail(
        &self,
        photo: &Photo,
        recipe: Option<&EditRecipe>,
        limit: u32,
        generation: u64,
        cancelled: impl Fn() -> bool + Sync,
    ) -> Result<Handle> {
        anyhow::ensure!(!cancelled(), "thumbnail cancelled");
        let key = super::thumbnail_key(photo, recipe, limit);
        let meta = photos_library::raster::open_media(&photo.path)?.metadata()?;
        let modified = meta.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
        anyhow::ensure!(
            meta.len() == photo.bytes && modified == photo.modified_ns,
            "photo changed; rescan the folder"
        );
        if generation == self.generation()
            && let Ok(Some(handle)) = self.get(&key)
        {
            return Ok(handle);
        }
        let handle = super::thumbnail_sized(photo, recipe, limit, &cancelled)?;
        if !cancelled()
            && let Err(error) = self.put(&key, &handle, generation)
        {
            tracing::debug!(%error, "thumbnail disk cache unavailable");
        }
        Ok(handle)
    }
}
fn charge(bytes: u64) -> u64 {
    bytes.div_ceil(4096) * 4096
}
fn name(key: &str) -> String {
    PhotoId::for_path(Path::new(key)).0
}
fn usage(index: &Index) -> Usage {
    Usage {
        bytes: index.bytes,
        entries: index.entries.len(),
    }
}
fn prune(index: &mut Index, budget: u64, visible: &HashSet<String>) -> Result<()> {
    let mut bytes = usage(index).bytes;
    if bytes <= budget {
        return Ok(());
    }
    let mut order: Vec<_> = index
        .entries
        .iter()
        .map(|(key, entry)| (visible.contains(key), entry.used, key.clone()))
        .collect();
    order.sort();
    for (_, _, key) in order {
        if bytes <= budget {
            break;
        }
        if let Some(entry) = index.entries.get(&key) {
            match std::fs::remove_file(&entry.path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        if let Some(entry) = index.entries.remove(&key) {
            bytes = bytes.saturating_sub(entry.bytes);
            index.bytes = bytes;
        }
    }
    Ok(())
}
pub(crate) fn encode(handle: &Handle) -> Result<(&'static str, Vec<u8>)> {
    let Handle::Rgba {
        width,
        height,
        pixels,
        ..
    } = handle
    else {
        anyhow::bail!("thumbnail must contain decoded pixels");
    };
    let pixels = RgbaImage::from_raw(*width, *height, pixels.to_vec())
        .context("invalid thumbnail pixels")?;
    let mut bytes = Vec::new();
    if pixels.pixels().any(|p| p[3] != 255) {
        image::codecs::png::PngEncoder::new(&mut bytes).write_image(
            pixels.as_raw(),
            *width,
            *height,
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(("png", bytes))
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 82)
            .encode_image(&image::DynamicImage::ImageRgba8(pixels))?;
        Ok(("jpg", bytes))
    }
}

pub(crate) fn measure(paths: &crate::platform::Paths, output: &Path) -> Result<()> {
    let text = std::fs::read_to_string(&paths.catalog)?;
    let catalog: photos_library::catalog::Catalog = serde_json::from_str(
        text.strip_prefix("enzui-iced photos-catalog v1\nbody\n")
            .context("invalid catalog")?,
    )?;
    let mut photos = photos_library::scan::scan(
        &super::roots(&paths.photos, &catalog),
        Default::default(),
        || false,
    )
    .photos;
    photos.sort_by_key(|p| p.captured_at);
    let store = Cache::new(paths.thumbnail_cache());
    store.set_limit(catalog.settings.thumbnail_cache_mb);
    let mut small = 0u64;
    let mut large = 0u64;
    let mut decoded_small = 0u64;
    let mut decoded_large = 0u64;
    let mut success = 0;
    let mut decode_seconds = 0.0;
    let mut errors = Vec::new();
    let started = std::time::Instant::now();
    for (index, photo) in photos.iter().enumerate() {
        let result = (|| -> Result<()> {
            let decode_started = std::time::Instant::now();
            let full = super::thumbnail(photo, catalog.edits.get(&photo.id), || false)?;
            decode_seconds += decode_started.elapsed().as_secs_f64();
            let Handle::Rgba {
                width,
                height,
                pixels,
                ..
            } = &full
            else {
                anyhow::bail!("invalid thumbnail");
            };
            decoded_large += pixels.len() as u64;
            large += encode(&full)?.1.len() as u64;
            let reduced =
                RgbaImage::from_raw(*width, *height, pixels.to_vec()).context("invalid preview")?;
            let reduced = super::handle(super::reduced(&reduced, 256));
            let Handle::Rgba { pixels, .. } = &reduced else {
                anyhow::bail!("invalid preview");
            };
            decoded_small += pixels.len() as u64;
            small += encode(&reduced)?.1.len() as u64;
            store.put(
                &super::thumbnail_key(photo, catalog.edits.get(&photo.id), 256),
                &reduced,
                0,
            )?;
            success += 1;
            Ok(())
        })();
        if let Err(error) = result {
            errors.push(format!("{}: {error}", photo.id.0));
        }
        if (index + 1) % 100 == 0 {
            eprintln!("Measured {}/{} previews", index + 1, photos.len());
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    let usage = store.maintain()?;
    let warm_started = std::time::Instant::now();
    let mut warm_count = 0;
    for photo in photos.iter().rev().take(100) {
        if store
            .get(&super::thumbnail_key(
                photo,
                catalog.edits.get(&photo.id),
                256,
            ))?
            .is_some()
        {
            warm_count += 1;
        }
    }
    let report = serde_json::json!({ "discovered": photos.len(), "measured": success, "encoded_256_bytes": small, "encoded_512_bytes": large, "decoded_256_bytes": decoded_small, "decoded_512_bytes": decoded_large, "measurement_seconds": elapsed, "cold_decode_seconds": decode_seconds, "cache_limit_mb": catalog.settings.thumbnail_cache_mb, "cache_bytes": usage.bytes, "cache_entries": usage.entries, "warm_sample_count": warm_count, "warm_sample_seconds": warm_started.elapsed().as_secs_f64(), "errors": errors });
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    println!("{report}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn handle() -> Handle {
        super::super::handle(RgbaImage::from_fn(64, 48, |x, y| {
            Rgba([x as u8, y as u8, 90, if x == 0 { 100 } else { 255 }])
        }))
    }
    fn photo(root: &Path) -> Photo {
        let pixels = RgbaImage::from_fn(96, 72, |x, y| {
            Rgba([x as u8, y as u8, 150, if x == 0 { 100 } else { 255 }])
        });
        pixels.save(root.join("photo.png")).expect("fixture");
        photos_library::scan::scan(&[root.to_owned()], Default::default(), || false)
            .photos
            .into_iter()
            .next()
            .expect("photo")
    }
    #[test]
    fn thumbnail_cache_restart_reads_saved_preview_without_decoding_original_again() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let photo = photo(sandbox.path());
        let root = sandbox.path().join("cache");
        let cache = Cache::new(root.clone());
        let expected = cache
            .thumbnail(&photo, None, 256, 0, || false)
            .expect("cold preview");
        assert_eq!(cache.maintain().expect("usage").entries, 1);
        drop(cache);
        let source = OpenOptions::new()
            .write(true)
            .open(&photo.path)
            .expect("fixture");
        std::fs::write(&photo.path, vec![0; photo.bytes as usize]).expect("poison decoder input");
        source
            .set_modified(UNIX_EPOCH + std::time::Duration::from_nanos(photo.modified_ns as u64))
            .expect("retain fingerprint");
        let restarted = Cache::new(root);
        let actual = restarted
            .thumbnail(&photo, None, 256, 0, || false)
            .expect("disk hit must avoid original decoder");
        assert_eq!(
            super::super::pixels_from_handle(expected).expect("valid test fixture"),
            super::super::pixels_from_handle(actual).expect("valid test fixture")
        );
    }
    #[test]
    fn thumbnail_cache_evicts_oldest_unseen_entries_and_obeys_byte_limit() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let cache = Cache::new(sandbox.path().join("cache"));
        let handle = handle();
        let cost = charge(encode(&handle).expect("encode").1.len() as u64);
        cache.budget.store(cost * 2, Ordering::Release);
        cache.put("a", &handle, 0).expect("valid test fixture");
        cache.put("b", &handle, 0).expect("valid test fixture");
        cache
            .with_index(|index| {
                index
                    .entries
                    .get_mut(&name("a"))
                    .expect("valid test fixture")
                    .used = UNIX_EPOCH;
                Ok(())
            })
            .expect("valid test fixture");
        cache.protect(["a".to_owned()].into_iter());
        cache.put("c", &handle, 0).expect("valid test fixture");
        assert!(
            cache.get("a").expect("valid test fixture").is_some(),
            "visible entries survive ahead of newer unseen entries"
        );
        assert!(cache.get("b").expect("valid test fixture").is_none());
        assert!(cache.get("c").expect("valid test fixture").is_some());
        assert!(cache.maintain().expect("valid test fixture").bytes <= cost * 2);
        cache.protect(std::iter::empty());
        cache
            .with_index(|index| {
                index
                    .entries
                    .get_mut(&name("c"))
                    .expect("valid test fixture")
                    .used = UNIX_EPOCH;
                Ok(())
            })
            .expect("valid test fixture");
        cache.put("d", &handle, 0).expect("valid test fixture");
        assert!(
            cache.get("c").expect("valid test fixture").is_none(),
            "least recently viewed is evicted"
        );
        assert!(cache.get("a").expect("valid test fixture").is_some());
        assert_eq!(cache.maintain().expect("valid test fixture").entries, 2);
    }
    #[test]
    fn thumbnail_cache_clear_rejects_old_work_and_shrinking_prunes_persisted_files() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let root = sandbox.path().join("cache");
        let cache = Cache::new(root.clone());
        cache.put("a", &handle(), 0).expect("valid test fixture");
        drop(cache);
        let cache = Cache::new(root);
        cache.invalidate();
        assert_eq!(
            cache.maintain().expect("valid test fixture").bytes,
            0,
            "clear also works before index initialization"
        );
        cache.put("old", &handle(), 0).expect("valid test fixture");
        assert_eq!(cache.maintain().expect("valid test fixture").entries, 0);
        cache
            .put("new", &handle(), cache.generation())
            .expect("valid test fixture");
        assert_eq!(cache.maintain().expect("valid test fixture").entries, 1);
        cache.set_limit(0);
        assert_eq!(cache.maintain().expect("valid test fixture").bytes, 0);
        assert!(cache.get("new").expect("valid test fixture").is_none());
    }
    #[test]
    fn thumbnail_cache_corruption_regenerates_and_edits_use_distinct_keys() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let photo = photo(sandbox.path());
        let original = std::fs::read(&photo.path).expect("valid test fixture");
        let cache = Cache::new(sandbox.path().join("cache"));
        cache
            .thumbnail(&photo, None, 256, 0, || false)
            .expect("valid test fixture");
        let key = name(&super::super::thumbnail_key(&photo, None, 256));
        let path = cache
            .with_index(|index| Ok(index.entries[&key].path.clone()))
            .expect("valid test fixture");
        std::fs::write(path, b"broken preview").expect("valid test fixture");
        assert!(cache.thumbnail(&photo, None, 256, 0, || false).is_ok());
        let recipe = EditRecipe {
            quarter_turns: 1,
            ..Default::default()
        };
        let rotated = cache
            .thumbnail(&photo, Some(&recipe), 256, 0, || false)
            .expect("valid test fixture");
        let Handle::Rgba { width, height, .. } = rotated else {
            panic!("decoded handle");
        };
        assert_eq!((width, height), (72, 96));
        assert_eq!(cache.maintain().expect("valid test fixture").entries, 2);
        assert!(cache.thumbnail(&photo, None, 256, 0, || true).is_err());
        assert_eq!(
            std::fs::read(&photo.path).expect("valid test fixture"),
            original
        );
    }
}
