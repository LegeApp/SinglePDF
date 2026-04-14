use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::Context;
use lopdf::{
    content::{Content, Operation},
    dictionary, Dictionary, Document, Object, ObjectId, Stream, StringFormat,
};
use subsetter::GlyphRemapper;
use ttf_parser::{Face, GlyphId};

use crate::{
    compression::{self, page_fallback_permitted},
    paginate::{estimate_table_column_widths, estimate_table_row_heights},
    model::{
        BlockKind, Color, CompressionDiagnostics, CompressionStageRecord, CompressionStageStatus,
        DrawOp, EncodedImageAsset, ImageColorSpace, ImageEncoding, LogicalPage, RasterAsset,
        RasterIntent, RasterMode, Rect, RenderPage, RenderReport, ReportCount, TableData,
        TableRole, TextRun, TextStyle,
    },
    profile::{CompressionPolicy, MrcMode, SinglePdfProfile},
    text_layout::{resolve_font_path, FontVariant, TextMeasurer},
};

pub struct PdfRenderer;

impl PdfRenderer {
    pub fn write_pages(path: impl AsRef<Path>, pages: &[LogicalPage]) -> anyhow::Result<()> {
        let bytes = Self::bytes_from_pages(pages)?;
        std::fs::write(path.as_ref(), bytes)
            .with_context(|| format!("failed to save PDF to {}", path.as_ref().display()))?;
        Ok(())
    }

    pub fn bytes_from_pages(pages: &[LogicalPage]) -> anyhow::Result<Vec<u8>> {
        let (bytes, _) = Self::bytes_and_report_from_pages(pages)?;
        Ok(bytes)
    }

