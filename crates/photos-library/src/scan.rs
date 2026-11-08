use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::catalog::{Photo, PhotoId};
use crate::metadata;
use crate::raster::MAX_ENCODED_BYTES;

/// Discovery caps prevent unexpected mount trees from monopolizing the process.
#[derive(Debug, Clone, Copy)]
pub struct ScanLimits {
    pub max_photos: usize,
    pub max_entries: usize,
    pub max_depth: usize,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            max_photos: 100_000,
            max_entries: 300_000,
            max_depth: 32,
        }
    }
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub photos: Vec<Photo>,
    pub warnings: Vec<String>,
    pub skipped: usize,
    pub truncated: bool,
    pub cancelled: bool,
}

/// Recursively discovers supported photos without following child symlinks.
/// Roots may be symlinks selected by the user; overlapping roots are deduplicated.
#[must_use]
pub fn scan(roots: &[PathBuf], limits: ScanLimits, cancelled: impl Fn() -> bool) -> ScanReport {
    let mut report = ScanReport::default();
    let mut visited = HashSet::new();
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().rev().cloned().map(|p| (p, 0)).collect();
    let mut entries = 0usize;
    while let Some((path, depth)) = stack.pop() {
        if cancelled() {
            report.cancelled = true;
            break;
        }
        if entries >= limits.max_entries || report.photos.len() >= limits.max_photos {
            report.truncated = true;
            break;
        }
        entries += 1;
        if depth > 0 && fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink())
        {
            report.skipped += 1;
            continue;
        }
        let Ok(path) = fs::canonicalize(&path) else {
            warn(&mut report, format!("Cannot access {}", path.display()));
            continue;
        };
        if !visited.insert(path.clone()) {
            continue;
        }
        let Ok(meta) = fs::metadata(&path) else {
            report.skipped += 1;
            continue;
        };
        if meta.is_dir() {
            if depth >= limits.max_depth {
                report.truncated = true;
                continue;
            }
            let children = match fs::read_dir(&path) {
                Ok(children) => children,
                Err(error) => {
                    warn(
                        &mut report,
                        format!("Cannot read {}: {error}", path.display()),
                    );
                    continue;
                }
            };
            for child in children {
                if cancelled() {
                    report.cancelled = true;
                    break;
                }
                let Ok(child) = child else {
                    report.skipped += 1;
                    continue;
                };
                let Ok(kind) = child.file_type() else {
                    report.skipped += 1;
                    continue;
                };
                if kind.is_symlink() || (!kind.is_dir() && !kind.is_file()) {
                    continue;
                }
                if child.file_name().as_encoded_bytes().starts_with(b".") {
                    continue;
                }
                if stack.len().saturating_add(entries) >= limits.max_entries {
                    report.truncated = true;
                    break;
                }
                stack.push((child.path(), depth + 1));
            }
        } else if meta.is_file() && is_supported_path(&path) {
            if is_video_path(&path) && paired_movie(&path) {
                continue;
            }
            match photo(&path, &meta) {
                Ok(photo) => report.photos.push(photo),
                Err(_) => report.skipped += 1,
            }
        }
    }
    report.photos.sort_by(|a, b| {
        b.captured_at
            .cmp(&a.captured_at)
            .then_with(|| a.path.cmp(&b.path))
    });
    report
}

fn warn(report: &mut ScanReport, warning: String) {
    if report.warnings.len() < 20 {
        report.warnings.push(warning);
    }
    report.skipped += 1;
}

/// Discoverable photo and video formats, including optional host-provided decoders.
#[must_use]
pub fn is_supported_path(path: &Path) -> bool {
    if is_video_path(path) {
        return true;
    }
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "jpg"
                    | "jpeg"
                    | "jpe"
                    | "png"
                    | "apng"
                    | "webp"
                    | "bmp"
                    | "heic"
                    | "heif"
                    | "heics"
                    | "heifs"
                    | "avif"
                    | "jxl"
                    | "gif"
                    | "tif"
                    | "tiff"
                    | "ico"
                    | "tga"
                    | "ppm"
                    | "pgm"
                    | "pbm"
                    | "pam"
                    | "pnm"
                    | "qoi"
                    | "hdr"
                    | "exr"
                    | "psd"
                    | "psb"
                    | "dng"
                    | "cr2"
                    | "cr3"
                    | "crw"
                    | "nef"
                    | "nrw"
                    | "arw"
                    | "srf"
                    | "sr2"
                    | "raf"
                    | "orf"
                    | "rw2"
                    | "pef"
                    | "ptx"
                    | "rwl"
                    | "srw"
                    | "x3f"
                    | "raw"
                    | "3fr"
                    | "fff"
                    | "iiq"
                    | "kdc"
                    | "dcr"
                    | "mos"
            )
        })
}

/// Whether opening the file requires developing camera sensor data.
#[must_use]
pub fn is_camera_raw(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "dng"
                    | "cr2"
                    | "cr3"
                    | "crw"
                    | "nef"
                    | "nrw"
                    | "arw"
                    | "srf"
                    | "sr2"
                    | "raf"
                    | "orf"
                    | "rw2"
                    | "pef"
                    | "ptx"
                    | "rwl"
                    | "srw"
                    | "x3f"
                    | "raw"
                    | "3fr"
                    | "fff"
                    | "iiq"
                    | "kdc"
                    | "dcr"
                    | "mos"
            )
        })
}

