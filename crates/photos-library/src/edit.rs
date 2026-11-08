use image::{RgbaImage, imageops};
use serde::{Deserialize, Serialize};

/// A crop in normalized, orientation-adjusted image coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Crop {
    /// Requires a finite, nonempty rectangle wholly inside the image.
    #[must_use]
    pub fn is_valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.x >= 0.0
            && self.y >= 0.0
            && self.width > 0.0
            && self.height > 0.0
            && self.x + self.width <= 1.000_001
            && self.y + self.height <= 1.000_001
    }
}

/// Reversible operations applied to original pixels; the original file is never modified.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditRecipe {
    pub quarter_turns: u8,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub crop: Option<Crop>,
    pub resize: Option<(u32, u32)>,
    pub exposure: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub temperature: f32,
    pub tint: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub sharpness: f32,
    pub monochrome: bool,
}

impl Default for EditRecipe {
    fn default() -> Self {
        Self {
            quarter_turns: 0,
            flip_horizontal: false,
            flip_vertical: false,
            crop: None,
            resize: None,
            exposure: 0.0,
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            temperature: 0.0,
            tint: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            sharpness: 0.0,
            monochrome: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("edit contains a nonfinite or out-of-range adjustment")]
    InvalidAdjustment,
    #[error("crop must be nonempty and inside the image")]
    InvalidCrop,
    #[error("resize exceeds the 64 megapixel editing limit")]
    InvalidResize,
    #[error("image has no pixels")]
    EmptyImage,
}

impl EditRecipe {
    /// Validates every user-controlled operation before pixel work.
    ///
    /// # Errors
    /// Rejects invalid crop, resize, nonfinite values, and adjustments outside their domains.
    pub fn validate(&self) -> Result<(), EditError> {
        if self.crop.is_some_and(|crop| !crop.is_valid()) {
            return Err(EditError::InvalidCrop);
        }
        if self.resize.is_some_and(|(w, h)| {
            w == 0 || h == 0 || w > 32_768 || h > 32_768 || u64::from(w) * u64::from(h) > 64_000_000
        }) {
            return Err(EditError::InvalidResize);
        }
        if !self.exposure.is_finite()
            || self.exposure.abs() > 3.0
            || [
                self.brightness,
                self.contrast,
                self.saturation,
                self.temperature,
                self.tint,
                self.highlights,
                self.shadows,
            ]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 100.0)
            || !self.sharpness.is_finite()
            || !(0.0..=100.0).contains(&self.sharpness)
        {
            return Err(EditError::InvalidAdjustment);
        }
        Ok(())
    }

    /// Rotates clockwise without accumulating unbounded turns.
    pub fn rotate_clockwise(&mut self) {
        self.quarter_turns = (self.quarter_turns % 4 + 1) % 4;
        if let Some((width, height)) = self.resize {
            self.resize = Some((height, width));
        }
    }

