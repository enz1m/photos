use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader, Limits, RgbaImage};

/// Encoded input cap shared by scan and edit admission.
pub const MAX_ENCODED_BYTES: u64 = 128 * 1024 * 1024;
/// Full-resolution editing limit, screened before allocating decoded pixels.
pub const MAX_PIXELS: u64 = 64_000_000;

#[derive(Debug, thiserror::Error)]
pub enum RasterError {
    #[error("cannot read photo: {0}")]
    Io(#[from] std::io::Error),
    #[error("photo exceeds the encoded or 64 megapixel size limit")]
    TooLarge,
    #[error("this image format is not supported for editing")]
    Unsupported,
    #[error("invalid image: {0}")]
    Image(#[from] image::ImageError),
    #[error("operation cancelled")]
    Cancelled,
    #[error("photo is not a regular file")]
    NotRegular,
}

/// Reads a regular file with a cap even if it grows after the initial stat.
///
/// # Errors
/// Refuses nonregular files, oversized input, and I/O failures.
pub fn read_bounded(path: &Path) -> Result<Vec<u8>, RasterError> {
    let file = open_regular(path)?;
    let meta = file.metadata()?;
    if meta.len() > MAX_ENCODED_BYTES {
        return Err(RasterError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.take(MAX_ENCODED_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ENCODED_BYTES {
        return Err(RasterError::TooLarge);
    }
    Ok(bytes)
}

pub(crate) fn read_prefix(path: &Path, limit: u64) -> Result<Vec<u8>, RasterError> {
    let file = open_regular(path)?;
    if file.metadata()?.len() > MAX_ENCODED_BYTES {
        return Err(RasterError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Opens a regular media file without following a final symlink or buffering its contents.
///
/// # Errors
/// Rejects nonregular files, files larger than 64 GiB, and I/O failures.
pub fn open_media(path: &Path) -> Result<File, RasterError> {
    let file = open_regular(path)?;
    if file.metadata()?.len() > MAX_MEDIA_BYTES {
        return Err(RasterError::TooLarge);
    }
    Ok(file)
}

/// Maximum size of a streamed video or atomic media copy.
pub const MAX_MEDIA_BYTES: u64 = 64 * 1024 * 1024 * 1024;

fn open_regular(path: &Path) -> Result<File, RasterError> {
    #[cfg(unix)]
    let file = File::from(
        rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(std::io::Error::from)?,
    );
    #[cfg(not(unix))]
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(RasterError::NotRegular);
    }
    Ok(file)
}

/// Decodes an admitted raster with orientation and supported RGB-to-sRGB conversion.
///
/// # Errors
/// Rejects unknown codecs, invalid content, oversized images, and cancellation.
pub fn decode(bytes: &[u8], cancelled: impl Fn() -> bool) -> Result<RgbaImage, RasterError> {
    if cancelled() {
        return Err(RasterError::Cancelled);
    }
    if bytes.len() as u64 > MAX_ENCODED_BYTES {
        return Err(RasterError::TooLarge);
    }
    let format = image::guess_format(bytes)?;
    let mut reader = ImageReader::with_format(BufReader::new(Cursor::new(bytes)), format);
    reader.no_limits();
    let mut decoder = reader.into_decoder()?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || w > 32_768 || h > 32_768 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(RasterError::TooLarge);
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(32_768);
    limits.max_image_height = Some(32_768);
    limits.max_alloc = Some(MAX_PIXELS * 4);
    decoder.set_limits(limits)?;
    let profile = decoder
        .icc_profile()
        .ok()
        .flatten()
        .filter(|profile| profile.len() <= 4 * 1024 * 1024);
    let orientation = decoder.orientation()?;
    if cancelled() {
        return Err(RasterError::Cancelled);
    }
    let mut raster = DynamicImage::from_decoder(decoder)?;
    raster.apply_orientation(orientation);
    if cancelled() {
        return Err(RasterError::Cancelled);
    }
    let mut pixels = raster.into_rgba8();
    if let Some(profile) = profile {
        normalize_rgb(&mut pixels, &profile, &cancelled)?;
    }
    Ok(pixels)
}

/// Reduces an image to fit within a longest edge without enlarging it.
/// A zero limit is treated as one pixel; the caller runs this pixel work on a worker.
pub fn display_proxy(pixels: &RgbaImage, limit: u32) -> RgbaImage {
    let limit = limit.max(1);
    let ratio = (f64::from(limit) / f64::from(pixels.width()))
        .min(f64::from(limit) / f64::from(pixels.height()))
        .min(1.0);
    if ratio >= 1.0 {
        return pixels.clone();
    }
    image::imageops::resize(
        pixels,
        (f64::from(pixels.width()) * ratio).round().max(1.0) as u32,
        (f64::from(pixels.height()) * ratio).round().max(1.0) as u32,
        image::imageops::FilterType::Triangle,
    )
}

fn normalize_rgb(
    pixels: &mut RgbaImage,
    profile: &[u8],
    cancelled: &impl Fn() -> bool,
) -> Result<(), RasterError> {
    use moxcms::{ColorProfile, DataColorSpace, Layout, TransformOptions};
    let Ok(source) = ColorProfile::new_from_slice(profile) else {
        return Ok(());
    };
    if source.color_space != DataColorSpace::Rgb {
        return Ok(());
    }
    let Ok(transform) = source.create_transform_8bit(
        Layout::Rgba,
        &ColorProfile::new_srgb(),
        Layout::Rgba,
        TransformOptions::default(),
    ) else {
        return Ok(());
    };
    let width = pixels.width() as usize * 4;
    let mut row = vec![0; width];
    for source in pixels.as_mut().chunks_exact_mut(width) {
        if cancelled() {
            return Err(RasterError::Cancelled);
        }
        transform
            .transform(source, &mut row)
            .map_err(|_| RasterError::Unsupported)?;
        source.copy_from_slice(&row);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_sparse_oversized_input_is_refused_without_reading() {
        let dir = tempfile::tempdir().expect("sandbox");
        let path = dir.path().join("huge.jpg");
        File::create(&path)
            .expect("file")
            .set_len(MAX_ENCODED_BYTES + 1)
            .expect("length");
        assert!(matches!(read_bounded(&path), Err(RasterError::TooLarge)));
    }

    #[cfg(unix)]
    #[test]
    fn raster_refuses_fifo_and_final_symlink() {
        let dir = tempfile::tempdir().expect("sandbox");
        let fifo = dir.path().join("fifo.jpg");
        rustix::fs::mknodat(
            rustix::fs::CWD,
            &fifo,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
            0,
        )
        .expect("fifo");
        assert!(matches!(read_bounded(&fifo), Err(RasterError::NotRegular)));
        let real = dir.path().join("real.png");
        RgbaImage::new(2, 2).save(&real).expect("image");
        let linked = dir.path().join("linked.png");
        std::os::unix::fs::symlink(&real, &linked).expect("symlink");
        assert!(read_bounded(&linked).is_err());
        assert!(read_bounded(&real).is_ok());
    }
}