    pub fn bytes_and_report_from_pages(
        pages: &[LogicalPage],
    ) -> anyhow::Result<(Vec<u8>, RenderReport)> {
        let measurer = TextMeasurer::new()?;
        let profile = SinglePdfProfile::default();
        let render_pages = pages
            .iter()
            .map(|page| Self::build_render_page(page, &measurer, &profile.compression))
            .collect::<anyhow::Result<Vec<_>>>()?;
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let fonts = EmbeddedFontSet::load(&mut doc, &render_pages)?;

        let mut kids = Vec::new();
        for page in &render_pages {
            let (operations, xobjects) = Self::ops_to_pdf(&mut doc, page, &fonts)?;
            let content = Content { operations };
            let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode()?));
            let mut resources = dictionary! {
                "Font" => fonts.resource_dictionary(),
            };
            if !xobjects.is_empty() {
                resources.set("XObject", Object::Dictionary(xobjects));
            }
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), page.width_pt.into(), page.height_pt.into()],
                "Contents" => content_id,
                "Resources" => resources
            });
            kids.push(page_id);
        }

        let kids_len = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids.into_iter().map(Object::Reference).collect::<Vec<_>>(),
                "Count" => kids_len,
            }),
        );

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.compress();
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes)
            .context("failed to serialize PDF document")?;
        let report = build_render_report(pages, &render_pages);
        Ok((bytes, report))
    }

    fn build_render_page(
        page: &LogicalPage,
        measurer: &TextMeasurer,
        compression_policy: &CompressionPolicy,
    ) -> anyhow::Result<RenderPage> {
        let mut render_page = Self::build_legacy_render_page(page, measurer, compression_policy)?;
        if matches!(compression_policy.page_mrc, MrcMode::Enabled) {
            match compression::compress_render_page_mrc(&render_page, compression_policy) {
                Ok(result) => {
                    let mut ops = Vec::with_capacity(render_page.ops.len() + 1);
                    ops.push(DrawOp::DrawRaster {
                        rect: Rect {
                            x: 0.0,
                            y: 0.0,
                            w: page.width_pt,
                            h: page.height_pt,
                        },
                        raster: result.raster,
                    });
                    ops.extend(
                        render_page
                            .ops
                            .iter()
                            .filter(|op| matches!(op, DrawOp::DrawText(_)))
                            .cloned(),
                    );
                    render_page.ops = ops;
                    render_page.compression_diagnostics.push(result.diagnostics);
                }
                Err(error) => {
                    render_page
                        .compression_diagnostics
                        .push(page_fallback_diagnostic(error.to_string()));
                    if !page_fallback_permitted(compression_policy) {
                        anyhow::bail!("page-level MRC failed and fail-fast fallback is configured");
                    }
                }
            }
        }
        Ok(render_page)
    }

    fn build_legacy_render_page(
        page: &LogicalPage,
        measurer: &TextMeasurer,
        compression_policy: &CompressionPolicy,
    ) -> anyhow::Result<RenderPage> {
        let mut ops = Vec::new();
        let mut compression_diagnostics = Vec::new();
        for positioned in &page.blocks {
            if let Some(background) = positioned.block.style.background {
                ops.push(DrawOp::FillRect {
                    rect: positioned.rect,
                    color: background,
                });
            }

            if matches!(positioned.block.kind, BlockKind::BlockQuote) {
                let rule_rect = Rect {
                    x: positioned.rect.x,
                    y: positioned.rect.y,
                    w: 2.5,
                    h: positioned.rect.h,
                };
                ops.push(DrawOp::FillRect {
                    rect: rule_rect,
                    color: positioned
                        .block
                        .style
                        .border
                        .map(|border| border.color)
                        .unwrap_or(Color {
                            r: 181,
                            g: 186,
                            b: 196,
                        }),
                });
            } else if let Some(border) = positioned.block.style.border {
                ops.push(DrawOp::StrokeRect {
                    rect: positioned.rect,
                    color: border.color,
                    width: border.width,
                });
            }

            match positioned.block.kind {
                BlockKind::Image | BlockKind::Figure => {
                    if let Some(image_rect) = positioned.image_rect {
                        match positioned.block.source.as_deref() {
                            Some(source) => match compression::compress_image_source(
                                source,
                                compression_policy,
                            ) {
                                Ok(result) => {
                                    compression_diagnostics.push(result.diagnostics);
                                    ops.push(DrawOp::DrawRaster {
                                        rect: image_rect,
                                        raster: result.raster,
                                    });
                                }
                                Err(error) => {
                                    compression_diagnostics
                                        .push(skipped_image_diagnostic(source, error.to_string()));
                                    push_image_placeholder(&mut ops, image_rect);
                                }
                            },
                            None => {
                                push_image_placeholder(&mut ops, image_rect);
                            }
                        }
                    }
                }
                BlockKind::Table | BlockKind::Aside if positioned.block.table.is_some() => {
                    let title_height = if positioned.block.title.is_some() {
                        positioned.lines.len() as f32 * positioned.block.style.text.line_height_pt + 4.0
                    } else {
                        0.0
                    };
                    if let Some(text_rect) = positioned.text_rect {
                        if title_height > 0.0 {
                            draw_text_lines(
                                &mut ops,
                                text_rect,
                                &positioned.lines,
                                &positioned.block.style.text,
                            );
                        }
                        let table_rect = Rect {
                            x: positioned.rect.x + positioned.block.style.padding.left,
                            y: positioned.rect.y + positioned.block.style.padding.top + title_height,
                            w: positioned.rect.w
                                - positioned.block.style.padding.left
                                - positioned.block.style.padding.right,
                            h: positioned.rect.h
                                - positioned.block.style.padding.top
                                - positioned.block.style.padding.bottom
                                - title_height,
                        };
                        render_table_ops(
                            &mut ops,
                            table_rect,
                            positioned.block.table.as_ref(),
                            positioned.block.table_role,
                            measurer,
                        );
                    }
                    continue;
                }
                _ => {}
            }

            if let Some(text_rect) = positioned.text_rect {
                draw_text_lines(
                    &mut ops,
                    text_rect,
                    &positioned.lines,
                    &positioned.block.style.text,
                );
            }
        }

        Ok(RenderPage {
            width_pt: page.width_pt,
            height_pt: page.height_pt,
            ops,
            compression_diagnostics,
        })
    }

    fn ops_to_pdf(
        doc: &mut Document,
        page: &RenderPage,
        fonts: &EmbeddedFontSet,
    ) -> anyhow::Result<(Vec<Operation>, Dictionary)> {
        let mut out = Vec::new();
        let mut xobjects = Dictionary::new();
        let mut image_index = 1usize;
        for op in &page.ops {
            match op {
                DrawOp::FillRect { rect, color } => {
                    out.push(Operation::new("q", vec![]));
                    out.push(Operation::new("rg", color_to_operands(*color)));
                    out.push(Operation::new(
                        "re",
                        vec![
                            rect.x.into(),
                            (page.height_pt - rect.y - rect.h).into(),
                            rect.w.into(),
                            rect.h.into(),
                        ],
                    ));
                    out.push(Operation::new("f", vec![]));
                    out.push(Operation::new("Q", vec![]));
                }
                DrawOp::StrokeRect { rect, color, width } => {
                    out.push(Operation::new("q", vec![]));
                    out.push(Operation::new("RG", color_to_operands(*color)));
                    out.push(Operation::new("w", vec![(*width).into()]));
                    out.push(Operation::new(
                        "re",
                        vec![
                            rect.x.into(),
                            (page.height_pt - rect.y - rect.h).into(),
                            rect.w.into(),
                            rect.h.into(),
                        ],
                    ));
                    out.push(Operation::new("S", vec![]));
                    out.push(Operation::new("Q", vec![]));
                }
                DrawOp::DrawRaster { rect, raster } => {
                    Self::append_raster_ops(
                        doc,
                        page,
                        rect,
                        raster,
                        &mut out,
                        &mut xobjects,
                        &mut image_index,
                    )?;
                }
                DrawOp::DrawText(text) => out.extend(text_to_ops(page.height_pt, text, fonts)),
            }
        }
        Ok((out, xobjects))
    }

    fn append_raster_ops(
        doc: &mut Document,
        page: &RenderPage,
        rect: &Rect,
        raster: &RasterAsset,
        out: &mut Vec<Operation>,
        xobjects: &mut Dictionary,
        image_index: &mut usize,
    ) -> anyhow::Result<()> {
        match raster {
            RasterAsset::Single(image) => {
                let image_name = next_image_name(image_index);
                let image_object_id = add_image_xobject(doc, image, None)?;
                xobjects.set(image_name.as_str(), Object::Reference(image_object_id));
                out.extend(image_to_ops(page.height_pt, rect, &image_name, image));
            }
            RasterAsset::Mrc(mrc) => {
                if let Some(background) = &mrc.background {
                    let name = next_image_name(image_index);
                    let object_id = add_image_xobject(doc, background, None)?;
                    xobjects.set(name.as_str(), Object::Reference(object_id));
                    out.extend(image_to_ops(page.height_pt, rect, &name, background));
                }
                if let Some(foreground) = &mrc.foreground {
                    let mask_id = if let Some(mask) = &mrc.mask {
                        Some(add_image_xobject(doc, mask, None)?)
                    } else {
                        None
                    };
                    let name = next_image_name(image_index);
                    let object_id = add_image_xobject(doc, foreground, mask_id)?;
                    xobjects.set(name.as_str(), Object::Reference(object_id));
                    out.extend(image_to_ops(page.height_pt, rect, &name, foreground));
                }
            }
        }
        Ok(())
    }
}