    /// Output dimensions after a valid crop, rotation and optional resize.
    #[must_use]
    pub fn output_dimensions(&self, source: (u32, u32)) -> (u32, u32) {
        if let Some(resize) = self.resize {
            return resize;
        }
        let (mut width, mut height) = source;
        if let Some(crop) = self.crop.filter(|crop| crop.is_valid()) {
            let x = ((crop.x * width as f32).floor() as u32).min(width.saturating_sub(1));
            let y = ((crop.y * height as f32).floor() as u32).min(height.saturating_sub(1));
            width =
                ((crop.width * width as f32).round().max(1.0) as u32).min(width.saturating_sub(x));
            height = ((crop.height * height as f32).round().max(1.0) as u32)
                .min(height.saturating_sub(y));
        }
        if self.quarter_turns % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// Applies crop, rotation, flips, resize, color, and sharpening, preserving alpha.
    ///
    /// # Errors
    /// Returns the validation error or rejects an empty source image.
    pub fn apply(&self, source: &RgbaImage) -> Result<RgbaImage, EditError> {
        self.validate()?;
        if source.width() == 0 || source.height() == 0 {
            return Err(EditError::EmptyImage);
        }
        let mut output = if let Some(crop) = self.crop {
            let x = (crop.x * source.width() as f32).floor() as u32;
            let y = (crop.y * source.height() as f32).floor() as u32;
            let w = (crop.width * source.width() as f32).round().max(1.0) as u32;
            let h = (crop.height * source.height() as f32).round().max(1.0) as u32;
            let x = x.min(source.width() - 1);
            let y = y.min(source.height() - 1);
            imageops::crop_imm(
                source,
                x,
                y,
                w.min(source.width() - x),
                h.min(source.height() - y),
            )
            .to_image()
        } else {
            source.clone()
        };
        output = match self.quarter_turns % 4 {
            1 => imageops::rotate90(&output),
            2 => imageops::rotate180(&output),
            3 => imageops::rotate270(&output),
            _ => output,
        };
        if self.flip_horizontal {
            imageops::flip_horizontal_in_place(&mut output);
        }
        if self.flip_vertical {
            imageops::flip_vertical_in_place(&mut output);
        }
        if let Some((w, h)) = self.resize {
            output = imageops::resize(&output, w, h, imageops::FilterType::Lanczos3);
        }
        self.apply_color(&mut output);
        if self.sharpness > 0.0 {
            let alpha: Vec<u8> = output.pixels().map(|p| p[3]).collect();
            output = imageops::unsharpen(&output, 0.6 + self.sharpness / 100.0, 3);
            for (pixel, alpha) in output.pixels_mut().zip(alpha) {
                pixel[3] = alpha;
            }
        }
        Ok(output)
    }

    fn apply_color(&self, output: &mut RgbaImage) {
        if self.exposure == 0.0
            && self.brightness == 0.0
            && self.contrast == 0.0
            && self.saturation == 0.0
            && self.temperature == 0.0
            && self.tint == 0.0
            && self.highlights == 0.0
            && self.shadows == 0.0
            && !self.monochrome
        {
            return;
        }
        let exposure = 2.0_f32.powf(self.exposure);
        let contrast = (1.0 + self.contrast / 100.0).powi(2);
        let saturation = if self.monochrome {
            0.0
        } else {
            1.0 + self.saturation / 100.0
        };
        for pixel in output.pixels_mut() {
            let mut rgb = [
                f32::from(pixel[0]) / 255.0,
                f32::from(pixel[1]) / 255.0,
                f32::from(pixel[2]) / 255.0,
            ];
            let luma = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
            let tonal = self.shadows / 250.0 * (1.0 - luma).powi(2)
                + self.highlights / 250.0 * luma.powi(2);
            let white_balance = [
                self.temperature / 500.0 + self.tint / 1000.0,
                -self.tint / 500.0,
                -self.temperature / 500.0 + self.tint / 1000.0,
            ];
            for (channel, balance) in rgb.iter_mut().zip(white_balance) {
                *channel = (luma + (*channel - luma) * saturation) * exposure;
                *channel =
                    ((*channel - 0.5) * contrast + 0.5 + self.brightness / 255.0 + balance + tonal)
                        .clamp(0.0, 1.0);
            }
            for (index, channel) in rgb.into_iter().enumerate() {
                pixel[index] = (channel * 255.0).round() as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn edit_crop_selects_real_source_pixels_before_rotation() {
        let source = RgbaImage::from_fn(4, 4, |x, y| Rgba([x as u8, y as u8, 0, 255]));
        let recipe = EditRecipe {
            crop: Some(Crop {
                x: 0.5,
                y: 0.25,
                width: 0.5,
                height: 0.25,
            }),
            quarter_turns: 1,
            ..Default::default()
        };
        let result = recipe.apply(&source).expect("edit");
        assert_eq!(result.dimensions(), (1, 2));
        assert_eq!(*result.get_pixel(0, 0), Rgba([2, 1, 0, 255]));
        assert_eq!(*result.get_pixel(0, 1), Rgba([3, 1, 0, 255]));
    }

    #[test]
    fn edit_color_changes_rgb_but_preserves_transparency() {
        let source = RgbaImage::from_pixel(2, 2, Rgba([60, 80, 100, 42]));
        let recipe = EditRecipe {
            exposure: 1.0,
            ..Default::default()
        };
        let result = recipe.apply(&source).expect("edit");
        assert_eq!(*result.get_pixel(0, 0), Rgba([120, 160, 200, 42]));
    }

    #[test]
    fn edit_invalid_crop_nan_and_oversized_resize_are_rejected() {
        assert!(
            EditRecipe {
                exposure: f32::NAN,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            EditRecipe {
                resize: Some((0, 100)),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            EditRecipe {
                resize: Some((32_768, 32_768)),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            EditRecipe {
                crop: Some(Crop {
                    x: 0.9,
                    y: 0.0,
                    width: 0.5,
                    height: 1.0
                }),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
