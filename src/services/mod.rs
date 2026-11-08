use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use iced::widget::image::Handle;
use image::{ImageDecoder, ImageEncoder, RgbaImage};
use photos_library::catalog::{Catalog, Photo, PhotoId};
use photos_library::edit::EditRecipe;
use photos_library::{metadata, raster, scan};

pub(crate) mod formats;
pub(crate) mod import;
pub(crate) mod persistence;
pub(crate) mod remote;
#[cfg(test)]
mod tests;
pub(crate) mod thumbnail_cache;
pub(crate) mod video;

pub(crate) fn clipboard_error(error: iced::clipboard::Error) -> String {
    match error {
        iced::clipboard::Error::ClipboardOccupied => {
            "The desktop clipboard is busy. Try copying again.".into()
        }
        iced::clipboard::Error::ConversionFailure => {
            "This image could not be converted for the clipboard.".into()
        }
        _ => "The desktop clipboard could not accept the image. Save a copy instead.".into(),
    }
}

#[derive(Debug)]
pub(crate) struct LoadedPhoto {
    pub motion: Option<formats::Motion>,
    pub animate: bool,
    pub stream: Option<PathBuf>,
    pub video_info: Option<video::VideoInfo>,
    pub motion_warning: Option<String>,
    pub pixels: RgbaImage,
    pub proxy: RgbaImage,
    pub original_preview: Handle,
    pub metadata: metadata::Metadata,
    pub icc_profile: Option<Vec<u8>>,
}

pub(crate) fn thumbnail(
    photo: &Photo,
    recipe: Option<&EditRecipe>,
    cancelled: impl Fn() -> bool + Sync,
) -> Result<Handle> {
    thumbnail_sized(photo, recipe, 512, cancelled)
}

pub(crate) fn thumbnail_sized(
    photo: &Photo,
    recipe: Option<&EditRecipe>,
    limit: u32,
    cancelled: impl Fn() -> bool + Sync,
) -> Result<Handle> {
    anyhow::ensure!(!cancelled(), "thumbnail cancelled");
    if scan::is_video_path(&photo.path) {
        return Ok(handle(video::frame(
            &photo.path,
            0.0,
            Some(limit),
            &cancelled,
        )?));
    }
    let bytes = raster::read_bounded(&photo.path)?;
    let pixels = formats::decode(&photo.path, &bytes, Some(limit), &cancelled)?;
    if let Some(recipe) = recipe {
        return preview_bounded(&reduced(&pixels, limit), recipe, limit);
    }

    anyhow::ensure!(!cancelled(), "thumbnail cancelled");
    Ok(handle(reduced(&pixels, limit)))
}

pub(crate) fn load_photo(path: &Path, cancelled: impl Fn() -> bool + Sync) -> Result<LoadedPhoto> {
    anyhow::ensure!(!cancelled(), "photo load cancelled");
    if scan::is_video_path(path) {
        let video_info = video::probe(path, &cancelled)?;
        let pixels = video::frame(path, 0.0, Some(1600), &cancelled)?;
        let proxy = pixels.clone();
        return Ok(LoadedPhoto {
            animate: false,
            stream: Some(path.to_owned()),
            video_info: Some(video_info),
            motion: None,
            motion_warning: None,
            original_preview: handle(proxy.clone()),
            pixels,
            proxy,
            metadata: Default::default(),
            icc_profile: None,
        });
    }
    let load_started = std::time::Instant::now();
    let bytes = raster::read_bounded(path)?;
    let metadata = metadata::inspect(&bytes);
    let icc_profile = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_decoder().ok())
        .and_then(|mut decoder| decoder.icc_profile().ok().flatten())
        .filter(|profile| profile.len() <= 4 * 1024 * 1024)
        .and_then(|profile| moxcms::ColorProfile::new_from_slice(&profile).ok())
        .filter(|profile| profile.color_space == moxcms::DataColorSpace::Rgb)
        .and_then(|_| moxcms::ColorProfile::new_srgb().encode().ok());
    tracing::debug!(
        elapsed_ms = load_started.elapsed().as_millis(),
        "photo headers loaded"
    );
    let pixels = formats::decode(path, &bytes, None, &cancelled)?;
    tracing::debug!(
        elapsed_ms = load_started.elapsed().as_millis(),
        "photo pixels decoded"
    );
    let stream = formats::motion_companion(path);
    let motion = None;
    let motion_warning = None;
    anyhow::ensure!(!cancelled(), "photo load cancelled");
    let proxy = reduced(&pixels, 1600);
    tracing::debug!(
        elapsed_ms = load_started.elapsed().as_millis(),
        "photo display proxy prepared"
    );
    let original_preview = handle(proxy.clone());
    Ok(LoadedPhoto {
        animate: formats::may_animate(path, &bytes),
        stream,
        video_info: None,
        motion,
        motion_warning,
        pixels,
        proxy,
        original_preview,
        metadata,
        icc_profile,
    })
}

