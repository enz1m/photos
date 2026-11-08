use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use photos_library::{raster, scan};

#[derive(Debug, Default)]
pub(crate) struct ImportReport {
    pub paths: Vec<PathBuf>,
    pub skipped: usize,
    pub errors: Vec<String>,
    pub cancelled: bool,
    pub destination: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ImportProgress {
    pub completed: usize,
    pub total: usize,
}

#[cfg(test)]
pub(crate) fn from_folder(
    folder: &Path,
    destination: &Path,
    cancelled: impl Fn() -> bool,
) -> Result<ImportReport> {
    from_folder_with_progress(folder, destination, cancelled, |_| {})
}

#[cfg(test)]
pub(crate) fn from_folder_with_progress(
    folder: &Path,
    destination: &Path,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(ImportProgress),
) -> Result<ImportReport> {
    from_folder_organized(
        folder,
        destination,
        photos_library::catalog::ImportLayout::Flat,
        cancelled,
        progress,
    )
}

pub(crate) fn from_folder_organized(
    folder: &Path,
    destination: &Path,
    layout: photos_library::catalog::ImportLayout,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(ImportProgress),
) -> Result<ImportReport> {
    let source = fs::canonicalize(folder).context("cannot access import folder")?;
    anyhow::ensure!(source.is_dir(), "choose an import folder, not a file");
    fs::create_dir_all(destination).context("cannot create Photos folder")?;
    let destination = fs::canonicalize(destination)?;
    anyhow::ensure!(
        !destination.starts_with(&source) && !source.starts_with(&destination),
        "choose a folder outside your Photos library"
    );
    let found = scan::scan(
        &[source],
        scan::ScanLimits {
            max_photos: 10_000,
            ..Default::default()
        },
        &cancelled,
    );
    import_scanned(found, &destination, layout, folder, cancelled, progress)
}

pub(crate) const ENCODED_IMAGE_LIMIT: usize = 16 * 1024 * 1024;
pub(crate) const IMAGE_MIME_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/tiff",
    "image/bmp",
    "image/avif",
    "image/heic",
    "image/heif",
];

pub(crate) fn from_encoded_image(
    bytes: &[u8],
    mime: &str,
    destination: &Path,
    layout: photos_library::catalog::ImportLayout,
    cancelled: impl Fn() -> bool + Sync,
    progress: impl Fn(ImportProgress),
) -> Result<ImportReport> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() <= ENCODED_IMAGE_LIMIT,
        "drop an encoded image of at most 16 MiB"
    );
    let (format, extension) = match mime {
        "image/png" => (Some(image::ImageFormat::Png), "png"),
        "image/jpeg" => (Some(image::ImageFormat::Jpeg), "jpg"),
        "image/webp" => (Some(image::ImageFormat::WebP), "webp"),
        "image/gif" => (Some(image::ImageFormat::Gif), "gif"),
        "image/tiff" => (Some(image::ImageFormat::Tiff), "tiff"),
        "image/bmp" => (Some(image::ImageFormat::Bmp), "bmp"),
        "image/avif" => (Some(image::ImageFormat::Avif), "avif"),
        "image/heic" => (None, "heic"),
        "image/heif" => (None, "heif"),
        _ => anyhow::bail!("unsupported dropped image format"),
    };
    if let Some(format) = format {
        anyhow::ensure!(
            image::guess_format(bytes)? == format,
            "dropped image does not match its declared format"
        );
    } else {
        anyhow::ensure!(
            bytes.get(4..8) == Some(b"ftyp"),
            "dropped image is not a HEIF container"
        );
    }
    anyhow::ensure!(!cancelled(), "image import cancelled");
    let directory = tempfile::Builder::new()
        .prefix("enz-photos-drop-")
        .tempdir()?;
    let path = directory.path().join(format!("Dropped image.{extension}"));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    for chunk in bytes.chunks(64 * 1024) {
        anyhow::ensure!(!cancelled(), "image import cancelled");
        file.write_all(chunk)?;
    }
    file.flush()?;
    if format.is_none() {
        super::formats::decode(&path, bytes, Some(1), &cancelled)?;
    }
    from_paths_organized(&[path], destination, layout, cancelled, progress)
}