fn next_image_name(image_index: &mut usize) -> String {
    let name = format!("Im{}", *image_index);
    *image_index += 1;
    name
}

fn page_fallback_diagnostic(detail: String) -> CompressionDiagnostics {
    CompressionDiagnostics {
        intent: RasterIntent::Page,
        attempted_mode: RasterMode::Mrc,
        actual_mode: RasterMode::LegacyJpeg,
        fallback_used: true,
        stages: vec![CompressionStageRecord {
            stage: "page-mrc",
            status: CompressionStageStatus::Failed,
            detail: Some(detail),
        }],
        warnings: vec!["page-level MRC fell back to the legacy page pipeline".to_string()],
    }
}

fn skipped_image_diagnostic(source: &str, detail: String) -> CompressionDiagnostics {
    CompressionDiagnostics {
        intent: RasterIntent::Block,
        attempted_mode: RasterMode::LegacyJpeg,
        actual_mode: RasterMode::LegacyJpeg,
        fallback_used: true,
        stages: vec![CompressionStageRecord {
            stage: "block-image",
            status: CompressionStageStatus::Failed,
            detail: Some(detail),
        }],
        warnings: vec![format!(
            "skipped image source during PDF rendering: {}",
            truncate_for_diagnostic(source, 160)
        )],
    }
}