fn pixels_from_handle(handle: Handle) -> Result<RgbaImage> {
    match handle {
        Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } => RgbaImage::from_raw(width, height, pixels.as_ref().to_vec())
            .context("decoder returned invalid pixels"),
        _ => anyhow::bail!("decoder returned an unsupported image handle"),
    }
}

fn reduced(pixels: &RgbaImage, limit: u32) -> RgbaImage {
    raster::display_proxy(pixels, limit)
}

pub(crate) fn handle(pixels: RgbaImage) -> Handle {
    Handle::from_rgba(pixels.width(), pixels.height(), pixels.into_raw())
}

pub(crate) fn preview(source: &RgbaImage, recipe: &EditRecipe) -> Result<Handle> {
    preview_bounded(source, recipe, 1600)
}

fn preview_bounded(source: &RgbaImage, recipe: &EditRecipe, limit: u32) -> Result<Handle> {
    let mut recipe = recipe.clone();
    if let Some((w, h)) = recipe.resize {
        let ratio = (f64::from(limit) / f64::from(w))
            .min(f64::from(limit) / f64::from(h))
            .min(1.0);
        recipe.resize = Some((
            (f64::from(w) * ratio).round().max(1.0) as u32,
            (f64::from(h) * ratio).round().max(1.0) as u32,
        ));
    }
    let pixels = recipe.apply(source)?;
    let pixels = reduced(&pixels, limit);
    Ok(handle(pixels))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ExportFormat {
    #[default]
    Png,
    Jpeg,
    Webp,
}

impl ExportFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExportOptions {
    pub format: ExportFormat,
    pub quality: u8,
    pub scale_percent: u8,
    pub keep_metadata: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Png,
            quality: 92,
            scale_percent: 100,
            keep_metadata: false,
        }
    }
}

pub(crate) fn scaled_dimensions(dimensions: (u32, u32), percent: u8) -> (u32, u32) {
    let factor = f64::from(percent.clamp(1, 100)) / 100.0;
    (
        (f64::from(dimensions.0) * factor).round().max(1.0) as u32,
        (f64::from(dimensions.1) * factor).round().max(1.0) as u32,
    )
}

pub(crate) fn export_with_metadata(
    source: &LoadedPhoto,
    recipe: &EditRecipe,
    path: &Path,
    options: ExportOptions,
    cancelled: impl Fn() -> bool,
) -> Result<PathBuf> {
    export_encoded(
        &source.pixels,
        recipe,
        path,
        options,
        options.keep_metadata.then_some(source),
        cancelled,
    )
}

#[cfg(test)]
pub(crate) fn export(
    source: &RgbaImage,
    recipe: &EditRecipe,
    path: &Path,
    cancelled: impl Fn() -> bool,
) -> Result<PathBuf> {
    export_with_options(source, recipe, path, ExportOptions::default(), cancelled)
}

#[cfg(test)]
pub(crate) fn export_with_options(
    source: &RgbaImage,
    recipe: &EditRecipe,
    path: &Path,
    options: ExportOptions,
    cancelled: impl Fn() -> bool,
) -> Result<PathBuf> {
    export_encoded(source, recipe, path, options, None, cancelled)
}

fn configure_metadata<E: ImageEncoder>(
    mut encoder: E,
    source: Option<&LoadedPhoto>,
    dimensions: (u32, u32),
) -> Result<E> {
    if let Some(source) = source {
        if let Some(exif) = metadata::encode_for_export(&source.metadata, dimensions)? {
            encoder.set_exif_metadata(exif)?;
        }
        if let Some(profile) = &source.icc_profile {
            encoder.set_icc_profile(profile.clone())?;
        }
    }
    Ok(encoder)
}

