use image::DynamicImage;

use crate::{
    model::{EncodedImageAsset, ImageColorSpace},
    profile::JpxForegroundMode,
};
use openjp2::{encode_gray8_to_jp2_vec, encode_rgb8_to_jp2_vec, EncodeRgb8Options};

#[derive(Clone, Copy, Debug)]
pub enum JpxContainer {
    Jp2,
}

#[derive(Clone, Debug)]
pub struct JpxEncoded {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub colorspace: ImageColorSpace,
    pub container: JpxContainer,
}

pub fn encode_dynamic_image(
    image: &DynamicImage,
    mode: &JpxForegroundMode,
) -> anyhow::Result<JpxEncoded> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let bytes = encode_rgb8_to_jp2_vec(width, height, &rgb, encode_opts(mode))
        .map_err(anyhow::Error::msg)?;

    Ok(JpxEncoded {
        bytes,
        width,
        height,
        colorspace: ImageColorSpace::Rgb8,
        container: JpxContainer::Jp2,
    })
}

pub fn encode_dynamic_image_asset(
    image: &DynamicImage,
    mode: &JpxForegroundMode,
) -> anyhow::Result<EncodedImageAsset> {
    match image {
        DynamicImage::ImageLuma8(gray) => {
            let (width, height) = gray.dimensions();
            encode_gray8_asset(gray.as_raw(), width, height, mode)
        }
        DynamicImage::ImageLumaA8(gray_alpha) => {
            let gray = DynamicImage::ImageLumaA8(gray_alpha.clone()).to_luma8();
            let (width, height) = gray.dimensions();
            encode_gray8_asset(gray.as_raw(), width, height, mode)
        }
        _ => {
            let rgb = flatten_to_rgb_white(image);
            let (width, height) = rgb.dimensions();
            encode_rgb8_asset(rgb.as_raw(), width, height, mode)
        }
    }
}

fn flatten_to_rgb_white(image: &DynamicImage) -> image::RgbImage {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut out = image::RgbImage::new(width, height);
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = a as u16;
        let inv = 255u16.saturating_sub(alpha);
        let blend = |component: u8| -> u8 {
            (((component as u16 * alpha) + (255u16 * inv) + 127) / 255) as u8
        };
        out.put_pixel(x, y, image::Rgb([blend(r), blend(g), blend(b)]));
    }
    out
}

pub fn encode_rgb8_asset(
    pixels: &[u8],
    width: u32,
    height: u32,
    mode: &JpxForegroundMode,
) -> anyhow::Result<EncodedImageAsset> {
    let bytes = encode_rgb8_to_jp2_vec(width, height, pixels, encode_opts(mode))
        .map_err(anyhow::Error::msg)?;

    Ok(EncodedImageAsset {
        pixel_width: width,
        pixel_height: height,
        bits_per_component: 8,
        color_space: ImageColorSpace::Rgb8,
        encoding: crate::model::ImageEncoding::Jpeg2000,
        bytes,
        jbig2_globals: None,
    })
}

pub fn encode_gray8_asset(
    pixels: &[u8],
    width: u32,
    height: u32,
    mode: &JpxForegroundMode,
) -> anyhow::Result<EncodedImageAsset> {
    let bytes = encode_gray8_to_jp2_vec(width, height, pixels, encode_opts(mode))
        .map_err(anyhow::Error::msg)?;

    Ok(EncodedImageAsset {
        pixel_width: width,
        pixel_height: height,
        bits_per_component: 8,
        color_space: ImageColorSpace::Gray8,
        encoding: crate::model::ImageEncoding::Jpeg2000,
        bytes,
        jbig2_globals: None,
    })
}

fn encode_opts(mode: &JpxForegroundMode) -> EncodeRgb8Options {
    match mode {
        JpxForegroundMode::Lossless => EncodeRgb8Options {
            num_resolutions: Some(3),
            rate: Some(0.0),
        },
        // `quality` here is treated as an OpenJPEG "rate" (compression ratio / target).
        // We keep it simple and let upstream tuning evolve over time.
        JpxForegroundMode::Lossy { quality } => EncodeRgb8Options {
            num_resolutions: Some(3),
            rate: Some(*quality),
        },
    }
}
