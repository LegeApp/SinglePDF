use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotDocument {
    #[serde(skip, default)]
    pub source: SnapshotSource,
    #[serde(default)]
    pub metadata: SnapshotMetadata,
    pub html: String,
    #[serde(default)]
    pub hints: Vec<ElementLayoutHint>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct SnapshotMetadata {
    pub title: Option<String>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ElementLayoutHint {
    pub id: String,
    #[serde(default)]
    pub tag_name: Option<String>,
    pub rect: LayoutRect,
    #[serde(default)]
    pub display: Option<String>,
    #[serde(default)]
    pub rendered_width: Option<f32>,
    #[serde(default)]
    pub rendered_height: Option<f32>,
    #[serde(default, rename = "float")]
    pub float_mode: Option<String>,
    #[serde(default)]
    pub is_aside: bool,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub text_align: Option<String>,
    #[serde(default)]
    pub background_color: Option<Color>,
    #[serde(default)]
    pub likely_main_content: bool,
    #[serde(default)]
    pub role_hint: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug)]
pub enum SnapshotSource {
    File(PathBuf),
    Url(String),
    Inline,
}

impl Default for SnapshotSource {
    fn default() -> Self {
        Self::Inline
    }
}

#[derive(Clone, Debug)]
pub struct ParsedDocument {
    pub title: Option<String>,
    pub blocks: Vec<FlowBlock>,
}

#[derive(Clone, Debug)]
pub struct FlowBlock {
    pub kind: BlockKind,
    pub text: String,
    pub title: Option<String>,
    pub caption: Option<String>,
    pub source: Option<String>,
    pub table: Option<TableData>,
    pub estimated_height: f32,
    pub style: BlockStyle,
    pub spacing_before: f32,
    pub spacing_after: f32,
    pub pagination: PaginationPolicy,
    pub layout_hint: Option<LayoutHint>,
    pub table_role: Option<TableRole>,
    pub image_role: Option<ImageRole>,
    pub section_role: Option<SectionRole>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Heading { level: u8 },
    Paragraph,
    ListItem,
    Preformatted,
    BlockQuote,
    Image,
    Table,
    Aside,
    Caption,
    ReferenceList,
    Figure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableRole {
    Data,
    Infobox,
    MessageBox,
    NavBox,
    Metadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageRole {
    Content,
    Decorative,
    Figure,
    Avatar,
    Icon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionRole {
    Body,
    References,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaginationPolicy {
    Atomic,
    SplitParagraph,
    SplitTable,
    SplitPreformatted,
    KeepWithNext,
}

#[derive(Clone, Debug)]
pub struct LayoutHint {
    pub block_id: String,
    pub rect: LayoutRect,
    pub display: Option<String>,
    pub rendered_width: Option<f32>,
    pub rendered_height: Option<f32>,
    pub float_mode: Option<FloatMode>,
    pub is_aside: bool,
    pub visible: bool,
    pub text_align: Option<TextAlignment>,
    pub background_color: Option<Color>,
    pub likely_main_content: bool,
    pub role_hint: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatMode {
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub struct BlockStyle {
    pub text: TextStyle,
    pub padding: EdgeInsets,
    pub border: Option<BorderStyle>,
    pub background: Option<Color>,
    pub indent_pt: f32,
    pub alignment: TextAlignment,
}

#[derive(Clone, Copy, Debug)]
pub struct EdgeInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct BorderStyle {
    pub color: Color,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlignment {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug)]
pub struct PageLayout {
    pub width_pt: f32,
    pub height_pt: f32,
    pub margin: MarginBox,
}

impl Default for PageLayout {
    fn default() -> Self {
        Self {
            width_pt: 612.0,
            height_pt: 1008.0,
            margin: MarginBox {
                top: 36.0,
                right: 36.0,
                bottom: 42.0,
                left: 36.0,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MarginBox {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Clone, Debug)]
pub struct LogicalPage {
    pub number: usize,
    pub width_pt: f32,
    pub height_pt: f32,
    pub blocks: Vec<PositionedBlock>,
}

#[derive(Clone, Debug)]
pub struct PositionedBlock {
    pub rect: Rect,
    pub text_rect: Option<Rect>,
    pub image_rect: Option<Rect>,
    pub block: FlowBlock,
    pub lines: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Clone, Debug)]
pub struct TextStyle {
    pub font_size_pt: f32,
    pub line_height_pt: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Color,
}

#[derive(Clone, Debug)]
pub struct RenderPage {
    pub width_pt: f32,
    pub height_pt: f32,
    pub ops: Vec<DrawOp>,
    pub compression_diagnostics: Vec<CompressionDiagnostics>,
}

#[derive(Clone, Debug)]
pub enum DrawOp {
    FillRect {
        rect: Rect,
        color: Color,
    },
    StrokeRect {
        rect: Rect,
        color: Color,
        width: f32,
    },
    DrawRaster {
        rect: Rect,
        raster: RasterAsset,
    },
    DrawText(TextRun),
}

#[derive(Clone, Debug)]
pub struct TextRun {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub style: TextStyle,
}

#[derive(Clone, Debug)]
pub struct TableData {
    pub title: Option<String>,
    pub rows: Vec<TableRow>,
}

#[derive(Clone, Debug)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

#[derive(Clone, Debug)]
pub struct TableCell {
    pub text: String,
    pub header: bool,
}

#[derive(Clone, Debug)]
pub struct ImageAsset {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub bits_per_component: u8,
    pub color_space: ImageColorSpace,
    pub encoding: ImageEncoding,
    pub bytes: Vec<u8>,
    pub jbig2_globals: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug)]
pub enum ImageColorSpace {
    Rgb8,
    Gray8,
}

#[derive(Clone, Debug)]
pub enum ImageEncoding {
    Jpeg { quality: u8 },
    Jpeg2000,
    FlateBitmap,
    Jbig2,
}

pub type EncodedImageAsset = ImageAsset;

#[derive(Clone, Debug)]
pub enum RasterAsset {
    Single(ImageAsset),
    Mrc(MrcAsset),
}

#[derive(Clone, Debug)]
pub struct MrcAsset {
    pub width: u32,
    pub height: u32,
    pub background: Option<ImageAsset>,
    pub foreground: Option<ImageAsset>,
    pub mask: Option<ImageAsset>,
    pub diagnostics: CompressionDiagnostics,
}

#[derive(Clone, Debug)]
pub struct SegmentedMrc {
    pub width: u32,
    pub height: u32,
    pub background_rgb: Vec<u8>,
    pub foreground_rgb_or_gray: Option<Vec<u8>>,
    pub foreground_color_space: ImageColorSpace,
    pub mask_1bpp: Option<Vec<u8>>,
    pub metadata: SegmentationMetadata,
}

#[derive(Clone, Debug, Default)]
pub struct SegmentationMetadata {
    pub source_alpha_used: bool,
    pub strategy: SegmentationStrategy,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub enum SegmentationStrategy {
    #[default]
    PlaceholderBackgroundOnly,
    AlphaMask,
    NaiveThreshold,
    DarkEdgeHybrid,
    ColorClusterMrc,
}

#[derive(Clone, Debug)]
pub struct CompressionDiagnostics {
    pub intent: RasterIntent,
    pub attempted_mode: RasterMode,
    pub actual_mode: RasterMode,
    pub fallback_used: bool,
    pub stages: Vec<CompressionStageRecord>,
    pub warnings: Vec<String>,
}

impl Default for CompressionDiagnostics {
    fn default() -> Self {
        Self {
            intent: RasterIntent::Block,
            attempted_mode: RasterMode::LegacyJpeg,
            actual_mode: RasterMode::LegacyJpeg,
            fallback_used: false,
            stages: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterIntent {
    Block,
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RasterMode {
    LegacyJpeg,
    Jpeg2000,
    Jbig2,
    Mrc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionStageStatus {
    Attempted,
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Clone, Debug)]
pub struct CompressionStageRecord {
    pub stage: &'static str,
    pub status: CompressionStageStatus,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RenderReport {
    pub page_count: usize,
    pub block_count: usize,
    pub compression_records: usize,
    pub fallback_records: usize,
    pub warning_count: usize,
    pub skipped_images: usize,
    pub stage_failures: Vec<ReportCount>,
    pub attempted_modes: Vec<ReportCount>,
    pub intents: Vec<ReportCount>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportCount {
    pub key: String,
    pub count: usize,
}