fn export_encoded(
    source: &RgbaImage,
    recipe: &EditRecipe,
    path: &Path,
    options: ExportOptions,
    metadata: Option<&LoadedPhoto>,
    cancelled: impl Fn() -> bool,
) -> Result<PathBuf> {
    anyhow::ensure!(!cancelled(), "export cancelled");
    let mut pixels = recipe.apply(source)?;
    if options.scale_percent != 100 {
        let (width, height) = scaled_dimensions(pixels.dimensions(), options.scale_percent);
        pixels = image::imageops::resize(
            &pixels,
            width,
            height,
            image::imageops::FilterType::Lanczos3,
        );
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut bytes = Vec::new();
    match extension.as_str() {
        "png" => {
            configure_metadata(
                image::codecs::png::PngEncoder::new(&mut bytes),
                metadata,
                pixels.dimensions(),
            )?
            .write_image(
                &pixels,
                pixels.width(),
                pixels.height(),
                image::ExtendedColorType::Rgba8,
            )?;
        }
        "jpg" | "jpeg" => {
            let rgb = image::RgbImage::from_fn(pixels.width(), pixels.height(), |x, y| {
                let p = pixels.get_pixel(x, y);
                let alpha = u32::from(p[3]);
                image::Rgb(std::array::from_fn(|i| {
                    ((u32::from(p[i]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
                }))
            });
            configure_metadata(
                image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut bytes,
                    options.quality.clamp(1, 100),
                ),
                metadata,
                pixels.dimensions(),
            )?
            .encode_image(&rgb)?;
        }
        "webp" => {
            configure_metadata(
                image::codecs::webp::WebPEncoder::new_lossless(&mut bytes),
                metadata,
                pixels.dimensions(),
            )?
            .write_image(
                &pixels,
                pixels.width(),
                pixels.height(),
                image::ExtendedColorType::Rgba8,
            )?;
        }
        _ => anyhow::bail!("choose a .png, .jpg, or .webp destination"),
    }
    anyhow::ensure!(!cancelled(), "export cancelled");
    enzui_iced_core::user_file::save(
        path,
        enzui_iced_core::user_file::SavePolicy::CreateNew,
        &bytes,
    )
    .context("could not create export; existing files are never overwritten")?;
    Ok(path.to_owned())
}

pub(crate) fn roots(home: &Path, catalog: &Catalog) -> Vec<PathBuf> {
    std::iter::once(home.to_owned())
        .chain(catalog.sources.iter().cloned())
        .collect()
}

pub(crate) fn discovery(
    home: &Path,
    catalog: &Catalog,
    cancelled: impl Fn() -> bool,
) -> scan::ScanReport {
    if let Err(error) = std::fs::create_dir_all(home) {
        return scan::ScanReport {
            warnings: vec![format!("Cannot create Photos folder: {error}")],
            ..Default::default()
        };
    }
    scan::scan(
        &roots(home, catalog),
        scan::ScanLimits::default(),
        cancelled,
    )
}

pub(crate) const THUMBNAIL_LIMIT: u32 = 512;

pub(crate) fn thumbnail_key(photo: &Photo, recipe: Option<&EditRecipe>, limit: u32) -> String {
    format!("{}:p{limit}", photo_key(photo, recipe))
}

pub(crate) fn photo_key(photo: &Photo, recipe: Option<&EditRecipe>) -> String {
    use std::hash::{Hash, Hasher};
    let mut edit = std::collections::hash_map::DefaultHasher::new();
    if let Some(recipe) = recipe {
        recipe.quarter_turns.hash(&mut edit);
        recipe.flip_horizontal.hash(&mut edit);
        recipe.flip_vertical.hash(&mut edit);
        recipe.monochrome.hash(&mut edit);
        recipe.resize.hash(&mut edit);
        recipe
            .crop
            .map(|crop| {
                [
                    crop.x.to_bits(),
                    crop.y.to_bits(),
                    crop.width.to_bits(),
                    crop.height.to_bits(),
                ]
            })
            .hash(&mut edit);
        [
            recipe.exposure,
            recipe.brightness,
            recipe.contrast,
            recipe.saturation,
            recipe.temperature,
            recipe.tint,
            recipe.highlights,
            recipe.shadows,
            recipe.sharpness,
        ]
        .map(f32::to_bits)
        .hash(&mut edit);
    }
    format!(
        "srgb-v2:{}:{}:{}:{}",
        photo.id.0,
        photo.modified_ns,
        photo.bytes,
        edit.finish()
    )
}

pub(crate) fn imported_ids(paths: &[PathBuf]) -> Vec<PhotoId> {
    paths.iter().map(|path| PhotoId::for_path(path)).collect()
}

pub(crate) fn validate_source(path: &Path) -> Result<PathBuf> {
    anyhow::ensure!(path.is_absolute(), "choose an absolute folder path");
    let path = std::fs::canonicalize(path).context("cannot access source folder")?;
    anyhow::ensure!(path.is_dir(), "choose an existing folder");
    std::fs::read_dir(&path).context("cannot read source folder")?;
    Ok(path)
}