fn truncate_for_diagnostic(input: &str, max_len: usize) -> String {
    let mut chars = input.chars();
    let truncated = chars.by_ref().take(max_len).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn push_image_placeholder(ops: &mut Vec<DrawOp>, rect: Rect) {
    ops.push(DrawOp::FillRect {
        rect,
        color: Color {
            r: 245,
            g: 245,
            b: 245,
        },
    });
    ops.push(DrawOp::StrokeRect {
        rect,
        color: Color {
            r: 180,
            g: 180,
            b: 180,
        },
        width: 1.0,
    });
}

fn build_render_report(pages: &[LogicalPage], render_pages: &[RenderPage]) -> RenderReport {
    let mut stage_failures = BTreeMap::<String, usize>::new();
    let mut attempted_modes = BTreeMap::<String, usize>::new();
    let mut intents = BTreeMap::<String, usize>::new();
    let mut compression_records = 0usize;
    let mut fallback_records = 0usize;
    let mut warning_count = 0usize;
    let mut skipped_images = 0usize;

    for page in render_pages {
        for diag in &page.compression_diagnostics {
            compression_records += 1;
            if diag.fallback_used {
                fallback_records += 1;
            }
            warning_count += diag.warnings.len();
            *attempted_modes
                .entry(raster_mode_key(diag.attempted_mode).to_string())
                .or_insert(0) += 1;
            *intents
                .entry(raster_intent_key(diag.intent).to_string())
                .or_insert(0) += 1;

            for warning in &diag.warnings {
                if warning.contains("skipped image source") {
                    skipped_images += 1;
                }
            }

            for stage in &diag.stages {
                if matches!(stage.status, CompressionStageStatus::Failed) {
                    *stage_failures.entry(stage.stage.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    RenderReport {
        page_count: pages.len(),
        block_count: pages.iter().map(|page| page.blocks.len()).sum(),
        compression_records,
        fallback_records,
        warning_count,
        skipped_images,
        stage_failures: counts_to_vec(stage_failures),
        attempted_modes: counts_to_vec(attempted_modes),
        intents: counts_to_vec(intents),
    }
}

fn counts_to_vec(counts: BTreeMap<String, usize>) -> Vec<ReportCount> {
    counts
        .into_iter()
        .map(|(key, count)| ReportCount { key, count })
        .collect()
}

fn raster_mode_key(mode: RasterMode) -> &'static str {
    match mode {
        RasterMode::LegacyJpeg => "legacy_jpeg",
        RasterMode::Jpeg2000 => "jpeg2000",
        RasterMode::Jbig2 => "jbig2",
        RasterMode::Mrc => "mrc",
    }
}

fn raster_intent_key(intent: RasterIntent) -> &'static str {
    match intent {
        RasterIntent::Block => "block",
        RasterIntent::Page => "page",
    }
}

fn font_resource_name(variant: FontVariant) -> &'static str {
    match variant {
        FontVariant::Regular => "F1",
        FontVariant::Bold => "F2",
        FontVariant::Italic => "F3",
        FontVariant::BoldItalic => "F4",
    }
}

struct EmbeddedFontSet {
    regular: EmbeddedPdfFont,
    bold: EmbeddedPdfFont,
    italic: EmbeddedPdfFont,
    bold_italic: EmbeddedPdfFont,
}

impl EmbeddedFontSet {
    fn load(doc: &mut Document, pages: &[RenderPage]) -> anyhow::Result<Self> {
        let used_chars = collect_used_chars(pages);
        Ok(Self {
            regular: EmbeddedPdfFont::load(doc, FontVariant::Regular, &used_chars.regular)?,
            bold: EmbeddedPdfFont::load(doc, FontVariant::Bold, &used_chars.bold)?,
            italic: EmbeddedPdfFont::load(doc, FontVariant::Italic, &used_chars.italic)?,
            bold_italic: EmbeddedPdfFont::load(
                doc,
                FontVariant::BoldItalic,
                &used_chars.bold_italic,
            )?,
        })
    }

    fn resource_dictionary(&self) -> Dictionary {
        dictionary! {
            self.regular.resource_name => self.regular.object_id,
            self.bold.resource_name => self.bold.object_id,
            self.italic.resource_name => self.italic.object_id,
            self.bold_italic.resource_name => self.bold_italic.object_id,
        }
    }

    fn select(&self, style: &TextStyle) -> &EmbeddedPdfFont {
        match FontVariant::from_style(style) {
            FontVariant::Regular => &self.regular,
            FontVariant::Bold => &self.bold,
            FontVariant::Italic => &self.italic,
            FontVariant::BoldItalic => &self.bold_italic,
        }
    }
}

struct EmbeddedPdfFont {
    object_id: ObjectId,
    resource_name: &'static str,
    encoder: UnicodeFontEncoder,
}

impl EmbeddedPdfFont {
    fn load(
        doc: &mut Document,
        variant: FontVariant,
        used_chars: &BTreeSet<char>,
    ) -> anyhow::Result<Self> {
        let program = LoadedFontProgram::load(variant, used_chars)?;
        let object_id = embed_font_objects(doc, &program)?;
        Ok(Self {
            object_id,
            resource_name: font_resource_name(variant),
            encoder: UnicodeFontEncoder {
                char_to_cid: program.char_to_cid,
                fallback_cid: program.fallback_cid,
            },
        })
    }
}

struct UnicodeFontEncoder {
    char_to_cid: BTreeMap<char, u16>,
    fallback_cid: u16,
}

impl UnicodeFontEncoder {
    fn encode_text(&self, text: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() * 2);
        for ch in text.chars() {
            let cid = self.char_to_cid.get(&ch).copied().unwrap_or(self.fallback_cid);
            out.extend_from_slice(&cid.to_be_bytes());
        }
        out
    }
}

struct LoadedFontProgram {
    base_font_name: String,
    bytes: Vec<u8>,
    char_to_cid: BTreeMap<char, u16>,
    cid_to_unicode: BTreeMap<u16, String>,
    cid_widths: BTreeMap<u16, i64>,
    fallback_cid: u16,
    ascent: i64,
    descent: i64,
    font_bbox: [i64; 4],
    flags: i64,
    italic_angle: i64,
}

impl LoadedFontProgram {
    fn load(variant: FontVariant, used_chars: &BTreeSet<char>) -> anyhow::Result<Self> {
        let font_path = resolve_font_path(variant)?;
        let source_bytes = fs::read(&font_path)
            .with_context(|| format!("failed to read font {}", font_path.display()))?;
        let mut chars = used_chars.clone();
        chars.insert('?');
        let source_face = Face::parse(&source_bytes, 0)
            .with_context(|| format!("failed to parse font {}", font_path.display()))?;
        let source_units_per_em = i64::from(source_face.units_per_em());
        let fallback_gid = source_face.glyph_index('?').map(|gid| gid.0).unwrap_or(0);
        let (bytes, char_to_cid, fallback_cid) =
            subset_font_bytes(&source_bytes, &source_face, &chars, fallback_gid)
            .with_context(|| format!("failed to subset font {}", font_path.display()))?;
        let face = Face::parse(&bytes, 0)
            .with_context(|| format!("failed to parse subset font {}", font_path.display()))?;

        let mut cid_to_unicode = BTreeMap::new();
        let mut cid_widths = BTreeMap::new();
        for ch in chars {
            let cid = *char_to_cid.get(&ch).unwrap_or(&fallback_cid);
            let width = face
                .glyph_hor_advance(GlyphId(cid))
                .or_else(|| face.glyph_hor_advance(GlyphId(fallback_cid)))
                .unwrap_or(face.units_per_em());
            cid_widths
                .entry(cid)
                .or_insert(scale_metric(i64::from(width), source_units_per_em));
            if cid == fallback_cid && ch != '?' {
                cid_to_unicode.entry(cid).or_insert_with(|| "?".to_string());
            } else {
                cid_to_unicode.entry(cid).or_insert_with(|| ch.to_string());
            }
        }

        let bbox = face.global_bounding_box();
        let ascent = scale_metric(i64::from(face.ascender()), source_units_per_em);
        let descent = scale_metric(i64::from(face.descender()), source_units_per_em);
        let font_bbox = [
            scale_metric(i64::from(bbox.x_min), source_units_per_em),
            scale_metric(i64::from(bbox.y_min), source_units_per_em),
            scale_metric(i64::from(bbox.x_max), source_units_per_em),
            scale_metric(i64::from(bbox.y_max), source_units_per_em),
        ];
        let flags = if face.is_italic() { 32 | 64 } else { 32 };
        let italic_angle = if face.is_italic() { -12 } else { 0 };

        let base_font_name = sanitize_font_name(
            font_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("SinglePdfUnicodeFont"),
        );

        Ok(Self {
            base_font_name,
            bytes,
            char_to_cid,
            cid_to_unicode,
            cid_widths,
            fallback_cid,
            ascent,
            descent,
            font_bbox,
            flags,
            italic_angle,
        })
    }
}

fn subset_font_bytes(
    font_bytes: &[u8],
    face: &Face<'_>,
    retained_chars: &BTreeSet<char>,
    fallback_gid: u16,
) -> anyhow::Result<(Vec<u8>, BTreeMap<char, u16>, u16)> {
    let mut remapper = GlyphRemapper::new();
    let fallback_cid = remapper.remap(fallback_gid);
    let mut char_to_cid = BTreeMap::new();

    for ch in retained_chars {
        let gid = face
            .glyph_index(*ch)
            .map(|glyph| glyph.0)
            .unwrap_or(fallback_gid);
        let cid = remapper.remap(gid);
        char_to_cid.insert(*ch, cid);
    }

    let subset = subsetter::subset(font_bytes, 0, &remapper)
        .context("failed to build glyph subset for embedded font")?;
    Ok((subset, char_to_cid, fallback_cid))
}

struct UsedCharsByFont {
    regular: BTreeSet<char>,
    bold: BTreeSet<char>,
    italic: BTreeSet<char>,
    bold_italic: BTreeSet<char>,
}

fn collect_used_chars(pages: &[RenderPage]) -> UsedCharsByFont {
    let mut used = UsedCharsByFont {
        regular: BTreeSet::new(),
        bold: BTreeSet::new(),
        italic: BTreeSet::new(),
        bold_italic: BTreeSet::new(),
    };

    for page in pages {
        for op in &page.ops {
            let DrawOp::DrawText(text) = op else {
                continue;
            };
            let target = match FontVariant::from_style(&text.style) {
                FontVariant::Regular => &mut used.regular,
                FontVariant::Bold => &mut used.bold,
                FontVariant::Italic => &mut used.italic,
                FontVariant::BoldItalic => &mut used.bold_italic,
            };
            target.extend(text.text.chars());
        }
    }

    used
}

fn sanitize_font_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect::<String>();
    if sanitized.is_empty() {
        "SinglePdfUnicodeFont".to_string()
    } else {
        sanitized
    }
}

fn scale_metric(value: i64, units_per_em: i64) -> i64 {
    (value * 1000) / units_per_em.max(1)
}

fn embed_font_objects(doc: &mut Document, font: &LoadedFontProgram) -> anyhow::Result<ObjectId> {
    let font_stream_id = doc.add_object(Stream::new(
        dictionary! {
            "Length1" => font.bytes.len() as i64,
        },
        font.bytes.clone(),
    ));

    let descriptor_id = doc.add_object(dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => Object::Name(font.base_font_name.as_bytes().to_vec()),
        "Flags" => font.flags,
        "FontBBox" => vec![
            font.font_bbox[0].into(),
            font.font_bbox[1].into(),
            font.font_bbox[2].into(),
            font.font_bbox[3].into(),
        ],
        "ItalicAngle" => font.italic_angle,
        "Ascent" => font.ascent,
        "Descent" => font.descent,
        "CapHeight" => font.ascent,
        "StemV" => 80,
        "FontFile2" => font_stream_id,
    });

    let cid_system_info = Dictionary::from_iter(vec![
        (
            b"Registry".to_vec(),
            Object::String(b"Adobe".to_vec(), StringFormat::Literal),
        ),
        (
            b"Ordering".to_vec(),
            Object::String(b"Identity".to_vec(), StringFormat::Literal),
        ),
        (b"Supplement".to_vec(), 0.into()),
    ]);
    let cid_font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => Object::Name(font.base_font_name.as_bytes().to_vec()),
        "CIDSystemInfo" => Object::Dictionary(cid_system_info),
        "FontDescriptor" => descriptor_id,
        "DW" => 1000,
        "W" => build_width_array(&font.cid_widths),
        "CIDToGIDMap" => "Identity",
    });

    let to_unicode_id = doc.add_object(Stream::new(
        dictionary! {},
        build_to_unicode_cmap(&font.cid_to_unicode),
    ));

    Ok(doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => Object::Name(font.base_font_name.as_bytes().to_vec()),
        "Encoding" => "Identity-H",
        "DescendantFonts" => vec![Object::Reference(cid_font_id)],
        "ToUnicode" => to_unicode_id,
    }))
}

