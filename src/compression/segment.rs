use std::collections::{HashMap, VecDeque};

use palette::{IntoColor, Lab, Srgb};

use crate::{
    model::{ImageColorSpace, SegmentationMetadata, SegmentationStrategy, SegmentedMrc},
    profile::SegmentationPolicy,
};

use super::rasterize::NormalizedRaster;

const EDGE_MIN: f32 = 12.0;
const EDGE_STRONG: f32 = 28.0;
const MIN_COMPONENT_AREA: usize = 3;
const MONOCHROME_CHROMA_MAX: f32 = 6.5;

pub fn segment_raster(raster: &NormalizedRaster, policy: &SegmentationPolicy) -> SegmentedMrc {
    let pixels = build_pixels(raster);
    let mut metadata = SegmentationMetadata::default();
    let alpha_ratio = pixels.iter().filter(|pixel| pixel.alpha < 250).count() as f32
        / pixels.len().max(1) as f32;
    metadata.source_alpha_used = alpha_ratio > 0.002;

    let edge_map = compute_edge_map(&pixels, raster.width as usize, raster.height as usize);
    let colorful_ratio = pixels
        .iter()
        .filter(|pixel| pixel.chroma > 18.0 && pixel.edge > EDGE_MIN)
        .count() as f32
        / pixels.len().max(1) as f32;

    let strategy = if metadata.source_alpha_used {
        SegmentationStrategy::AlphaMask
    } else if colorful_ratio > 0.08 {
        SegmentationStrategy::ColorClusterMrc
    } else {
        SegmentationStrategy::DarkEdgeHybrid
    };
    metadata.strategy = strategy.clone();

    let mut mask = match strategy {
        SegmentationStrategy::AlphaMask => alpha_mask(&pixels, raster.width as usize, raster.height as usize, policy),
        SegmentationStrategy::ColorClusterMrc => {
            color_cluster_mask(&pixels, raster.width as usize, raster.height as usize, policy)
        }
        SegmentationStrategy::DarkEdgeHybrid | SegmentationStrategy::NaiveThreshold => {
            dark_edge_mask(&pixels, raster.width as usize, raster.height as usize, policy)
        }
        SegmentationStrategy::PlaceholderBackgroundOnly => vec![0u8; pixels.len()],
    };

    if policy.morphology_radius > 0 {
        let radius = usize::from(policy.morphology_radius.min(2));
        mask = dilate_mask(&mask, raster.width as usize, raster.height as usize, radius);
        mask = erode_mask(&mask, raster.width as usize, raster.height as usize, radius);
    }

    mask = filter_components(
        &mask,
        &edge_map,
        raster.width as usize,
        raster.height as usize,
        &mut metadata,
    );

    let selected = mask.iter().filter(|value| **value > 0).count();
    let coverage = selected as f32 / pixels.len().max(1) as f32;
    if !(0.0025..0.78).contains(&coverage) {
        metadata.warnings.push(format!(
            "segmentation coverage {:.2}% fell outside the useful MRC range; using background-only",
            coverage * 100.0
        ));
        return background_only(&pixels, raster.width, raster.height, metadata);
    }

    let mut background_rgb = Vec::with_capacity(pixels.len() * 3);
    let mut foreground_rgb = Vec::with_capacity(pixels.len() * 3);
    let mut foreground_gray = Vec::with_capacity(pixels.len());
    let mut selected_colored = 0usize;
    let mut selected_total = 0usize;
    for (pixel, keep) in pixels.iter().zip(mask.iter()) {
        if *keep > 0 {
            selected_total += 1;
            if pixel.chroma > MONOCHROME_CHROMA_MAX {
                selected_colored += 1;
            }
            background_rgb.extend_from_slice(&[255, 255, 255]);
        } else {
            background_rgb.extend_from_slice(&pixel.composited_rgb);
        }
        foreground_rgb.extend_from_slice(&pixel.composited_rgb);
        foreground_gray.push(rgb_to_gray(pixel.composited_rgb));
    }

    let colored_ratio = selected_colored as f32 / selected_total.max(1) as f32;
    let use_gray_foreground = selected_total > 0 && colored_ratio < 0.05;

    let (foreground_rgb_or_gray, foreground_color_space, color_mode_label) = if use_gray_foreground {
        (
            Some(foreground_gray),
            ImageColorSpace::Gray8,
            format!("gray foreground ({:.2}% colored foreground pixels)", colored_ratio * 100.0),
        )
    } else {
        (
            Some(foreground_rgb),
            ImageColorSpace::Rgb8,
            format!("rgb foreground ({:.2}% colored foreground pixels)", colored_ratio * 100.0),
        )
    };

    metadata.warnings.push(format!(
        "{:?} selected {:.2}% of pixels; colorful ratio {:.2}%; {}",
        metadata.strategy,
        coverage * 100.0,
        colorful_ratio * 100.0,
        color_mode_label
    ));

    SegmentedMrc {
        width: raster.width,
        height: raster.height,
        background_rgb,
        foreground_rgb_or_gray,
        foreground_color_space,
        mask_1bpp: Some(mask),
        metadata,
    }
}

