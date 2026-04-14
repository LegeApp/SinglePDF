use crate::{
    encode::encode_rgb8_to_jpeg,
    model::{
        CompressionDiagnostics, CompressionStageRecord, CompressionStageStatus, MrcAsset,
        RasterAsset, RasterIntent, RasterMode,
    },
    profile::CompressionPolicy,
};

use super::{
    encode_jbig2::encode_mask_asset,
    encode_jpx::{encode_gray8_asset, encode_rgb8_asset},
    rasterize::NormalizedRaster,
    segment::segment_raster,
    CompressedRasterResult,
};

pub fn build_mrc_asset(
    raster: &NormalizedRaster,
    policy: &CompressionPolicy,
    intent: RasterIntent,
) -> anyhow::Result<CompressedRasterResult> {
    let mut diagnostics = CompressionDiagnostics {
        intent,
        attempted_mode: RasterMode::Mrc,
        actual_mode: RasterMode::Mrc,
        fallback_used: false,
        stages: Vec::new(),
        warnings: Vec::new(),
    };

    diagnostics.stages.push(CompressionStageRecord {
        stage: "segment",
        status: CompressionStageStatus::Attempted,
        detail: Some(format!("{}x{}", raster.width, raster.height)),
    });
    let segmented = segment_raster(raster, &policy.mrc.segmentation);
    diagnostics.stages.push(CompressionStageRecord {
        stage: "segment",
        status: CompressionStageStatus::Succeeded,
        detail: Some(format!("{:?}", segmented.metadata.strategy)),
    });
    diagnostics
        .warnings
        .extend(segmented.metadata.warnings.iter().cloned());

    diagnostics.stages.push(CompressionStageRecord {
        stage: "encode-background",
        status: CompressionStageStatus::Attempted,
        detail: None,
    });
    let background = match encode_rgb8_asset(
        &segmented.background_rgb,
        segmented.width,
        segmented.height,
        &policy.mrc.background_jpx,
    ) {
        Ok(asset) => {
            diagnostics.stages.push(CompressionStageRecord {
                stage: "encode-background",
                status: CompressionStageStatus::Succeeded,
                detail: Some(format!("JPX {} bytes", asset.bytes.len())),
            });
            asset
        }
        Err(error) => {
            diagnostics.warnings.push(format!(
                "background JPX adapter fallback to JPEG: {error}"
            ));
            let asset = encode_rgb8_to_jpeg(
                &segmented.background_rgb,
                segmented.width,
                segmented.height,
                82,
                true,
            )?;
            diagnostics.stages.push(CompressionStageRecord {
                stage: "encode-background",
                status: CompressionStageStatus::Succeeded,
                detail: Some(format!("JPEG fallback {} bytes", asset.bytes.len())),
            });
            asset
        }
    };

    let foreground = if let Some(bytes) = segmented.foreground_rgb_or_gray.as_ref() {
        diagnostics.stages.push(CompressionStageRecord {
            stage: "encode-foreground",
            status: CompressionStageStatus::Attempted,
            detail: Some(format!("{:?}", segmented.foreground_color_space)),
        });
        let encode_result = match segmented.foreground_color_space {
            crate::model::ImageColorSpace::Rgb8 => {
                encode_rgb8_asset(bytes, segmented.width, segmented.height, &policy.mrc.foreground_jpx)
            }
            crate::model::ImageColorSpace::Gray8 => {
                encode_gray8_asset(bytes, segmented.width, segmented.height, &policy.mrc.foreground_jpx)
            }
        };
        let asset = match encode_result {
            Ok(asset) => {
                diagnostics.stages.push(CompressionStageRecord {
                    stage: "encode-foreground",
                    status: CompressionStageStatus::Succeeded,
                    detail: Some(format!(
                        "JPX {:?} {} bytes",
                        asset.color_space,
                        asset.bytes.len()
                    )),
                });
                asset
            }
            Err(error) => {
                diagnostics.warnings.push(format!(
                    "foreground JPX adapter fallback to JPEG: {error}"
                ));
                let rgb_bytes = match segmented.foreground_color_space {
                    crate::model::ImageColorSpace::Rgb8 => bytes.clone(),
                    crate::model::ImageColorSpace::Gray8 => bytes
                        .iter()
                        .flat_map(|gray| [*gray, *gray, *gray])
                        .collect::<Vec<_>>(),
                };
                let asset = encode_rgb8_to_jpeg(&rgb_bytes, segmented.width, segmented.height, 82, true)?;
                diagnostics.stages.push(CompressionStageRecord {
                    stage: "encode-foreground",
                    status: CompressionStageStatus::Succeeded,
                    detail: Some(format!("JPEG fallback {:?} {} bytes", segmented.foreground_color_space, asset.bytes.len())),
                });
                asset
            }
        };
        Some(asset)
    } else {
        diagnostics.stages.push(CompressionStageRecord {
            stage: "encode-foreground",
            status: CompressionStageStatus::Skipped,
            detail: Some("segmentation produced background-only page".to_string()),
        });
        None
    };

    let mask = if let Some(mask) = segmented.mask_1bpp.as_ref() {
        diagnostics.stages.push(CompressionStageRecord {
            stage: "encode-mask",
            status: CompressionStageStatus::Attempted,
            detail: None,
        });
        let asset = encode_mask_asset(mask, segmented.width, segmented.height)?;
        diagnostics.stages.push(CompressionStageRecord {
            stage: "encode-mask",
            status: CompressionStageStatus::Succeeded,
            detail: Some(format!("{} bytes", asset.bytes.len())),
        });
        Some(asset)
    } else {
        diagnostics.stages.push(CompressionStageRecord {
            stage: "encode-mask",
            status: CompressionStageStatus::Skipped,
            detail: Some("segmentation produced no binary mask".to_string()),
        });
        None
    };

    Ok(CompressedRasterResult {
        raster: RasterAsset::Mrc(MrcAsset {
            width: segmented.width,
            height: segmented.height,
            background: Some(background),
            foreground,
            mask,
            diagnostics: diagnostics.clone(),
        }),
        diagnostics,
    })
}