fn build_width_array(widths: &BTreeMap<u16, i64>) -> Object {
    let mut entries = Vec::with_capacity(widths.len() * 2);
    for (cid, width) in widths {
        entries.push(Object::Integer(i64::from(*cid)));
        entries.push(Object::Array(vec![Object::Integer(*width)]));
    }
    Object::Array(entries)
}

fn build_to_unicode_cmap(mapping: &BTreeMap<u16, String>) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("/CIDInit /ProcSet findresource begin\n");
    out.push_str("12 dict begin\n");
    out.push_str("begincmap\n");
    out.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
    out.push_str("/CMapName /Adobe-Identity-UCS def\n");
    out.push_str("/CMapType 2 def\n");
    out.push_str("1 begincodespacerange\n");
    out.push_str("<0000> <FFFF>\n");
    out.push_str("endcodespacerange\n");

    let entries = mapping.iter().collect::<Vec<_>>();
    for chunk in entries.chunks(100) {
        out.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (cid, text) in chunk {
            out.push_str(&format!(
                "<{:04X}> <{}>\n",
                cid,
                utf16_hex(text)
            ));
        }
        out.push_str("endbfchar\n");
    }

    out.push_str("endcmap\n");
    out.push_str("CMapName currentdict /CMap defineresource pop\n");
    out.push_str("end\n");
    out.push_str("end\n");
    out.into_bytes()
}