#[derive(Clone)]
struct PixelInfo {
    composited_rgb: [u8; 3],
    alpha: u8,
    luminance: f32,
    chroma: f32,
    edge: f32,
}

fn build_pixels(raster: &NormalizedRaster) -> Vec<PixelInfo> {
    let mut pixels = raster
        .rgba
        .chunks_exact(4)
        .map(|rgba| {
            let alpha = rgba[3];
            let composited = composite_over_white(rgba[0], rgba[1], rgba[2], alpha);
            let lab: Lab = Srgb::new(
                composited[0] as f32 / 255.0,
                composited[1] as f32 / 255.0,
                composited[2] as f32 / 255.0,
            )
            .into_linear()
            .into_color();
            PixelInfo {
                composited_rgb: composited,
                alpha,
                luminance: lab.l,
                chroma: (lab.a * lab.a + lab.b * lab.b).sqrt(),
                edge: 0.0,
            }
        })
        .collect::<Vec<_>>();

    let edges = compute_edge_map(&pixels, raster.width as usize, raster.height as usize);
    for (pixel, edge) in pixels.iter_mut().zip(edges) {
        pixel.edge = edge;
    }
    pixels
}

fn alpha_mask(
    pixels: &[PixelInfo],
    width: usize,
    height: usize,
    policy: &SegmentationPolicy,
) -> Vec<u8> {
    let soft_band = policy.alpha_threshold.saturating_add(48);
    let mut mask = vec![0u8; pixels.len()];
    for (index, pixel) in pixels.iter().enumerate() {
        let mut selected = pixel.alpha > policy.alpha_threshold;
        if !selected && pixel.alpha > policy.alpha_threshold.saturating_sub(8) && pixel.edge > EDGE_MIN {
            selected = true;
        }
        if !selected && pixel.alpha > soft_band && pixel.edge > EDGE_STRONG {
            selected = true;
        }
        if selected {
            mask[index] = 255;
        }
    }
    dilate_mask(&mask, width, height, 1)
}

fn dark_edge_mask(
    pixels: &[PixelInfo],
    width: usize,
    height: usize,
    policy: &SegmentationPolicy,
) -> Vec<u8> {
    let threshold_l = u8_to_lab_lightness(policy.foreground_dark_threshold);
    let mut mask = vec![0u8; pixels.len()];
    for (index, pixel) in pixels.iter().enumerate() {
        let darkness = pixel.luminance <= threshold_l;
        let strong_edge = pixel.edge >= EDGE_STRONG;
        let moderate_edge = pixel.edge >= EDGE_MIN && pixel.luminance <= threshold_l + 10.0;
        let saturated_stroke = pixel.chroma >= 20.0 && pixel.edge >= EDGE_MIN && pixel.luminance <= 88.0;
        if darkness || strong_edge || moderate_edge || saturated_stroke {
            mask[index] = 255;
        }
    }
    cleanup_mask(&mask, width, height)
}