/// Common video containers; decoder availability is checked when opening the media.
#[must_use]
pub fn is_video_path(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "mov"
                | "mp4"
                | "m4v"
                | "mkv"
                | "webm"
                | "avi"
                | "mpg"
                | "mpeg"
                | "mts"
                | "m2ts"
                | "ts"
                | "3gp"
                | "3g2"
                | "ogv"
                | "vob"
                | "wmv"
                | "flv"
        )
    })
}

fn paired_movie(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "mov" | "mp4")
        && ["jpg", "JPG", "jpeg", "JPEG", "heic", "HEIC", "heif", "HEIF"]
            .into_iter()
            .any(|ext| {
                fs::symlink_metadata(path.with_extension(ext))
                    .is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
            })
}

/// Inspects one explicitly selected media file without scanning its parent folder.
///
/// # Errors
/// Rejects unsupported paths, nonregular files, oversized media and unreadable headers.
pub fn inspect_file(path: &Path) -> Result<Photo, Box<dyn std::error::Error>> {
    let path = fs::canonicalize(path)?;
    if !is_supported_path(&path) {
        return Err("unsupported media file".into());
    }
    let metadata = crate::raster::open_media(&path)?.metadata()?;
    photo(&path, &metadata)
}

fn photo(path: &Path, meta: &fs::Metadata) -> Result<Photo, Box<dyn std::error::Error>> {
    if meta.len()
        > if is_video_path(path) {
            crate::raster::MAX_MEDIA_BYTES
        } else {
            MAX_ENCODED_BYTES
        }
    {
        return Err("photo is too large".into());
    }
    let prefix = if is_video_path(path) {
        Vec::new()
    } else {
        crate::raster::read_prefix(path, 4 * 1024 * 1024)?
    };
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (mut width, mut height) = if is_video_path(path)
        || prefix.get(4..8) == Some(b"ftyp")
        || is_camera_raw(path)
        || matches!(extension.as_str(), "jxl" | "psd" | "psb" | "tga")
    {
        (0, 0)
    } else {
        image::ImageReader::new(std::io::Cursor::new(&prefix))
            .with_guessed_format()?
            .into_dimensions()?
    };
    let exif = metadata::inspect(&prefix);
    if exif
        .orientation
        .is_some_and(|orientation| matches!(orientation, 5..=8))
    {
        std::mem::swap(&mut width, &mut height);
    }
    let modified = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Ok(Photo {
        id: PhotoId::for_path(path),
        path: path.to_owned(),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        width,
        height,
        bytes: meta.len(),
        modified_ns: modified.as_nanos(),
        captured_at: exif
            .captured_at
            .unwrap_or(i64::try_from(modified.as_secs()).unwrap_or(i64::MAX)),
        camera: exif.camera,
        has_location: exif.has_location,
        format: extension,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn seed(path: &Path) {
        RgbaImage::from_pixel(9, 7, Rgba([120, 80, 30, 255]))
            .save(path)
            .expect("image");
    }

    #[test]
    fn video_scan_groups_live_pairs_and_admits_large_standalone_movies() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        seed(&sandbox.path().join("still.png"));
        image::RgbImage::new(4, 3)
            .save(sandbox.path().join("Live.jpg"))
            .expect("still");
        fs::write(sandbox.path().join("Live.MOV"), b"companion").expect("pair");
        let movie = sandbox.path().join("large.mp4");
        fs::File::create(&movie)
            .expect("movie")
            .set_len(MAX_ENCODED_BYTES + 100)
            .expect("length");
        let report = scan(&[sandbox.path().to_owned()], Default::default(), || false);
        assert_eq!(report.photos.len(), 3);
        assert!(report.photos.iter().any(|p| p.path == movie));
        assert!(!report.photos.iter().any(|p| p.name == "Live.MOV"));
        fs::remove_file(sandbox.path().join("Live.jpg")).expect("still removed");
        let report = scan(&[sandbox.path().to_owned()], Default::default(), || false);
        assert!(report.photos.iter().any(|p| p.name == "Live.MOV"));
        for ext in [
            "mov", "mp4", "m4v", "mkv", "webm", "avi", "mpg", "mpeg", "mts", "m2ts", "ts", "3gp",
            "3g2", "ogv", "vob", "wmv", "flv",
        ] {
            assert!(is_supported_path(&PathBuf::from(format!("clip.{ext}"))));
        }
    }

    #[test]
    fn scan_cancellation_and_capacity_are_reported() {
        let dir = tempfile::tempdir().expect("sandbox");
        seed(&dir.path().join("one.png"));
        seed(&dir.path().join("two.png"));
        let result = scan(
            &[dir.path().to_owned()],
            ScanLimits {
                max_photos: 1,
                ..Default::default()
            },
            || false,
        );
        assert_eq!(result.photos.len(), 1);
        assert!(result.truncated);
        assert!(scan(&[dir.path().to_owned()], ScanLimits::default(), || true).cancelled);
    }

    #[cfg(unix)]
    #[test]
    fn scan_child_symlink_does_not_escape_the_selected_folder() {
        let dir = tempfile::tempdir().expect("sandbox");
        let other = tempfile::tempdir().expect("sandbox");
        seed(&other.path().join("private.png"));
        std::os::unix::fs::symlink(other.path(), dir.path().join("outside")).expect("link");
        assert!(
            scan(&[dir.path().to_owned()], ScanLimits::default(), || false)
                .photos
                .is_empty()
        );
    }
}
