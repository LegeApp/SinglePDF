use anyhow::Context;

use crate::model::{EncodedImageAsset, ImageColorSpace, ImageEncoding};

#[derive(Clone, Debug)]
pub struct Jbig2Encoded {
    pub global_data: Option<Vec<u8>>,
    pub page_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn encode_mask(mask: &[u8], width: u32, height: u32) -> anyhow::Result<Jbig2Encoded> {
    let normalized = mask
        .iter()
        .map(|value| if *value > 0 { 1u8 } else { 0u8 })
        .collect::<Vec<_>>();
    let encoded = jbig2enc_rust::encode_single_image_lossless(&normalized, width, height, true)
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .with_context(|| format!("failed to encode JBIG2 mask ({}x{})", width, height))?;
    Ok(Jbig2Encoded {
        global_data: encoded.global_data,
        page_data: encoded.page_data,
        width,
        height,
    })
}

pub fn encode_mask_asset(mask: &[u8], width: u32, height: u32) -> anyhow::Result<EncodedImageAsset> {
    let encoded = encode_mask(mask, width, height)?;
    Ok(EncodedImageAsset {
        pixel_width: encoded.width,
        pixel_height: encoded.height,
        bits_per_component: 1,
        color_space: ImageColorSpace::Gray8,
        encoding: ImageEncoding::Jbig2,
        bytes: encoded.page_data,
        jbig2_globals: encoded.global_data,
    })
}