fn utf16_hex(text: &str) -> String {
    text.encode_utf16()
        .map(|unit| format!("{unit:04X}"))
        .collect::<String>()
}

fn build_image_xobject(
    image: &EncodedImageAsset,
    soft_mask: Option<ObjectId>,
    jbig2_globals: Option<ObjectId>,
) -> anyhow::Result<Stream> {
    let color_space = match image.color_space {
        ImageColorSpace::Rgb8 => "DeviceRGB",
        ImageColorSpace::Gray8 => "DeviceGray",
    };
    let mut dict = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Width" => image.pixel_width as i64,
        "Height" => image.pixel_height as i64,
        "ColorSpace" => color_space,
        "BitsPerComponent" => image.bits_per_component as i64,
    };
    if let Some(mask_id) = soft_mask {
        dict.set("SMask", mask_id);
    }
    match image.encoding {
        ImageEncoding::Jpeg { .. } => {
            dict.set("Filter", "DCTDecode");
        }
        ImageEncoding::Jpeg2000 => {
            dict.set("Filter", "JPXDecode");
        }
        ImageEncoding::FlateBitmap => {
            dict.set("Filter", "FlateDecode");
        }
        ImageEncoding::Jbig2 => {
            dict.set("Filter", "JBIG2Decode");
            if let Some(globals_id) = jbig2_globals {
                dict.set(
                    "DecodeParms",
                    Object::Dictionary(dictionary! {
                        "JBIG2Globals" => globals_id,
                    }),
                );
            }
        }
    }
    Ok(Stream::new(dict, image.bytes.clone()))
}

