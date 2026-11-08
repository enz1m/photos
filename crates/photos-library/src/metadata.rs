use std::io::Cursor;

use chrono::NaiveDateTime;

/// Human-readable metadata and an explicit privacy summary.
#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub fields: Vec<(String, String)>,
    export_fields: Vec<exif::Field>,
    pub captured_at: Option<i64>,
    pub camera: Option<String>,
    pub has_location: bool,
    pub has_exif: bool,
    pub orientation: Option<u32>,
}

/// Extracts EXIF from a bounded prefix; malformed or absent EXIF is an empty result.
#[must_use]
pub fn inspect(bytes: &[u8]) -> Metadata {
    let bounded = &bytes[..bytes.len().min(4 * 1024 * 1024)];
    let Ok(exif) = exif::Reader::new().read_from_container(&mut Cursor::new(bounded)) else {
        return Metadata::default();
    };
    let mut metadata = Metadata {
        has_exif: true,
        ..Default::default()
    };
    for field in exif.fields() {
        if field.ifd_num == exif::In::PRIMARY && field.tag != exif::Tag::MakerNote {
            metadata.export_fields.push(field.clone());
        }
        if metadata.fields.len() < 128 {
            let value = field.display_value().with_unit(&exif).to_string();
            metadata
                .fields
                .push((field.tag.to_string(), value.chars().take(500).collect()));
        }
        if matches!(
            field.tag,
            exif::Tag::GPSLatitude | exif::Tag::GPSLongitude | exif::Tag::GPSAltitude
        ) {
            metadata.has_location = true;
        }
        if field.tag == exif::Tag::Model {
            metadata.camera = ascii_value(&field.value);
        }
        if field.tag == exif::Tag::Orientation {
            metadata.orientation = field.value.get_uint(0);
        }
        if field.tag == exif::Tag::DateTimeOriginal
            || (field.tag == exif::Tag::DateTime && metadata.captured_at.is_none())
        {
            metadata.captured_at = ascii_value(&field.value)
                .and_then(|date| NaiveDateTime::parse_from_str(&date, "%Y:%m:%d %H:%M:%S").ok())
                .map(|date| date.and_utc().timestamp());
        }
    }
    metadata
}

/// Encodes retained EXIF with baked orientation and updated output dimensions.
///
/// # Errors
/// Returns an EXIF serialization error for unsupported fields.
pub fn encode_for_export(
    metadata: &Metadata,
    dimensions: (u32, u32),
) -> Result<Option<Vec<u8>>, exif::Error> {
    if metadata.export_fields.is_empty() {
        return Ok(None);
    }
    let mut fields = metadata.export_fields.clone();
    fields.retain(|field| {
        !matches!(
            field.tag,
            exif::Tag::Orientation
                | exif::Tag::ImageWidth
                | exif::Tag::ImageLength
                | exif::Tag::PixelXDimension
                | exif::Tag::PixelYDimension
        )
    });
    for (tag, value) in [
        (exif::Tag::Orientation, 1),
        (exif::Tag::PixelXDimension, dimensions.0),
        (exif::Tag::PixelYDimension, dimensions.1),
    ] {
        fields.push(exif::Field {
            tag,
            ifd_num: exif::In::PRIMARY,
            value: if tag == exif::Tag::Orientation {
                exif::Value::Short(vec![1])
            } else {
                exif::Value::Long(vec![value])
            },
        });
    }
    let mut writer = exif::experimental::Writer::new();
    for field in &fields {
        writer.push_field(field);
    }
    let mut bytes = Cursor::new(Vec::new());
    writer.write(&mut bytes, true)?;
    Ok(Some(bytes.into_inner()))
}

fn ascii_value(value: &exif::Value) -> Option<String> {
    if let exif::Value::Ascii(values) = value {
        values.first().map(|bytes| {
            String::from_utf8_lossy(bytes)
                .trim_end_matches('\0')
                .trim()
                .chars()
                .take(500)
                .collect()
        })
    } else {
        None
    }
}
