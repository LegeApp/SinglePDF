use anyhow::Context;
use image::{imageops::FilterType, DynamicImage, Rgba, RgbaImage};

use crate::{
    encode::decode_image_source_to_dynamic_image,
    model::{DrawOp, ImageAsset, ImageEncoding, RasterAsset, RenderPage},
    profile::CompressionPolicy,
};

#[derive(Clone, Debug)]
pub struct NormalizedRaster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl NormalizedRaster {
    pub fn is_blank(&self) -> bool {
        self.rgba
            .chunks_exact(4)
            .all(|px| px[0] == 255 && px[1] == 255 && px[2] == 255 && px[3] == 255)
    }
}

pub fn normalize_image_source(source: &str) -> anyhow::Result<NormalizedRaster> {
    let image = decode_image_source_to_dynamic_image(source)?;
    Ok(dynamic_image_to_raster(&image))
}

pub fn rasterize_render_page_non_text(
    page: &RenderPage,
    policy: &CompressionPolicy,
) -> anyhow::Result<NormalizedRaster> {
    let dpi = policy.mrc.page_raster_dpi.max(72);
    let width = ((page.width_pt / 72.0) * dpi as f32).round().max(1.0) as u32;
    let height = ((page.height_pt / 72.0) * dpi as f32).round().max(1.0) as u32;
    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]));
    let sx = width as f32 / page.width_pt.max(1.0);
    let sy = height as f32 / page.height_pt.max(1.0);

    for op in &page.ops {
        match op {
            DrawOp::FillRect { rect, color } => {
                let (x0, y0, x1, y1) = rect_to_pixels(rect.x, rect.y, rect.w, rect.h, sx, sy, height);
                fill_rect(&mut canvas, x0, y0, x1, y1, [color.r, color.g, color.b, 255]);
            }
            DrawOp::StrokeRect { rect, color, width } => {
                let (x0, y0, x1, y1) = rect_to_pixels(rect.x, rect.y, rect.w, rect.h, sx, sy, height);
                let stroke = ((*width * sx.max(sy)).round() as u32).max(1);
                stroke_rect(
                    &mut canvas,
                    x0,
                    y0,
                    x1,
                    y1,
                    stroke,
                    [color.r, color.g, color.b, 255],
                );
            }
            DrawOp::DrawRaster { rect, raster } => {
                if let Some(image) = raster_to_dynamic_image(raster)? {
                    let target_w = (rect.w * sx).round().max(1.0) as u32;
                    let target_h = (rect.h * sy).round().max(1.0) as u32;
                    let fitted = fit_image_dimensions(target_w, target_h, image.width(), image.height());
                    let resized = image.resize_exact(fitted.0, fitted.1, FilterType::Triangle);
                    let x = (rect.x * sx).round().max(0.0) as i64 + ((target_w - fitted.0) / 2) as i64;
                    let y_from_top = (rect.y * sy).round().max(0.0) as i64
                        + ((target_h - fitted.1) / 2) as i64;
                    let y = height as i64 - y_from_top - fitted.1 as i64;
                    image::imageops::overlay(&mut canvas, &resized.to_rgba8(), x, y);
                }
            }
            DrawOp::DrawText(_) => {}
        }
    }

    let raster = NormalizedRaster {
        width,
        height,
        rgba: canvas.into_raw(),
    };
    if raster.is_blank() {
        anyhow::bail!("page has no rasterizable non-text visuals");
    }
    Ok(raster)
}

fn dynamic_image_to_raster(image: &DynamicImage) -> NormalizedRaster {
    let rgba = image.to_rgba8();
    NormalizedRaster {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    }
}

fn raster_to_dynamic_image(raster: &RasterAsset) -> anyhow::Result<Option<DynamicImage>> {
    match raster {
        RasterAsset::Single(image) => decode_image_asset(image).map(Some),
        RasterAsset::Mrc(mrc) => {
            if let Some(background) = &mrc.background {
                decode_image_asset(background).map(Some)
            } else {
                Ok(None)
            }
        }
    }
}

fn decode_image_asset(image: &ImageAsset) -> anyhow::Result<DynamicImage> {
    match image.encoding {
        ImageEncoding::Jpeg { .. } | ImageEncoding::Jpeg2000 | ImageEncoding::FlateBitmap => {
            image::load_from_memory(&image.bytes).context("failed to decode raster asset bytes")
        }
        ImageEncoding::Jbig2 => anyhow::bail!("cannot rasterize JBIG2 asset back into RGBA"),
    }
}

fn rect_to_pixels(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    sx: f32,
    sy: f32,
    canvas_height: u32,
) -> (u32, u32, u32, u32) {
    let x0 = (x * sx).floor().max(0.0) as u32;
    let x1 = ((x + w) * sx).ceil().max(0.0) as u32;
    let y_top = (y * sy).floor().max(0.0) as u32;
    let y_bottom = ((y + h) * sy).ceil().max(0.0) as u32;
    let y0 = canvas_height.saturating_sub(y_bottom);
    let y1 = canvas_height.saturating_sub(y_top);
    (x0, y0, x1, y1)
}

fn fill_rect(canvas: &mut RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, color: [u8; 4]) {
    for y in y0.min(canvas.height())..y1.min(canvas.height()) {
        for x in x0.min(canvas.width())..x1.min(canvas.width()) {
            canvas.put_pixel(x, y, Rgba(color));
        }
    }
}

fn stroke_rect(
    canvas: &mut RgbaImage,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    stroke: u32,
    color: [u8; 4],
) {
    fill_rect(canvas, x0, y0, x1, y0.saturating_add(stroke), color);
    fill_rect(canvas, x0, y1.saturating_sub(stroke), x1, y1, color);
    fill_rect(canvas, x0, y0, x0.saturating_add(stroke), y1, color);
    fill_rect(canvas, x1.saturating_sub(stroke), y0, x1, y1, color);
}

fn fit_image_dimensions(target_w: u32, target_h: u32, image_w: u32, image_h: u32) -> (u32, u32) {
    let image_aspect = image_w as f32 / image_h.max(1) as f32;
    let rect_aspect = target_w as f32 / target_h.max(1) as f32;
    if image_aspect > rect_aspect {
        (target_w.max(1), ((target_w as f32 / image_aspect).round() as u32).max(1))
    } else {
        (((target_h as f32 * image_aspect).round() as u32).max(1), target_h.max(1))
    }
}