fn add_image_xobject(
    doc: &mut Document,
    image: &EncodedImageAsset,
    soft_mask: Option<ObjectId>,
) -> anyhow::Result<ObjectId> {
    let globals_id = image
        .jbig2_globals
        .as_ref()
        .map(|globals| doc.add_object(Stream::new(dictionary! {}, globals.clone())));
    Ok(doc.add_object(build_image_xobject(image, soft_mask, globals_id)?))
}

fn image_to_ops(
    page_height: f32,
    rect: &crate::model::Rect,
    image_name: &str,
    image: &EncodedImageAsset,
) -> Vec<Operation> {
    let fitted_rect = fit_image_rect(rect, image);
    vec![
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![
                fitted_rect.w.into(),
                0.into(),
                0.into(),
                fitted_rect.h.into(),
                fitted_rect.x.into(),
                (page_height - fitted_rect.y - fitted_rect.h).into(),
            ],
        ),
        Operation::new("Do", vec![Object::Name(image_name.as_bytes().to_vec())]),
        Operation::new("Q", vec![]),
    ]
}

fn text_to_ops(page_height: f32, text: &TextRun, fonts: &EmbeddedFontSet) -> Vec<Operation> {
    let font = fonts.select(&text.style);
    vec![
        Operation::new("BT", vec![]),
        Operation::new(
            "Tf",
            vec![
                Object::Name(font.resource_name.as_bytes().to_vec()),
                text.style.font_size_pt.into(),
            ],
        ),
        Operation::new("rg", color_to_operands(text.style.color)),
        Operation::new(
            "Tm",
            vec![
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                text.x.into(),
                (page_height - text.y).into(),
            ],
        ),
        Operation::new(
            "Tj",
            vec![Object::String(
                font.encoder.encode_text(&text.text),
                StringFormat::Hexadecimal,
            )],
        ),
        Operation::new("ET", vec![]),
    ]
}