fn color_cluster_mask(
    pixels: &[PixelInfo],
    width: usize,
    height: usize,
    policy: &SegmentationPolicy,
) -> Vec<u8> {
    let mut bins: HashMap<(i16, i16, i16), usize> = HashMap::new();
    let mut keys = Vec::with_capacity(pixels.len());
    for pixel in pixels {
        let lab: Lab = Srgb::new(
            pixel.composited_rgb[0] as f32 / 255.0,
            pixel.composited_rgb[1] as f32 / 255.0,
            pixel.composited_rgb[2] as f32 / 255.0,
        )
        .into_linear()
        .into_color();
        let key = (
            (lab.l / 8.0).round() as i16,
            (lab.a / 12.0).round() as i16,
            (lab.b / 12.0).round() as i16,
        );
        *bins.entry(key).or_default() += 1;
        keys.push(key);
    }

    let dominant_bin = bins
        .iter()
        .max_by_key(|(_, count)| **count)
        .map(|(key, _)| *key);
    let threshold_l = u8_to_lab_lightness(policy.foreground_dark_threshold);
    let mut mask = vec![0u8; pixels.len()];
    for (index, pixel) in pixels.iter().enumerate() {
        let key = keys[index];
        let bin_count = *bins.get(&key).unwrap_or(&0) as f32 / pixels.len().max(1) as f32;
        let is_dominant = Some(key) == dominant_bin;
        let cluster_foreground = !is_dominant && (bin_count < 0.18 || pixel.edge > EDGE_MIN);
        let dark_fallback = pixel.luminance <= threshold_l && pixel.edge >= EDGE_MIN * 0.7;
        if cluster_foreground || dark_fallback {
            mask[index] = 255;
        }
    }
    cleanup_mask(&mask, width, height)
}

fn cleanup_mask(mask: &[u8], width: usize, height: usize) -> Vec<u8> {
    let dilated = dilate_mask(mask, width, height, 1);
    erode_mask(&dilated, width, height, 1)
}

fn filter_components(
    mask: &[u8],
    edge_map: &[f32],
    width: usize,
    height: usize,
    metadata: &mut SegmentationMetadata,
) -> Vec<u8> {
    let mut output = vec![0u8; mask.len()];
    let mut visited = vec![false; mask.len()];
    let neighbors = [
        (-1isize, -1isize),
        (-1, 0),
        (-1, 1),
        (0, -1),
        (0, 1),
        (1, -1),
        (1, 0),
        (1, 1),
    ];
    let mut removed = 0usize;

    for start in 0..mask.len() {
        if visited[start] || mask[start] == 0 {
            continue;
        }
        visited[start] = true;
        let mut queue = VecDeque::from([start]);
        let mut component = Vec::new();
        let mut min_x = width;
        let mut min_y = height;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        let mut edge_sum = 0.0f32;

        while let Some(index) = queue.pop_front() {
            component.push(index);
            let x = index % width;
            let y = index / width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            edge_sum += edge_map[index];
            for (dx, dy) in neighbors {
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                    continue;
                }
                let neighbor = ny as usize * width + nx as usize;
                if !visited[neighbor] && mask[neighbor] > 0 {
                    visited[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }

        let area = component.len();
        let bbox_w = max_x.saturating_sub(min_x) + 1;
        let bbox_h = max_y.saturating_sub(min_y) + 1;
        let avg_edge = edge_sum / area.max(1) as f32;
        let stroke_like = bbox_w <= 3 || bbox_h <= 3 || bbox_w as f32 / bbox_h.max(1) as f32 > 6.0;
        let huge_low_detail = area > mask.len() / 5 && avg_edge < EDGE_MIN;
        let keep = (area >= MIN_COMPONENT_AREA || stroke_like) && !huge_low_detail;

        if keep {
            for index in component {
                output[index] = 255;
            }
        } else {
            removed += 1;
        }
    }

    if removed > 0 {
        metadata
            .warnings
            .push(format!("connected-component cleanup removed {removed} low-value blobs"));
    }
    output
}

fn compute_edge_map(pixels: &[PixelInfo], width: usize, height: usize) -> Vec<f32> {
    let mut edges = vec![0.0f32; pixels.len()];
    if width < 3 || height < 3 {
        return edges;
    }
    let lum = pixels.iter().map(|pixel| pixel.luminance).collect::<Vec<_>>();
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let idx = y * width + x;
            let left = lum[idx - 1];
            let right = lum[idx + 1];
            let up = lum[idx - width];
            let down = lum[idx + width];
            let up_left = lum[idx - width - 1];
            let up_right = lum[idx - width + 1];
            let down_left = lum[idx + width - 1];
            let down_right = lum[idx + width + 1];

            let gx = -up_left - (2.0 * left) - down_left + up_right + (2.0 * right) + down_right;
            let gy = -up_left - (2.0 * up) - up_right + down_left + (2.0 * down) + down_right;
            edges[idx] = (gx * gx + gy * gy).sqrt();
        }
    }
    edges
}