pub(crate) fn from_paths_organized(
    paths: &[PathBuf],
    destination: &Path,
    layout: photos_library::catalog::ImportLayout,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(ImportProgress),
) -> Result<ImportReport> {
    anyhow::ensure!(
        !paths.is_empty() && paths.len() <= 128,
        "drop between 1 and 128 files or folders"
    );
    fs::create_dir_all(destination).context("cannot create Photos folder")?;
    let destination = fs::canonicalize(destination)?;
    let mut roots = Vec::with_capacity(paths.len());
    for path in paths {
        let source = fs::canonicalize(path).context("cannot access dropped media")?;
        anyhow::ensure!(
            source.is_dir() || scan::is_supported_path(&source),
            "unsupported dropped media: {}",
            source.display()
        );
        anyhow::ensure!(
            !source.starts_with(&destination) && !destination.starts_with(&source),
            "choose media outside the import destination"
        );
        roots.push(source);
    }
    let found = scan::scan(
        &roots,
        scan::ScanLimits {
            max_photos: 10_000,
            ..Default::default()
        },
        &cancelled,
    );
    import_scanned(
        found,
        &destination,
        layout,
        Path::new("Dropped media"),
        cancelled,
        progress,
    )
}

fn import_scanned(
    found: scan::ScanReport,
    destination: &Path,
    layout: photos_library::catalog::ImportLayout,
    folder: &Path,
    cancelled: impl Fn() -> bool,
    progress: impl Fn(ImportProgress),
) -> Result<ImportReport> {
    let mut report = ImportReport {
        destination: Some(destination.to_path_buf()),
        skipped: found.skipped,
        errors: found.warnings,
        cancelled: found.cancelled,
        ..Default::default()
    };
    if found.truncated {
        report
            .errors
            .push("Import limit reached; choose a smaller folder for remaining photos.".into());
    }
    let total = found.photos.len();
    progress(ImportProgress {
        completed: 0,
        total,
    });
    let session = if layout == photos_library::catalog::ImportLayout::PerImport
        && total > 0
        && !cancelled()
    {
        let source_name = folder.file_name().unwrap_or_default().to_string_lossy();
        let source_name: String = source_name
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect();
        let name = format!(
            "{} {}",
            chrono::Utc::now().format("%Y-%m-%d %H%M%S"),
            source_name
        );
        Some(create_session_folder(destination, &name)?)
    } else {
        None
    };
    for (index, photo) in found.photos.into_iter().enumerate() {
        if cancelled() {
            report.cancelled = true;
            break;
        }
        let result = (|| {
            let target = if let Some(session) = &session {
                session.clone()
            } else if layout == photos_library::catalog::ImportLayout::YearMonth {
                let date = chrono::DateTime::<chrono::Utc>::from_timestamp(photo.captured_at, 0);
                let subfolder = date.map_or_else(
                    || PathBuf::from("Undated"),
                    |date| PathBuf::from(date.format("%Y/%m").to_string()),
                );
                create_date_folder(destination, &subfolder)?
            } else {
                destination.to_path_buf()
            };
            copy_photo(&photo.path, &target, &cancelled)
        })();
        match result {
            Ok(path) => report.paths.push(path),
            Err(error) => {
                report.skipped += 1;
                if report.errors.len() < 20 {
                    report.errors.push(format!("{}: {error}", photo.name));
                }
            }
        }
        progress(ImportProgress {
            completed: index + 1,
            total,
        });
    }
    Ok(report)
}

pub(super) fn create_date_folder(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut target = root.to_owned();
    for component in relative.components() {
        target.push(component);
        match fs::create_dir(&target) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).context("cannot create import date folder"),
        }
        target = fs::canonicalize(&target)?;
        anyhow::ensure!(
            target.starts_with(root),
            "import folder resolves outside the destination"
        );
        anyhow::ensure!(target.is_dir(), "import date folder is not a directory");
    }
    Ok(target)
}

