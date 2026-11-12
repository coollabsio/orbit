//! Makes a large photo or screenshot smaller: long edge at most 2560 px, colors in sRGB, WebP. A PNG (most often a
//! screenshot, where lossy artifacts show around text) becomes a lossless WebP; a JPEG becomes a lossy one. The
//! upload service runs it on each upload, so a file from a client that did not shrink it (an import, the mobile
//! app, an API client) is stored small too. Admin → Storage runs it on files stored before. The web client
//! (`apps/web/src/lib/shrinkImage.ts`) shrinks JPEG and WebP the same way before the upload, and leaves PNG to this.

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbaImage};
use moxcms::{ColorProfile, Layout, TransformOptions};

const MAX_IMAGE_EDGE: u32 = 2560;
/// Smaller images stay as they are: re-encoding saves little and loses quality.
pub const SHRINK_MIN_BYTES: u64 = 512 * 1024;
const QUALITY: f32 = 85.0;
/// The most decoded pixel bytes one image may take. The header sets the size of the decode buffer, so a small
/// file that claims a huge size would otherwise make the allocation fail and stop the whole process.
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;

/// Re-encodes `bytes` as a WebP of at most `MAX_IMAGE_EDGE` px: lossless for a PNG, lossy for a JPEG or WebP. `None` when it is not a still JPEG, PNG or WebP,
/// cannot be decoded, or would not get smaller. CPU-heavy: call it on a blocking thread.
#[must_use]
pub fn shrink_image(bytes: &[u8]) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
    ) || is_animated(bytes)
    {
        return None;
    }
    let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
        .into_decoder()
        .ok()?;
    if decoder.total_bytes() > MAX_DECODED_BYTES {
        return None;
    }
    // Browsers show a photo turned as its EXIF orientation says; the WebP has no EXIF, so turn the pixels.
    let orientation = decoder.orientation().ok()?;
    // The WebP has no color profile either, so a Display P3 photo (an iPhone's) would look dull: convert to sRGB.
    let profile = decoder.icc_profile().ok().flatten();
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    let fits = image.width().max(image.height()) <= MAX_IMAGE_EDGE;
    // A WebP that fits is already what this produces: encoding it again only loses quality.
    if fits && format == ImageFormat::WebP {
        return None;
    }
    if !fits {
        image = image.resize(MAX_IMAGE_EDGE, MAX_IMAGE_EDGE, FilterType::Lanczos3);
    }
    let mut rgba = image.to_rgba8();
    if let Some(profile) = profile {
        to_srgb(&mut rgba, &profile);
    }
    let encoder = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height());
    let encoded = if format == ImageFormat::Png {
        encoder.encode_lossless()
    } else {
        encoder.encode(QUALITY)
    };
    (encoded.len() < bytes.len()).then(|| encoded.to_vec())
}

/// Converts the pixels from the ICC `profile` to sRGB. Leaves them as they are when the profile cannot be read or
/// is not an RGB one (a gray or CMYK profile, whose pixels the decoder already made RGB).
fn to_srgb(rgba: &mut RgbaImage, profile: &[u8]) {
    let Ok(source) = ColorProfile::new_from_slice(profile) else {
        return;
    };
    let Ok(transform) = source.create_transform_8bit(
        Layout::Rgba,
        &ColorProfile::new_srgb(),
        Layout::Rgba,
        TransformOptions::default(),
    ) else {
        return;
    };
    let mut converted = vec![0; rgba.len()];
    if transform.transform(rgba, &mut converted).is_ok() {
        rgba.copy_from_slice(&converted);
    }
}

/// Animated PNG (an `acTL` chunk) and animated WebP (an `ANIM` chunk) would lose all frames but the first. Both
/// chunks come before the image data, near the start of the file.
fn is_animated(bytes: &[u8]) -> bool {
    let header = &bytes[..bytes.len().min(4096)];
    header
        .windows(4)
        .any(|chunk| chunk == b"acTL" || chunk == b"ANIM")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    pub(crate) fn noisy_png(width: u32, height: u32) -> Vec<u8> {
        let image = ImageBuffer::from_fn(width, height, |x, y| {
            Rgba([
                (x * 7 % 256) as u8,
                (y * 13 % 256) as u8,
                ((x ^ y) % 256) as u8,
                255,
            ])
        });
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn shrink_skips_an_image_whose_header_claims_a_huge_size() {
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(ImageBuffer::new(8, 8))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Jpeg)
            .unwrap();
        // The baseline frame header (FF C0, length, precision) is followed by the height and the width.
        let sof = bytes
            .windows(2)
            .position(|pair| pair == [0xFF, 0xC0])
            .unwrap();
        bytes[sof + 5..sof + 9].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(shrink_image(&bytes).is_none());
    }

    #[test]
    fn shrink_scales_large_images_to_webp() {
        let shrunk = shrink_image(&noisy_png(3000, 1500)).unwrap();
        let image = image::load_from_memory(&shrunk).unwrap();
        assert_eq!((image.width(), image.height()), (2560, 1280));
        assert_eq!(image::guess_format(&shrunk).unwrap(), ImageFormat::WebP);
        // A second run leaves the result alone.
        assert!(shrink_image(&shrunk).is_none());
    }

    #[test]
    fn shrink_keeps_every_pixel_of_a_png() {
        let png = noisy_png(400, 300);
        let shrunk = shrink_image(&png).unwrap();
        assert_eq!(
            image::load_from_memory(&shrunk).unwrap().to_rgba8(),
            image::load_from_memory(&png).unwrap().to_rgba8()
        );
    }

    #[test]
    fn shrink_converts_display_p3_to_srgb() {
        let p3_orange = ImageBuffer::from_pixel(64, 64, image::Rgb([200_u8, 100, 60]));
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100);
        image::ImageEncoder::set_icc_profile(
            &mut encoder,
            ColorProfile::new_display_p3().encode().unwrap(),
        )
        .unwrap();
        image::ImageEncoder::write_image(
            encoder,
            &p3_orange,
            64,
            64,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
        let shrunk = shrink_image(&jpeg).unwrap();
        let pixel = *image::load_from_memory(&shrunk).unwrap().to_rgb8().get_pixel(32, 32);
        // P3 (200, 100, 60) is about sRGB (215, 92, 47); left unconverted, it would stay (200, 100, 60).
        assert!(pixel[0] >= 209 && pixel[2] <= 53, "{pixel:?}");
    }

    #[test]
    fn shrink_skips_animations_and_other_formats() {
        let mut animated = noisy_png(64, 64);
        animated.splice(
            33..33,
            b"\0\0\0\x08acTL\0\0\0\x02\0\0\0\0\0\0\0\0".iter().copied(),
        );
        assert!(shrink_image(&animated).is_none());
        assert!(shrink_image(b"GIF89a not really").is_none());
        assert!(shrink_image(b"%PDF-1.7").is_none());
    }
}