fn dilate_mask(mask: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    if radius == 0 {
        return mask.to_vec();
    }
    let mut out = vec![0u8; mask.len()];
    for y in 0..height {
        for x in 0..width {
            let mut on = false;
            'neighbors: for ny in y.saturating_sub(radius)..=(y + radius).min(height - 1) {
                for nx in x.saturating_sub(radius)..=(x + radius).min(width - 1) {
                    if mask[ny * width + nx] > 0 {
                        on = true;
                        break 'neighbors;
                    }
                }
            }
            if on {
                out[y * width + x] = 255;
            }
        }
    }
    out
}

fn erode_mask(mask: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    if radius == 0 {
        return mask.to_vec();
    }
    let mut out = vec![0u8; mask.len()];
    for y in 0..height {
        for x in 0..width {
            let mut on = true;
            'neighbors: for ny in y.saturating_sub(radius)..=(y + radius).min(height - 1) {
                for nx in x.saturating_sub(radius)..=(x + radius).min(width - 1) {
                    if mask[ny * width + nx] == 0 {
                        on = false;
                        break 'neighbors;
                    }
                }
            }
            if on {
                out[y * width + x] = 255;
            }
        }
    }
    out
}

fn background_only(
    pixels: &[PixelInfo],
    width: u32,
    height: u32,
    metadata: SegmentationMetadata,
) -> SegmentedMrc {
    SegmentedMrc {
        width,
        height,
        background_rgb: pixels
            .iter()
            .flat_map(|pixel| pixel.composited_rgb)
            .collect(),
        foreground_rgb_or_gray: None,
        foreground_color_space: ImageColorSpace::Rgb8,
        mask_1bpp: None,
        metadata,
    }
}

fn composite_over_white(r: u8, g: u8, b: u8, a: u8) -> [u8; 3] {
    let alpha = a as f32 / 255.0;
    [
        blend_channel(r, alpha),
        blend_channel(g, alpha),
        blend_channel(b, alpha),
    ]
}

fn blend_channel(channel: u8, alpha: f32) -> u8 {
    ((channel as f32 * alpha) + (255.0 * (1.0 - alpha))).round() as u8
}

fn rgb_to_gray(rgb: [u8; 3]) -> u8 {
    ((rgb[0] as u32 * 77 + rgb[1] as u32 * 150 + rgb[2] as u32 * 29) >> 8) as u8
}

fn u8_to_lab_lightness(value: u8) -> f32 {
    let lab: Lab = Srgb::new(
        value as f32 / 255.0,
        value as f32 / 255.0,
        value as f32 / 255.0,
    )
    .into_linear()
    .into_color();
    lab.l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hybrid_mask_picks_dark_shape() {
        let mut rgba = vec![255u8; 12 * 12 * 4];
        for y in 3..9 {
            for x in 4..8 {
                let idx = (y * 12 + x) * 4;
                rgba[idx] = 0;
                rgba[idx + 1] = 0;
                rgba[idx + 2] = 0;
            }
        }
        let raster = NormalizedRaster {
            width: 12,
            height: 12,
            rgba,
        };
        let segmented = segment_raster(
            &raster,
            &SegmentationPolicy {
                alpha_threshold: 32,
                foreground_dark_threshold: 172,
                morphology_radius: 1,
            },
        );
        assert!(segmented.mask_1bpp.is_some());
        let selected = segmented
            .mask_1bpp
            .unwrap()
            .into_iter()
            .filter(|value| *value > 0)
            .count();
        assert!(selected >= 12);
        assert!(matches!(segmented.foreground_color_space, ImageColorSpace::Gray8));
    }
}
