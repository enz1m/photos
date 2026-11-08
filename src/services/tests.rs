use super::*;

fn jpeg_with_private_metadata() -> Vec<u8> {
    let pixels =
        image::RgbImage::from_fn(12, 8, |x, y| image::Rgb([x as u8 * 15, y as u8 * 20, 60]));
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&pixels)
        .expect("jpeg");
    fn entry(bytes: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let fields = 134u16;
    let data = 8 + 2 + u32::from(fields) * 12 + 4;
    let gps = data + 12 + 20;
    let mut tiff = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
    tiff.extend_from_slice(&fields.to_le_bytes());
    for tag in 0xc000..0xc082 {
        entry(&mut tiff, tag, 3, 1, 1);
    }
    entry(&mut tiff, 0x0112, 3, 1, 6);
    entry(&mut tiff, 0x0110, 2, 12, data);
    entry(&mut tiff, 0x0132, 2, 20, data + 12);
    entry(&mut tiff, 0x8825, 4, 1, gps);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(b"Test Camera\0");
    tiff.extend_from_slice(b"2025:06:15 12:30:00\0");
    tiff.extend_from_slice(&2u16.to_le_bytes());
    entry(&mut tiff, 1, 2, 2, u32::from(b'N'));
    entry(&mut tiff, 2, 5, 3, gps + 30);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    for numerator in [53u32, 20, 15] {
        tiff.extend_from_slice(&numerator.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
    }
    let mut output = jpeg[..2].to_vec();
    output.extend_from_slice(b"\xff\xe1");
    output.extend_from_slice(
        &u16::try_from(tiff.len() + 8)
            .expect("EXIF size")
            .to_be_bytes(),
    );
    output.extend_from_slice(b"Exif\0\0");
    output.extend_from_slice(&tiff);
    output.extend_from_slice(&jpeg[2..]);
    output
}

#[test]
fn heic_real_content_loads_edits_and_exports_even_with_wrong_extension() {
    let sandbox = tempfile::tempdir().expect("sandbox");
    let path = sandbox.path().join("phone.jpg");
    let original = include_bytes!("../../tests/fixtures/synthetic.heic");
    std::fs::write(&path, original).expect("HEIC fixture");
    let loaded = load_photo(&path, || false).expect("HEIC decoder");
    assert_eq!(loaded.pixels.dimensions(), (64, 64));
    let output = sandbox.path().join("edited.png");
    let recipe = EditRecipe {
        resize: Some((32, 16)),
        brightness: 30.0,
        ..Default::default()
    };
    export(&loaded.pixels, &recipe, &output, || false).expect("export");
    let edited = image::open(&output).expect("PNG").into_rgba8();
    assert_eq!(edited.dimensions(), (32, 16));
    assert!(edited.get_pixel(16, 8)[0] > loaded.pixels.get_pixel(32, 32)[0]);
    let cancelled = sandbox.path().join("cancelled.png");
    assert!(export(&loaded.pixels, &recipe, &cancelled, || true).is_err());
    assert!(!cancelled.exists());
    assert_eq!(std::fs::read(path).expect("original"), original);
}

#[test]
fn thumbnail_oversized_decoder_handle_is_bounded_before_cache_admission() {
    let sandbox = tempfile::tempdir().expect("sandbox");
    let path = sandbox.path().join("photo.heic");
    std::fs::write(
        &path,
        include_bytes!("../../tests/fixtures/solid-960x640.heic"),
    )
    .expect("real HEVC image");
    let report =
        photos_library::scan::scan(&[sandbox.path().to_owned()], Default::default(), || false);
    let photo = &report.photos[0];
    let source_size = load_photo(&path, || false)
        .expect("full resolution")
        .pixels
        .dimensions();
    assert!(source_size.0.max(source_size.1) > 512);
    let thumbnail = thumbnail(photo, None, || false).expect("thumbnail");
    let pixels = pixels_from_handle(thumbnail).expect("pixels");
    assert!(pixels.width() > 0 && pixels.height() > 0);
    assert!(pixels.width().max(pixels.height()) <= 512);
}

#[test]
fn export_metadata_privacy_handles_fields_beyond_the_display_limit() {
    let sandbox = tempfile::tempdir().expect("sandbox");
    let path = sandbox.path().join("original.jpg");
    let original = jpeg_with_private_metadata();
    std::fs::write(&path, &original).expect("source");
    let source = load_photo(&path, || false).expect("load");
    assert!(
        !source
            .metadata
            .fields
            .iter()
            .any(|(tag, _)| tag == "GPSLatitude")
    );
    assert!(source.metadata.has_location);
    assert_eq!(source.metadata.orientation, Some(6));
    assert_eq!(source.metadata.camera.as_deref(), Some("Test Camera"));
    for format in [ExportFormat::Png, ExportFormat::Jpeg, ExportFormat::Webp] {
        for keep in [false, true] {
            let output = sandbox
                .path()
                .join(format!("copy-{keep}.{}", format.extension()));
            export_with_metadata(
                &source,
                &EditRecipe::default(),
                &output,
                ExportOptions {
                    format,
                    scale_percent: 50,
                    keep_metadata: keep,
                    ..Default::default()
                },
                || false,
            )
            .expect("export");
            let reopened = load_photo(&output, || false).expect("reopen");
            assert_eq!(reopened.pixels.dimensions(), (4, 6));
            assert_eq!(reopened.metadata.has_location, keep);
            assert_eq!(reopened.metadata.has_exif, keep);
            if keep {
                assert_eq!(reopened.metadata.orientation, Some(1));
                assert_eq!(reopened.metadata.camera.as_deref(), Some("Test Camera"));
            }
        }
    }
    assert_eq!(std::fs::read(path).expect("unchanged"), original);
}

#[test]
fn export_normalized_wide_gamut_pixels_retain_srgb_instead_of_the_source_profile() {
    use image::{ExtendedColorType, ImageEncoder};
    let sandbox = tempfile::tempdir().expect("sandbox");
    let path = sandbox.path().join("original.png");
    let profile = moxcms::ColorProfile::new_display_p3()
        .encode()
        .expect("P3 profile");
    let mut original = Vec::new();
    let mut encoder = image::codecs::png::PngEncoder::new(&mut original);
    encoder.set_icc_profile(profile.clone()).expect("profile");
    encoder
        .write_image(&[200, 100, 50, 127], 1, 1, ExtendedColorType::Rgba8)
        .expect("PNG");
    std::fs::write(&path, &original).expect("source");
    let source = load_photo(&path, || false).expect("load");
    let fallback = raster::decode(&original, || false).expect("bounded fallback decode");
    assert_eq!(fallback, source.pixels);
    let pixel = source.pixels.get_pixel(0, 0);
    assert!(pixel[0] > 200 && pixel[2] < 50);
    assert_eq!(pixel[3], 127);
    assert_ne!(source.icc_profile.as_ref(), Some(&profile));
    let report = scan::scan(std::slice::from_ref(&path), Default::default(), || false);
    let photo = report.photos.first().expect("discovered P3 image");
    for _ in 0..2 {
        let cache = thumbnail_cache::Cache::new(sandbox.path().join("cache"));
        let preview = cache
            .thumbnail(photo, None, 256, 0, || false)
            .expect("thumbnail");
        assert_eq!(pixels_from_handle(preview).expect("pixels"), source.pixels);
    }
    let output = sandbox.path().join("copy.png");
    export_with_metadata(
        &source,
        &EditRecipe::default(),
        &output,
        ExportOptions {
            keep_metadata: true,
            ..Default::default()
        },
        || false,
    )
    .expect("export");
    let reopened = load_photo(&output, || false).expect("reopen");
    assert_eq!(reopened.pixels, source.pixels);
    assert!(reopened.icc_profile.is_some());
    assert_eq!(std::fs::read(path).expect("source bytes"), original);
}