pub(super) fn create_session_folder(root: &Path, name: &str) -> Result<PathBuf> {
    for collision in 0..10_000 {
        let folder = root.join(if collision == 0 {
            name.to_owned()
        } else {
            format!("{name} ({collision})")
        });
        match fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("too many import folders with the same name")
}

fn copy_photo(path: &Path, destination: &Path, cancelled: impl Fn() -> bool) -> Result<PathBuf> {
    raster::open_media(path)?;
    anyhow::ensure!(!cancelled(), "import cancelled");
    let companion = super::formats::motion_companion(path);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path.extension().unwrap_or_default().to_string_lossy();
    for collision in 0..10_000 {
        anyhow::ensure!(!cancelled(), "import cancelled");
        let name = if collision == 0 {
            path.file_name().unwrap_or_default().to_owned()
        } else {
            format!("{stem} ({collision}).{extension}").into()
        };
        let output = destination.join(name);
        if let Some(companion) = &companion {
            let movie = output.with_extension(companion.extension().unwrap_or_default());
            if output.try_exists()? || movie.try_exists()? {
                continue;
            }
            anyhow::ensure!(!cancelled(), "import cancelled");
            super::video::copy_atomic(companion, &movie, &cancelled)
                .context("cannot copy the Live Photo companion")?;
        }
        match super::video::copy_atomic(path, &output, &cancelled) {
            Ok(()) => return Ok(output),
            Err(error)
                if error
                    .downcast_ref::<enzui_iced_core::user_file::SaveError>()
                    .is_some_and(|e| e.io_error().kind() == std::io::ErrorKind::AlreadyExists) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    anyhow::bail!("too many files with the same name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use photos_library::catalog::ImportLayout;

    #[test]
    fn formats_import_preserves_live_pair_stems_and_never_overwrites_existing_movies() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let source = sandbox.path().join("source");
        let destination = sandbox.path().join("destination");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&destination).expect("destination");
        image::RgbImage::from_pixel(4, 3, image::Rgb([20, 40, 80]))
            .save(source.join("Live.jpg"))
            .expect("photo");
        fs::write(source.join("Live.MOV"), b"source movie").expect("movie");
        fs::write(destination.join("Live.MOV"), b"existing movie").expect("existing");
        let report = from_folder(&source, &destination, || false).expect("import");
        assert_eq!(report.paths, vec![destination.join("Live (1).jpg")]);
        assert_eq!(
            fs::read(destination.join("Live (1).MOV")).expect("companion"),
            b"source movie"
        );
        assert_eq!(
            fs::read(destination.join("Live.MOV")).expect("unchanged existing"),
            b"existing movie"
        );
        let again = from_folder(&source, &destination, || false).expect("repeat import");
        assert_eq!(again.paths, vec![destination.join("Live (2).jpg")]);
        assert_eq!(
            fs::read(destination.join("Live (2).MOV")).expect("second companion"),
            b"source movie"
        );
        assert_eq!(
            fs::read(source.join("Live.MOV")).expect("source unchanged"),
            b"source movie"
        );
    }

    #[test]
    fn preferences_per_import_folders_are_unique_and_date_symlinks_cannot_escape() {
        use photos_library::catalog::ImportLayout;
        let sandbox = tempfile::tempdir().expect("sandbox");
        let source = sandbox.path().join("Camera");
        let destination = sandbox.path().join("Photos");
        fs::create_dir(&source).expect("source");
        RgbaImage::new(4, 3)
            .save(source.join("a.png"))
            .expect("photo");
        let first = from_folder_organized(
            &source,
            &destination,
            ImportLayout::PerImport,
            || false,
            |_| {},
        )
        .expect("import");
        let second = from_folder_organized(
            &source,
            &destination,
            ImportLayout::PerImport,
            || false,
            |_| {},
        )
        .expect("import");
        assert_eq!(first.paths.len(), 1);
        assert_eq!(second.paths.len(), 1);
        assert_ne!(first.paths[0].parent(), second.paths[0].parent());
        assert_eq!(
            fs::read(&first.paths[0]).expect("first"),
            fs::read(&second.paths[0]).expect("second")
        );
        let outside = sandbox.path().join("outside");
        fs::create_dir(&outside).expect("outside");
        let year = chrono::Utc::now().format("%Y").to_string();
        std::os::unix::fs::symlink(&outside, destination.join(year)).expect("symlink");
        let rejected = from_folder_organized(
            &source,
            &destination,
            ImportLayout::YearMonth,
            || false,
            |_| {},
        )
        .expect("report");
        assert!(rejected.paths.is_empty());
        assert_eq!(rejected.skipped, 1);
        assert_eq!(fs::read_dir(outside).expect("outside").count(), 0);
    }

    #[test]
    fn import_collision_copies_without_overwriting_source_or_destination() {
        let sandbox = tempfile::tempdir().expect("sandbox");
        let source = sandbox.path().join("source");
        let destination = sandbox.path().join("Photos");
        fs::create_dir(&source).expect("dir");
        fs::create_dir(&destination).expect("dir");
        RgbaImage::from_pixel(4, 3, Rgba([255, 80, 20, 255]))
            .save(source.join("a.png"))
            .expect("photo");
        fs::write(destination.join("a.png"), b"existing").expect("existing");
        let original = fs::read(source.join("a.png")).expect("read");
        assert!(from_folder(&destination, &destination, || false).is_err());
        let report = from_folder(&source, &destination, || false).expect("import");
        assert_eq!(report.paths.len(), 1);
        assert_eq!(report.paths[0].file_name().expect("name"), "a (1).png");
        assert_eq!(fs::read(&report.paths[0]).expect("copy"), original);
        assert_eq!(fs::read(source.join("a.png")).expect("source"), original);
        assert_eq!(
            fs::read(destination.join("a.png")).expect("existing"),
            b"existing"
        );
        let organized = destination.join("Organized");
        let organized_report = from_paths_organized(
            std::slice::from_ref(&report.paths[0]),
            &organized,
            ImportLayout::Flat,
            || false,
            |_| {},
        )
        .expect("copy a library photo into a folder");
        assert_eq!(organized_report.paths.len(), 1);
        assert_eq!(
            fs::read(&organized_report.paths[0]).expect("organized copy"),
            original
        );
        assert_eq!(
            fs::read(&report.paths[0]).expect("library original"),
            original
        );
        let encoded = from_encoded_image(
            &original,
            "image/png",
            &destination,
            ImportLayout::Flat,
            || false,
            |_| {},
        )
        .expect("encoded GTK image");
        assert_eq!(encoded.paths.len(), 1);
        assert_eq!(
            fs::read(&encoded.paths[0]).expect("encoded image copy"),
            original
        );
        let before_encoded_refusal = fs::read_dir(&destination)
            .expect("import destination")
            .count();
        assert!(
            from_encoded_image(
                &original,
                "image/jpeg",
                &destination,
                ImportLayout::Flat,
                || false,
                |_| {}
            )
            .is_err()
        );
        assert!(
            from_encoded_image(
                b"not an image",
                "image/png",
                &destination,
                ImportLayout::Flat,
                || false,
                |_| {}
            )
            .is_err()
        );
        assert!(
            from_encoded_image(
                &original,
                "image/png",
                &destination,
                ImportLayout::Flat,
                || true,
                |_| {}
            )
            .is_err()
        );
        assert_eq!(
            fs::read_dir(&destination)
                .expect("import destination")
                .count(),
            before_encoded_refusal
        );
        // Native file and folder batches use the same collision-safe copy
        // path and refuse an invalid mixed batch before publishing a subset.
        let dropped = from_paths_organized(
            &[source.join("a.png")],
            &destination,
            ImportLayout::Flat,
            || false,
            |_| {},
        )
        .expect("dropped file");
        assert_eq!(dropped.paths.len(), 1);
        assert_eq!(fs::read(&dropped.paths[0]).expect("dropped copy"), original);
        let repeated = from_paths_organized(
            &[source.clone(), source.join("a.png")],
            &destination,
            ImportLayout::Flat,
            || false,
            |_| {},
        )
        .expect("overlapping drop roots");
        assert_eq!(
            repeated.paths.len(),
            1,
            "overlapping selections import once"
        );
        let before = fs::read_dir(&destination).expect("imports").count();
        assert!(
            from_paths_organized(
                &[source.join("a.png"), destination.join("a.png")],
                &destination,
                ImportLayout::Flat,
                || false,
                |_| {}
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(&destination).expect("imports").count(), before);
        let unsupported = source.join("not-a-photo.txt");
        fs::write(&unsupported, b"plain text").expect("unsupported explicit file");
        assert!(
            from_paths_organized(
                &[source.join("a.png"), unsupported],
                &destination,
                ImportLayout::Flat,
                || false,
                |_| {}
            )
            .is_err()
        );
        assert_eq!(
            fs::read_dir(&destination)
                .expect("unchanged imports")
                .count(),
            before
        );
        assert_eq!(fs::read(source.join("a.png")).expect("source"), original);
    }

    #[test]
    fn import_progress_cancels_after_one_atomic_copy() {
        use std::cell::{Cell, RefCell};
        let sandbox = tempfile::tempdir().expect("sandbox");
        let source = sandbox.path().join("source");
        let destination = sandbox.path().join("Photos");
        fs::create_dir(&source).expect("source");
        for name in ["a.png", "b.png", "c.png"] {
            RgbaImage::new(4, 3).save(source.join(name)).expect("image");
        }
        assert!(
            from_folder(&source, &destination, || true)
                .expect("cancelled")
                .paths
                .is_empty()
        );
        let cancelled = Cell::new(false);
        let updates = RefCell::new(Vec::new());
        let report = from_folder_with_progress(
            &source,
            &destination,
            || cancelled.get(),
            |progress| {
                updates
                    .borrow_mut()
                    .push((progress.completed, progress.total));
                if progress.completed == 1 {
                    cancelled.set(true);
                }
            },
        )
        .expect("import");
        assert_eq!(updates.borrow().as_slice(), [(0, 3), (1, 3)]);
        assert!(report.cancelled);
        assert_eq!(report.paths.len(), 1);
        assert_eq!(fs::read_dir(destination).expect("destination").count(), 1);
        assert_eq!(fs::read_dir(source).expect("source").count(), 3);
    }
}