fn color_to_operands(color: Color) -> Vec<Object> {
    vec![
        (color.r as f32 / 255.0).into(),
        (color.g as f32 / 255.0).into(),
        (color.b as f32 / 255.0).into(),
    ]
}

fn fit_image_rect(rect: &crate::model::Rect, image: &EncodedImageAsset) -> crate::model::Rect {
    let image_aspect = image.pixel_width as f32 / image.pixel_height.max(1) as f32;
    let rect_aspect = rect.w / rect.h.max(1.0);

    if image_aspect > rect_aspect {
        let fitted_height = rect.w / image_aspect;
        crate::model::Rect {
            x: rect.x,
            y: rect.y + (rect.h - fitted_height) * 0.5,
            w: rect.w,
            h: fitted_height,
        }
    } else {
        let fitted_width = rect.h * image_aspect;
        crate::model::Rect {
            x: rect.x + (rect.w - fitted_width) * 0.5,
            y: rect.y,
            w: fitted_width,
            h: rect.h,
        }
    }
}

fn draw_text_lines(ops: &mut Vec<DrawOp>, rect: Rect, lines: &[String], style: &TextStyle) {
    for (line_index, line) in lines.iter().enumerate() {
        let y = rect.y + (line_index as f32 * style.line_height_pt) + style.font_size_pt;
        ops.push(DrawOp::DrawText(TextRun {
            x: rect.x,
            y,
            text: line.clone(),
            style: style.clone(),
        }));
    }
}

fn render_table_ops(
    ops: &mut Vec<DrawOp>,
    rect: Rect,
    table: Option<&TableData>,
    table_role: Option<TableRole>,
    measurer: &TextMeasurer,
) {
    let Some(table) = table else {
        ops.push(DrawOp::FillRect {
            rect,
            color: Color {
                r: 245,
                g: 245,
                b: 245,
            },
        });
        ops.push(DrawOp::StrokeRect {
            rect,
            color: Color {
                r: 180,
                g: 180,
                b: 180,
            },
            width: 1.0,
        });
        return;
    };

    let column_widths = estimate_table_column_widths(table, rect.w, measurer);
    let row_heights = estimate_table_row_heights(table, &column_widths, measurer);
    let mut y = rect.y;

    for (row_index, row) in table.rows.iter().enumerate() {
        let row_height = *row_heights.get(row_index).unwrap_or(&24.0);
        let mut x = rect.x;
        for (cell_index, cell) in row.cells.iter().enumerate() {
            let column_width = *column_widths.get(cell_index).unwrap_or(&rect.w);
            let cell_rect = crate::model::Rect {
                x,
                y,
                w: column_width,
                h: row_height,
            };
            let header_fill = match table_role.unwrap_or(TableRole::Data) {
                TableRole::Infobox => Color {
                    r: 232,
                    g: 238,
                    b: 248,
                },
                TableRole::MessageBox => Color {
                    r: 255,
                    g: 241,
                    b: 204,
                },
                _ => Color {
                    r: 235,
                    g: 235,
                    b: 235,
                },
            };
            if cell.header {
                ops.push(DrawOp::FillRect {
                    rect: cell_rect,
                    color: header_fill,
                });
            }
            ops.push(DrawOp::StrokeRect {
                rect: cell_rect,
                color: Color {
                    r: 160,
                    g: 160,
                    b: 160,
                },
                width: 0.75,
            });

            let cell_style = crate::model::TextStyle {
                font_size_pt: 9.5,
                line_height_pt: 11.5,
                bold: cell.header,
                italic: false,
                color: Color {
                    r: 32,
                    g: 32,
                    b: 32,
                },
            };
            let cell_lines = measurer.wrap_text(&cell.text, (column_width - 12.0).max(32.0), &cell_style);
            for (line_index, line) in cell_lines.iter().enumerate() {
                ops.push(DrawOp::DrawText(TextRun {
                    x: x + 6.0,
                    y: y + 14.0 + line_index as f32 * 11.5,
                    text: line.clone(),
                    style: cell_style.clone(),
                }));
            }
            x += column_width;
        }
        y += row_height;
    }
}
