pub mod encode_jbig2;
pub mod encode_jpx;
pub mod mrc;
pub mod rasterize;
pub mod segment;

use anyhow::Context;

use crate::{
    encode::{decode_image_source_to_dynamic_image, decode_image_source_to_jpeg_asset},
    model::{
        CompressionDiagnostics, CompressionStageRecord, CompressionStageStatus, RasterAsset,
        RasterIntent, RasterMode, RenderPage,
    },
    profile::{
        CompressionPolicy, DefaultRasterMode, LegacyImageCompressionStrategy, MrcMode,
        RasterFallbackMode,
    },
};

pub use mrc::build_mrc_asset;
pub use rasterize::{rasterize_render_page_non_text, NormalizedRaster};

#[derive(Clone, Debug)]
pub struct CompressedRasterResult {
    pub raster: RasterAsset,
    pub diagnostics: CompressionDiagnostics,
}

pub fn compress_image_source(
    source: &str,
    policy: &CompressionPolicy,
) -> anyhow::Result<CompressedRasterResult> {
    let attempted_mode = match policy.default_raster_mode {
        DefaultRasterMode::MrcPreferred if matches!(policy.block_mrc, MrcMode::Enabled) => {
            RasterMode::Mrc
        }
        _ => RasterMode::LegacyJpeg,
    };
    let mut diagnostics = CompressionDiagnostics {
        intent: RasterIntent::Block,
        attempted_mode,
        actual_mode: RasterMode::LegacyJpeg,
        fallback_used: false,
        stages: Vec::new(),
        warnings: Vec::new(),
    };

    if matches!(attempted_mode, RasterMode::Mrc) {
        diagnostics.stages.push(CompressionStageRecord {
            stage: "rasterize",
            status: CompressionStageStatus::Attempted,
            detail: Some("decoded image source for block-level MRC".to_string()),
        });
        match rasterize::normalize_image_source(source) {
            Ok(raster) => {
                diagnostics.stages.push(CompressionStageRecord {
                    stage: "rasterize",
                    status: CompressionStageStatus::Succeeded,
                    detail: Some(format!("{}x{}", raster.width, raster.height)),
                });
                match build_mrc_asset(&raster, policy, RasterIntent::Block) {
                    Ok(mut result) => {
                        result.diagnostics.stages.splice(0..0, diagnostics.stages);
                        return Ok(result);
                    }
                    Err(error) => {
                        diagnostics.fallback_used = true;
                        diagnostics.warnings.push(format!("block MRC fallback: {error}"));
                        diagnostics.stages.push(CompressionStageRecord {
                            stage: "mrc",
                            status: CompressionStageStatus::Failed,
                            detail: Some(error.to_string()),
                        });
                    }
                }
            }
            Err(error) => {
                diagnostics.fallback_used = true;
                diagnostics.warnings.push(format!("block raster decode failed: {error}"));
                diagnostics.stages.push(CompressionStageRecord {
                    stage: "rasterize",
                    status: CompressionStageStatus::Failed,
                    detail: Some(error.to_string()),
                });
            }
        }
    } else {
        diagnostics.stages.push(CompressionStageRecord {
            stage: "mrc",
            status: CompressionStageStatus::Skipped,
            detail: Some("block MRC disabled by policy".to_string()),
        });
    }

    let legacy = legacy_image_asset(source, policy)?;
    Ok(CompressedRasterResult {
        raster: RasterAsset::Single(legacy),
        diagnostics,
    })
}

pub fn compress_render_page_mrc(
    page: &RenderPage,
    policy: &CompressionPolicy,
) -> anyhow::Result<CompressedRasterResult> {
    if !matches!(policy.page_mrc, MrcMode::Enabled) {
        anyhow::bail!("page MRC disabled by profile");
    }

    let raster = rasterize_render_page_non_text(page, policy)
        .context("failed to rasterize non-text page visuals for MRC")?;
    build_mrc_asset(&raster, policy, RasterIntent::Page)
}

pub fn legacy_image_asset(
    source: &str,
    policy: &CompressionPolicy,
) -> anyhow::Result<crate::model::EncodedImageAsset> {
    match &policy.image_strategy {
        LegacyImageCompressionStrategy::Jpeg {
            quality,
            chroma_subsampling_420,
        } => decode_image_source_to_jpeg_asset(source, *quality, *chroma_subsampling_420),
        LegacyImageCompressionStrategy::Jpeg2000 { mode } => {
            let image = decode_image_source_to_dynamic_image(source)?;
            encode_jpx::encode_dynamic_image_asset(&image, mode)
        }
    }
}

pub fn page_fallback_permitted(policy: &CompressionPolicy) -> bool {
    !matches!(policy.fallback_mode, RasterFallbackMode::FailFast)
}
