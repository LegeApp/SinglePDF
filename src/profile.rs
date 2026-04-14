#[derive(Clone, Debug)]
pub struct SinglePdfProfile {
    pub capture: CapturePolicy,
    pub pagination: PaginationPolicy,
    pub compression: CompressionPolicy,
}

impl Default for SinglePdfProfile {
    fn default() -> Self {
        Self {
            capture: CapturePolicy {
                ignore_videos: true,
                ignore_audio: true,
                ignore_interactive: true,
                ignore_scripts_after_capture: true,
            },
            pagination: PaginationPolicy {
                keep_headings_with_following_block: true,
                keep_images_atomic: true,
                keep_tables_atomic: true,
            },
            compression: CompressionPolicy {
                text_as_text: true,
                default_raster_mode: DefaultRasterMode::LegacyJpeg,
                fallback_mode: RasterFallbackMode::PageLegacy,
                text_overlay: TextOverlayMode::Keep,
                page_mrc: MrcMode::Disabled,
                block_mrc: MrcMode::Disabled,
                image_strategy: LegacyImageCompressionStrategy::Jpeg2000 {
                    mode: JpxForegroundMode::Lossy { quality: 18.0 },
                },
                line_art_strategy: LineArtCompressionStrategy::LosslessPreferred,
                jpx_backend: JpxBackend::LocalOpenJp2,
                jbig2_backend: Jbig2Backend::LocalJbig2Rust,
                mrc: MrcCompressionPolicy {
                    page_raster_dpi: 110,
                    segmentation: SegmentationPolicy {
                        alpha_threshold: 32,
                        foreground_dark_threshold: 172,
                        morphology_radius: 1,
                    },
                    background_jpx: JpxForegroundMode::Lossy { quality: 18.0 },
                    foreground_jpx: JpxForegroundMode::Lossless,
                    mask_jbig2: Jbig2MaskMode::PdfLosslessMode,
                    debug_dump_layers: false,
                },
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct CapturePolicy {
    pub ignore_videos: bool,
    pub ignore_audio: bool,
    pub ignore_interactive: bool,
    pub ignore_scripts_after_capture: bool,
}

#[derive(Clone, Debug)]
pub struct PaginationPolicy {
    pub keep_headings_with_following_block: bool,
    pub keep_images_atomic: bool,
    pub keep_tables_atomic: bool,
}

#[derive(Clone, Debug)]
pub struct CompressionPolicy {
    pub text_as_text: bool,
    pub default_raster_mode: DefaultRasterMode,
    pub fallback_mode: RasterFallbackMode,
    pub text_overlay: TextOverlayMode,
    pub page_mrc: MrcMode,
    pub block_mrc: MrcMode,
    pub image_strategy: LegacyImageCompressionStrategy,
    pub line_art_strategy: LineArtCompressionStrategy,
    pub jpx_backend: JpxBackend,
    pub jbig2_backend: Jbig2Backend,
    pub mrc: MrcCompressionPolicy,
}

#[derive(Clone, Debug)]
pub enum LegacyImageCompressionStrategy {
    Jpeg {
        quality: u8,
        chroma_subsampling_420: bool,
    },
    Jpeg2000 {
        mode: JpxForegroundMode,
    },
}

#[derive(Clone, Debug)]
pub enum LineArtCompressionStrategy {
    LosslessPreferred,
}

#[derive(Clone, Debug)]
pub enum DefaultRasterMode {
    LegacyJpeg,
    MrcPreferred,
}

#[derive(Clone, Debug)]
pub enum RasterFallbackMode {
    AssetLegacy,
    PageLegacy,
    FailFast,
}

#[derive(Clone, Debug)]
pub enum TextOverlayMode {
    Keep,
    FlattenIntoRaster,
}

#[derive(Clone, Debug)]
pub enum MrcMode {
    Disabled,
    Enabled,
}

#[derive(Clone, Debug)]
pub enum JpxBackend {
    LocalOpenJp2,
}

#[derive(Clone, Debug)]
pub enum Jbig2Backend {
    LocalJbig2Rust,
}

#[derive(Clone, Debug)]
pub enum JpxForegroundMode {
    Lossy { quality: f32 },
    Lossless,
}

#[derive(Clone, Debug)]
pub enum Jbig2MaskMode {
    PdfSymbolMode,
    PdfLosslessMode,
}

#[derive(Clone, Debug)]
pub struct MrcCompressionPolicy {
    pub page_raster_dpi: u32,
    pub segmentation: SegmentationPolicy,
    pub background_jpx: JpxForegroundMode,
    pub foreground_jpx: JpxForegroundMode,
    pub mask_jbig2: Jbig2MaskMode,
    pub debug_dump_layers: bool,
}

#[derive(Clone, Debug)]
pub struct SegmentationPolicy {
    pub alpha_threshold: u8,
    pub foreground_dark_threshold: u8,
    pub morphology_radius: u8,
}
