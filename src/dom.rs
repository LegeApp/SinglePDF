use std::{cmp::Ordering, collections::HashMap};

use scraper::{node::Node, ElementRef, Html, Selector};
use url::Url;

use crate::model::{
    BlockKind, BlockStyle, BorderStyle, Color, EdgeInsets, ElementLayoutHint, FlowBlock,
    FloatMode, ImageRole, LayoutHint, PaginationPolicy, ParsedDocument, SectionRole,
    SnapshotDocument, SnapshotSource, TableCell, TableData, TableRole, TableRow, TextAlignment,
    TextStyle,
};

const BLOCK_ID_ATTR: &str = "data-singlepdf-id";

#[derive(Clone, Debug)]
struct BackgroundRule {
    selector: SimpleSelector,
    source: String,
}

#[derive(Clone, Debug)]
enum SimpleSelector {
    Class(String),
    Id(String),
    Tag(String),
}

#[derive(Clone, Debug)]
struct ParseState {
    current_section: SectionRole,
}

pub struct DomProcessor;

impl DomProcessor {
    pub fn parse(snapshot: &SnapshotDocument) -> ParsedDocument {
        let document = Html::parse_document(&snapshot.html);
        let hint_index = build_hint_index(&snapshot.hints);
        let background_rules = collect_background_rules(&document, snapshot);
        let title = snapshot
            .metadata
            .title
            .clone()
            .or_else(|| {
                Selector::parse("title")
                    .ok()
                    .and_then(|selector| document.select(&selector).next())
                    .map(clean_element_text)
                    .filter(|title| !title.is_empty())
            });

        let mut blocks = Vec::new();
        let mut state = ParseState {
            current_section: SectionRole::Body,
        };
        if let Some(root) = find_content_root(&document, &hint_index) {
            for child in root.children() {
                if let Some(element) = ElementRef::wrap(child) {
                    collect_blocks(
                        element,
                        &mut blocks,
                        snapshot,
                        &background_rules,
                        &hint_index,
                        &mut state,
                        0,
                    );
                }
            }
        }

        if blocks.is_empty() {
            let fallback = clean_element_text(document.root_element());
            if !fallback.is_empty() {
                blocks.push(make_text_block(
                    BlockKind::Paragraph,
                    fallback,
                    document.root_element(),
                    SectionRole::Body,
                    None,
                ));
            }
        }

        ParsedDocument { title, blocks }
    }
}

fn build_hint_index(hints: &[ElementLayoutHint]) -> HashMap<String, LayoutHint> {
    hints.iter()
        .map(|hint| {
            (
                hint.id.clone(),
                LayoutHint {
                    block_id: hint.id.clone(),
                    rect: hint.rect,
                    display: hint.display.clone(),
                    rendered_width: hint.rendered_width,
                    rendered_height: hint.rendered_height,
                    float_mode: hint.float_mode.as_deref().and_then(parse_float_mode),
                    is_aside: hint.is_aside,
                    visible: hint.visible,
                    text_align: hint.text_align.as_deref().and_then(parse_alignment),
                    background_color: hint.background_color,
                    likely_main_content: hint.likely_main_content,
                    role_hint: hint.role_hint.clone(),
                },
            )
        })
        .collect()
}

fn find_content_root<'a>(
    document: &'a Html,
    hint_index: &HashMap<String, LayoutHint>,
) -> Option<ElementRef<'a>> {
    if hint_index.is_empty() {
        if let Ok(selector) = Selector::parse("body") {
            if let Some(root) = document.select(&selector).next() {
                return Some(root);
            }
        }
    }

    for selector in [
        "main .mw-parser-output",
        "article .mw-parser-output",
        ".mw-parser-output",
        "article",
        "main",
        "[role='main']",
        "body",
    ] {
        if let Ok(selector) = Selector::parse(selector) {
            if let Some(root) = document.select(&selector).next() {
                return Some(root);
            }
        }
    }

    if let Ok(selector) = Selector::parse(&format!("[{BLOCK_ID_ATTR}]")) {
        let best = document
            .select(&selector)
            .filter_map(|element| {
                let hint = element_layout_hint(element, hint_index)?;
                if !is_root_container_candidate(element, &hint) {
                    return None;
                }
                let score = content_root_score(element, &hint);
                Some((score, element))
            })
            .max_by(|(left, _), (right, _)| left.partial_cmp(right).unwrap());
        if let Some((score, element)) = best {
            if score > 4.5 {
                return Some(element);
            }
        }
    }

    None
}

fn content_root_score(element: ElementRef<'_>, hint: &LayoutHint) -> f32 {
    let mut score = if hint.likely_main_content { 4.0 } else { 0.0 };
    let text = clean_element_text(element);
    let text_len = text.chars().count() as f32;
    score += (text_len / 220.0).min(4.0);
    let area = hint.rect.width * hint.rect.height;
    score += (area / 180_000.0).min(3.0);
    if matches!(
        element.value().name(),
        "main" | "article" | "section" | "div" | "body"
    ) {
        score += 1.0;
    }
    score -= element_depth(element) as f32 * 0.2;
    if let Some(role_hint) = &hint.role_hint {
        let lower = role_hint.to_ascii_lowercase();
        if lower.contains("article") || lower.contains("content") {
            score += 1.5;
        }
        if lower.contains("nav") || lower.contains("sidebar") {
            score -= 2.5;
        }
    }
    score
}

fn is_root_container_candidate(element: ElementRef<'_>, hint: &LayoutHint) -> bool {
    if !matches!(
        element.value().name(),
        "main" | "article" | "section" | "div" | "body"
    ) {
        return false;
    }
    if !hint.visible || !hint.likely_main_content {
        return false;
    }
    if hint.rect.width * hint.rect.height < 120_000.0 {
        return false;
    }
    let child_count = element
        .children()
        .filter(|child| ElementRef::wrap(*child).is_some())
        .count();
    let text_len = clean_element_text(element).chars().count();
    child_count >= 3 || text_len >= 600
}

fn element_depth(element: ElementRef<'_>) -> usize {
    let mut depth = 0usize;
    let mut current = element.parent();
    while let Some(parent) = current {
        if ElementRef::wrap(parent).is_some() {
            depth += 1;
        }
        current = parent.parent();
    }
    depth
}

fn collect_blocks(
    element: ElementRef<'_>,
    blocks: &mut Vec<FlowBlock>,
    snapshot: &SnapshotDocument,
    background_rules: &[BackgroundRule],
    hint_index: &HashMap<String, LayoutHint>,
    state: &mut ParseState,
    depth: usize,
) {
    let tag = element.value().name();
    let layout_hint = element_layout_hint(element, hint_index);

    if should_skip_element(element, layout_hint.as_ref(), depth) {
        return;
    }

    for background_source in collect_background_sources(element, snapshot, background_rules) {
        if let Some(block) =
            make_background_block(element, background_source, layout_hint.clone(), state.current_section)
        {
            blocks.push(block);
        }
    }

    match tag {
        "video" | "audio" | "source" | "track" => {}
        "input" | "button" | "select" | "textarea" | "option" | "form" => {}
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = tag[1..].parse::<u8>().unwrap_or(1);
            let text = clean_element_text(element);
            if !text.is_empty() {
                state.current_section = classify_section_role(&text);
                blocks.push(make_heading_block(level, text, element, layout_hint));
            }
        }
        "p" => push_text_block(blocks, BlockKind::Paragraph, element, state.current_section, layout_hint),
        "li" => {
            let text = clean_element_text(element);
            if !text.is_empty() {
                let kind = if state.current_section == SectionRole::References {
                    BlockKind::ReferenceList
                } else {
                    BlockKind::ListItem
                };
                blocks.push(make_list_block(
                    kind,
                    text,
                    element,
                    state.current_section,
                    layout_hint,
                ));
            }
        }
        "pre" => {
            let text = collect_raw_preformatted_text(element);
            let text = text.trim().to_string();
            if !text.is_empty() {
                blocks.push(make_preformatted_block(
                    text,
                    element,
                    state.current_section,
                    layout_hint,
                ));
            }
        }
        "blockquote" => {
            let text = clean_element_text(element);
            if !text.is_empty() {
                blocks.push(make_blockquote_block(
                    text,
                    element,
                    state.current_section,
                    layout_hint,
                ));
            }
        }
        "figure" => {
            if let Some(block) = make_figure_block(element, snapshot, state.current_section, layout_hint) {
                blocks.push(block);
            }
        }
        "img" => {
            if let Some(block) = make_image_block(element, snapshot, state.current_section, layout_hint)
            {
                blocks.push(block);
            }
        }
        "table" => {
            if let Some(block) =
                make_table_block(element, state.current_section, layout_hint)
            {
                blocks.push(block);
            }
        }
        "article" | "section" | "main" | "div" | "aside" => {
            let mut emitted_child = false;
            for child in element.children() {
                if let Some(child) = ElementRef::wrap(child) {
                    let before = blocks.len();
                    collect_blocks(
                        child,
                        blocks,
                        snapshot,
                        background_rules,
                        hint_index,
                        state,
                        depth + 1,
                    );
                    emitted_child |= blocks.len() > before;
                }
            }
            if !emitted_child {
                let text = clean_element_text(element);
                if !text.is_empty() {
                    let kind = if state.current_section == SectionRole::References {
                        BlockKind::ReferenceList
                    } else if layout_hint.as_ref().is_some_and(|hint| hint.is_aside) {
                        BlockKind::Aside
                    } else {
                        BlockKind::Paragraph
                    };
                    blocks.push(make_text_block(
                        kind,
                        text,
                        element,
                        state.current_section,
                        layout_hint,
                    ));
                }
            }
        }
        _ => {
            for child in element.children() {
                if let Some(child) = ElementRef::wrap(child) {
                    collect_blocks(
                        child,
                        blocks,
                        snapshot,
                        background_rules,
                        hint_index,
                        state,
                        depth + 1,
                    );
                }
            }
        }
    }
}

fn element_layout_hint(
    element: ElementRef<'_>,
    hint_index: &HashMap<String, LayoutHint>,
) -> Option<LayoutHint> {
    let id = element.value().attr(BLOCK_ID_ATTR)?;
    hint_index.get(id).cloned()
}

fn collect_background_sources(
    element: ElementRef<'_>,
    snapshot: &SnapshotDocument,
    background_rules: &[BackgroundRule],
) -> Vec<String> {
    let mut sources = Vec::new();
    if let Some(source) = element
        .value()
        .attr("style")
        .and_then(extract_background_image_url)
        .and_then(|src| resolve_image_source(src, snapshot))
    {
        sources.push(source);
    }
    sources.extend(matching_background_sources(element, background_rules));
    sources.dedup();
    sources
}

fn make_background_block(
    element: ElementRef<'_>,
    background_source: String,
    layout_hint: Option<LayoutHint>,
    section_role: SectionRole,
) -> Option<FlowBlock> {
    let (estimated_height, style, role_hint_lower) = if let Some(hint) = layout_hint.as_ref() {
        if !hint.visible || !hint.likely_main_content {
            return None;
        }
        let area = hint.rect.width * hint.rect.height;
        if area < 40_000.0 {
            return None;
        }
        (
            hint.rendered_height.unwrap_or(180.0).clamp(96.0, 320.0),
            image_block_style(Some(hint)),
            hint.role_hint
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase(),
        )
    } else {
        let classes_and_id = element
            .value()
            .id()
            .into_iter()
            .chain(element.value().classes())
            .collect::<Vec<_>>()
            .join(" ")
            .to_ascii_lowercase();
        if ["icon", "logo", "sprite", "badge", "button", "thumb", "avatar"]
            .iter()
            .any(|needle| classes_and_id.contains(needle))
        {
            return None;
        }
        let estimated_height = estimate_background_height_without_hint(element)?;
        (estimated_height, image_block_style(None), classes_and_id)
    };
    if ["icon", "logo", "sprite", "badge", "button"]
        .iter()
        .any(|needle| role_hint_lower.contains(needle))
    {
        return None;
    }

    Some(FlowBlock {
        kind: BlockKind::Image,
        text: String::new(),
        title: Some(format!("Background image ({})", element.value().name())),
        caption: None,
        source: Some(background_source),
        table: None,
        estimated_height,
        style,
        spacing_before: 10.0,
        spacing_after: 10.0,
        pagination: PaginationPolicy::Atomic,
        layout_hint,
        table_role: None,
        image_role: Some(ImageRole::Decorative),
        section_role: Some(section_role),
    })
}

fn make_figure_block(
    element: ElementRef<'_>,
    snapshot: &SnapshotDocument,
    section_role: SectionRole,
    layout_hint: Option<LayoutHint>,
) -> Option<FlowBlock> {
    let img_selector = Selector::parse("img").expect("valid selector");
    let caption_selector = Selector::parse("figcaption").expect("valid selector");
    let (image, source) = element
        .select(&img_selector)
        .filter_map(|img| {
            let source = resolve_element_image_source(img, snapshot)?;
            let role = classify_image_role(img, layout_hint.as_ref());
            if matches!(role, ImageRole::Decorative | ImageRole::Icon) {
                return None;
            }
            let score = image_candidate_score(img, &source);
            Some((img, source, score))
        })
        .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(Ordering::Equal))
        .map(|(img, source, _)| (img, source))?;
    let caption = element
        .select(&caption_selector)
        .next()
        .map(clean_element_text)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            image.value()
                .attr("alt")
                .map(normalize_text)
                .filter(|text| is_meaningful_alt_text(text))
        });

    Some(FlowBlock {
        kind: BlockKind::Figure,
        text: String::new(),
        title: None,
        caption,
        source: Some(source),
        table: None,
        estimated_height: estimate_image_height(image, layout_hint.as_ref()),
        style: image_block_style(layout_hint.as_ref()),
        spacing_before: 10.0,
        spacing_after: 10.0,
        pagination: PaginationPolicy::Atomic,
        layout_hint,
        table_role: None,
        image_role: Some(ImageRole::Figure),
        section_role: Some(section_role),
    })
}

fn make_image_block(
    element: ElementRef<'_>,
    snapshot: &SnapshotDocument,
    section_role: SectionRole,
    layout_hint: Option<LayoutHint>,
) -> Option<FlowBlock> {
    let src = resolve_element_image_source(element, snapshot)?;
    let image_role = classify_image_role(element, layout_hint.as_ref());
    if matches!(image_role, ImageRole::Decorative | ImageRole::Icon) {
        return None;
    }
    let caption = element
        .value()
        .attr("alt")
        .map(normalize_text)
        .filter(|text| is_meaningful_alt_text(text));

    Some(FlowBlock {
        kind: if matches!(image_role, ImageRole::Figure) {
            BlockKind::Figure
        } else {
            BlockKind::Image
        },
        text: String::new(),
        title: None,
        caption,
        source: Some(src),
        table: None,
        estimated_height: estimate_image_height(element, layout_hint.as_ref()),
        style: image_block_style(layout_hint.as_ref()),
        spacing_before: 10.0,
        spacing_after: 10.0,
        pagination: PaginationPolicy::Atomic,
        layout_hint,
        table_role: None,
        image_role: Some(image_role),
        section_role: Some(section_role),
    })
}

fn make_table_block(
    element: ElementRef<'_>,
    section_role: SectionRole,
    layout_hint: Option<LayoutHint>,
) -> Option<FlowBlock> {
    let table = extract_table_data(element);
    if table.rows.is_empty() {
        return None;
    }

    let table_role = classify_table_role(element, &table)?;
    if matches!(table_role, TableRole::NavBox | TableRole::Metadata) {
        return None;
    }

    let kind = match table_role {
        TableRole::Infobox | TableRole::MessageBox => BlockKind::Aside,
        TableRole::Data => BlockKind::Table,
        TableRole::NavBox | TableRole::Metadata => return None,
    };

    let mut style = table_block_style(table_role, layout_hint.as_ref());
    let mut spacing_before = 10.0;
    let mut spacing_after = 10.0;
    apply_element_style_overrides(element, &mut style, &mut spacing_before, &mut spacing_after);

    Some(FlowBlock {
        kind,
        text: String::new(),
        title: table.title.clone(),
        caption: None,
        source: None,
        table: Some(table),
        estimated_height: 0.0,
        style,
        spacing_before,
        spacing_after,
        pagination: if matches!(table_role, TableRole::Data) {
            PaginationPolicy::SplitTable
        } else {
            PaginationPolicy::Atomic
        },
        layout_hint,
        table_role: Some(table_role),
        image_role: None,
        section_role: Some(section_role),
    })
}

fn classify_table_role(element: ElementRef<'_>, table: &TableData) -> Option<TableRole> {
    let classes = element
        .value()
        .classes()
        .map(|class| class.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let id = element
        .value()
        .id()
        .map(|id| id.to_ascii_lowercase())
        .unwrap_or_default();

    if classes.iter().any(|class| class.contains("navbox")) || id.contains("navbox") {
        return Some(TableRole::NavBox);
    }
    if classes.iter().any(|class| class.contains("metadata")) || id.contains("metadata") {
        return Some(TableRole::Metadata);
    }
    if classes.iter().any(|class| {
        ["ambox", "tmbox", "cmbox", "ombox", "messagebox", "warning"]
            .iter()
            .any(|needle| class.contains(needle))
    }) {
        return Some(TableRole::MessageBox);
    }
    if classes.iter().any(|class| class.contains("infobox")) {
        return Some(TableRole::Infobox);
    }

    let row_count = table.rows.len();
    let max_columns = table
        .rows
        .iter()
        .map(|row| row.cells.len())
        .max()
        .unwrap_or(0);
    let first_column_short_ratio = if row_count == 0 {
        0.0
    } else {
        table
            .rows
            .iter()
            .filter(|row| {
                row.cells
                    .first()
                    .map(|cell| cell.text.chars().count() <= 24)
                    .unwrap_or(false)
            })
            .count() as f32
            / row_count as f32
    };
    let header_like_ratio = if row_count == 0 {
        0.0
    } else {
        table
            .rows
            .iter()
            .filter(|row| row.cells.first().is_some_and(|cell| cell.header))
            .count() as f32
            / row_count as f32
    };

    if max_columns <= 2 && row_count >= 4 && (first_column_short_ratio > 0.55 || header_like_ratio > 0.4)
    {
        return Some(TableRole::Infobox);
    }

    Some(TableRole::Data)
}

fn push_text_block(
    blocks: &mut Vec<FlowBlock>,
    default_kind: BlockKind,
    element: ElementRef<'_>,
    section_role: SectionRole,
    layout_hint: Option<LayoutHint>,
) {
    let text = clean_element_text(element);
    if !text.is_empty() {
        let kind = if section_role == SectionRole::References {
            BlockKind::ReferenceList
        } else {
            default_kind
        };
        blocks.push(make_text_block(kind, text, element, section_role, layout_hint));
    }
}

fn make_heading_block(
    level: u8,
    text: String,
    element: ElementRef<'_>,
    layout_hint: Option<LayoutHint>,
) -> FlowBlock {
    let size = match level {
        1 => 24.0,
        2 => 19.0,
        3 => 16.0,
        4 => 14.0,
        _ => 12.5,
    };
    let mut style = base_block_style(size, true, false);
    style.padding = EdgeInsets {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };
    let mut spacing_before = if level == 1 { 8.0 } else { 12.0 };
    let mut spacing_after = 6.0;
    apply_element_style_overrides(element, &mut style, &mut spacing_before, &mut spacing_after);

    FlowBlock {
        kind: BlockKind::Heading { level },
        text,
        title: None,
        caption: None,
        source: None,
        table: None,
        estimated_height: 0.0,
        style,
        spacing_before,
        spacing_after,
        pagination: PaginationPolicy::KeepWithNext,
        layout_hint,
        table_role: None,
        image_role: None,
        section_role: Some(SectionRole::Body),
    }
}

fn make_text_block(
    kind: BlockKind,
    text: String,
    element: ElementRef<'_>,
    section_role: SectionRole,
    layout_hint: Option<LayoutHint>,
) -> FlowBlock {
    let mut style = match kind {
        BlockKind::ReferenceList => reference_block_style(),
        BlockKind::Aside => aside_text_style(layout_hint.as_ref()),
        BlockKind::BlockQuote => blockquote_style(),
        _ => base_block_style(11.5, false, false),
    };
    let mut spacing_before = 4.0;
    let mut spacing_after = 4.0;
    apply_element_style_overrides(element, &mut style, &mut spacing_before, &mut spacing_after);

    FlowBlock {
        kind,
        text,
        title: None,
        caption: None,
        source: None,
        table: None,
        estimated_height: 0.0,
        style,
        spacing_before,
        spacing_after,
        pagination: PaginationPolicy::SplitParagraph,
        layout_hint,
        table_role: None,
        image_role: None,
        section_role: Some(section_role),
    }
}

fn make_list_block(
    kind: BlockKind,
    text: String,
    element: ElementRef<'_>,
    section_role: SectionRole,
    layout_hint: Option<LayoutHint>,
) -> FlowBlock {
    let mut style = if kind == BlockKind::ReferenceList {
        reference_block_style()
    } else {
        let mut style = base_block_style(11.0, false, false);
        style.indent_pt = 14.0;
        style
    };
    let mut spacing_before = 2.0;
    let mut spacing_after = 2.0;
    apply_element_style_overrides(element, &mut style, &mut spacing_before, &mut spacing_after);

    FlowBlock {
        kind,
        text: format!("• {}", text),
        title: None,
        caption: None,
        source: None,
        table: None,
        estimated_height: 0.0,
        style,
        spacing_before,
        spacing_after,
        pagination: PaginationPolicy::SplitParagraph,
        layout_hint,
        table_role: None,
        image_role: None,
        section_role: Some(section_role),
    }
}

fn make_preformatted_block(
    text: String,
    element: ElementRef<'_>,
    section_role: SectionRole,
    layout_hint: Option<LayoutHint>,
) -> FlowBlock {
    let mut style = base_block_style(9.5, false, false);
    style.padding = EdgeInsets {
        top: 8.0,
        right: 10.0,
        bottom: 8.0,
        left: 10.0,
    };
    style.background = Some(Color {
        r: 246,
        g: 246,
        b: 246,
    });
    style.border = Some(BorderStyle {
        color: Color {
            r: 214,
            g: 214,
            b: 214,
        },
        width: 0.75,
    });
    let mut spacing_before = 8.0;
    let mut spacing_after = 8.0;
    apply_element_style_overrides(element, &mut style, &mut spacing_before, &mut spacing_after);

    FlowBlock {
        kind: BlockKind::Preformatted,
        text,
        title: None,
        caption: None,
        source: None,
        table: None,
        estimated_height: 0.0,
        style,
        spacing_before,
        spacing_after,
        pagination: PaginationPolicy::SplitPreformatted,
        layout_hint,
        table_role: None,
        image_role: None,
        section_role: Some(section_role),
    }
}

fn make_blockquote_block(
    text: String,
    element: ElementRef<'_>,
    section_role: SectionRole,
    layout_hint: Option<LayoutHint>,
) -> FlowBlock {
    let mut style = blockquote_style();
    let mut spacing_before = 6.0;
    let mut spacing_after = 6.0;
    apply_element_style_overrides(element, &mut style, &mut spacing_before, &mut spacing_after);

    FlowBlock {
        kind: BlockKind::BlockQuote,
        text,
        title: None,
        caption: None,
        source: None,
        table: None,
        estimated_height: 0.0,
        style,
        spacing_before,
        spacing_after,
        pagination: PaginationPolicy::SplitParagraph,
        layout_hint,
        table_role: None,
        image_role: None,
        section_role: Some(section_role),
    }
}

fn image_block_style(layout_hint: Option<&LayoutHint>) -> BlockStyle {
    let mut style = base_block_style(10.0, false, false);
    style.padding = EdgeInsets {
        top: 6.0,
        right: 6.0,
        bottom: 6.0,
        left: 6.0,
    };
    style.border = Some(BorderStyle {
        color: Color {
            r: 222,
            g: 222,
            b: 222,
        },
        width: 0.75,
    });
    style.background = layout_hint.and_then(|hint| hint.background_color).or(Some(Color {
        r: 250,
        g: 250,
        b: 250,
    }));
    style.alignment = TextAlignment::Center;
    style
}

fn table_block_style(table_role: TableRole, layout_hint: Option<&LayoutHint>) -> BlockStyle {
    let mut style = base_block_style(10.5, false, false);
    style.padding = EdgeInsets {
        top: 8.0,
        right: 8.0,
        bottom: 8.0,
        left: 8.0,
    };
    style.border = Some(BorderStyle {
        color: Color {
            r: 190,
            g: 196,
            b: 205,
        },
        width: 0.9,
    });
    style.background = Some(match table_role {
        TableRole::Infobox => layout_hint
            .and_then(|hint| hint.background_color)
            .unwrap_or(Color {
                r: 246,
                g: 248,
                b: 252,
            }),
        TableRole::MessageBox => Color {
            r: 255,
            g: 247,
            b: 225,
        },
        TableRole::Data => Color {
            r: 255,
            g: 255,
            b: 255,
        },
        TableRole::NavBox | TableRole::Metadata => Color {
            r: 255,
            g: 255,
            b: 255,
        },
    });
    style
}

fn aside_text_style(layout_hint: Option<&LayoutHint>) -> BlockStyle {
    let mut style = base_block_style(10.5, false, false);
    style.padding = EdgeInsets {
        top: 8.0,
        right: 10.0,
        bottom: 8.0,
        left: 10.0,
    };
    style.border = Some(BorderStyle {
        color: Color {
            r: 200,
            g: 205,
            b: 214,
        },
        width: 0.75,
    });
    style.background = layout_hint.and_then(|hint| hint.background_color).or(Some(Color {
        r: 248,
        g: 249,
        b: 251,
    }));
    style
}

fn reference_block_style() -> BlockStyle {
    let mut style = base_block_style(10.0, false, false);
    style.text.line_height_pt = 13.0;
    style
}

fn blockquote_style() -> BlockStyle {
    let mut style = base_block_style(11.5, false, true);
    style.padding = EdgeInsets {
        top: 4.0,
        right: 0.0,
        bottom: 4.0,
        left: 12.0,
    };
    style.border = Some(BorderStyle {
        color: Color {
            r: 181,
            g: 186,
            b: 196,
        },
        width: 2.0,
    });
    style
}

fn base_block_style(font_size_pt: f32, bold: bool, italic: bool) -> BlockStyle {
    BlockStyle {
        text: TextStyle {
            font_size_pt,
            line_height_pt: font_size_pt * 1.35,
            bold,
            italic,
            color: Color {
                r: 32,
                g: 32,
                b: 32,
            },
        },
        padding: EdgeInsets {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        },
        border: None,
        background: None,
        indent_pt: 0.0,
        alignment: TextAlignment::Start,
    }
}

fn apply_element_style_overrides(
    element: ElementRef<'_>,
    style: &mut BlockStyle,
    spacing_before: &mut f32,
    spacing_after: &mut f32,
) {
    let declarations = parse_style_declarations(element.value().attr("style"));
    if declarations.is_empty() {
        return;
    }

    let base_font = style.text.font_size_pt.max(1.0);
    if let Some(font_size) = declarations
        .get("font-size")
        .and_then(|value| parse_css_length_pt(value, base_font))
    {
        style.text.font_size_pt = font_size.clamp(7.0, 30.0);
        style.text.line_height_pt = style.text.line_height_pt.max(style.text.font_size_pt * 1.2);
    }

    if let Some(line_height) = declarations
        .get("line-height")
        .and_then(|value| parse_css_line_height_pt(value, style.text.font_size_pt))
    {
        style.text.line_height_pt = line_height.clamp(style.text.font_size_pt * 1.05, 48.0);
    }

    if let Some(text_align) = declarations
        .get("text-align")
        .and_then(|value| parse_alignment(value))
    {
        style.alignment = text_align;
    }

    if let Some(color) = declarations.get("color").and_then(|value| parse_css_color(value)) {
        style.text.color = color;
    }

    if let Some(weight) = declarations.get("font-weight") {
        let lower = weight.to_ascii_lowercase();
        if lower.contains("bold")
            || lower.contains("semi")
            || lower.contains("demi")
            || lower.parse::<u16>().is_ok_and(|w| w >= 600)
        {
            style.text.bold = true;
        }
    }

    if let Some(font_style) = declarations.get("font-style") {
        if font_style.to_ascii_lowercase().contains("italic") {
            style.text.italic = true;
        }
    }

    if let Some(margin_top) = declarations
        .get("margin-top")
        .and_then(|value| parse_css_length_pt(value, style.text.font_size_pt))
    {
        *spacing_before = margin_top.clamp(0.0, 20.0);
    }
    if let Some(margin_bottom) = declarations
        .get("margin-bottom")
        .and_then(|value| parse_css_length_pt(value, style.text.font_size_pt))
    {
        *spacing_after = margin_bottom.clamp(0.0, 20.0);
    }

    if let Some(padding_left) = declarations
        .get("padding-left")
        .and_then(|value| parse_css_length_pt(value, style.text.font_size_pt))
    {
        style.indent_pt = style.indent_pt.max(padding_left.clamp(0.0, 36.0));
    }
}

fn parse_style_declarations(style_attr: Option<&str>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(style) = style_attr else {
        return out;
    };

    for declaration in style.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let key = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if !key.is_empty() && !value.is_empty() {
            out.insert(key, value.to_string());
        }
    }
    out
}

fn parse_css_line_height_pt(value: &str, font_size_pt: f32) -> Option<f32> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("normal") {
        return Some(font_size_pt * 1.35);
    }
    if let Ok(multiplier) = trimmed.parse::<f32>() {
        return Some((font_size_pt * multiplier).max(font_size_pt));
    }
    parse_css_length_pt(trimmed, font_size_pt)
}

fn parse_css_length_pt(value: &str, base_font_pt: f32) -> Option<f32> {
    let trimmed = value.trim().to_ascii_lowercase();
    let parse_number = |input: &str| input.trim().parse::<f32>().ok();

    if let Some(number) = trimmed.strip_suffix("px").and_then(parse_number) {
        return Some(number * 0.75);
    }
    if let Some(number) = trimmed.strip_suffix("pt").and_then(parse_number) {
        return Some(number);
    }
    if let Some(number) = trimmed.strip_suffix("rem").and_then(parse_number) {
        return Some(number * 12.0);
    }
    if let Some(number) = trimmed.strip_suffix("em").and_then(parse_number) {
        return Some(number * base_font_pt);
    }
    if let Some(number) = trimmed.strip_suffix('%').and_then(parse_number) {
        return Some((number / 100.0) * base_font_pt);
    }

    parse_number(&trimmed).map(|number| number * 0.75)
}

fn parse_css_color(value: &str) -> Option<Color> {
    let lower = value.trim().to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                Some(Color { r, g, b })
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color { r, g, b })
            }
            _ => None,
        };
    }

    if let Some(payload) = lower
        .strip_prefix("rgb(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let mut parts = payload.split(',').map(str::trim);
        let r = parts.next()?.parse::<u8>().ok()?;
        let g = parts.next()?.parse::<u8>().ok()?;
        let b = parts.next()?.parse::<u8>().ok()?;
        return Some(Color { r, g, b });
    }

    None
}

fn estimate_background_height_without_hint(element: ElementRef<'_>) -> Option<f32> {
    let declarations = parse_style_declarations(element.value().attr("style"));
    let raw_height = declarations
        .get("height")
        .or_else(|| declarations.get("min-height"))
        .and_then(|value| parse_dimension_attr(value))
        .filter(|height| *height > 0.0);
    let raw_width = declarations
        .get("width")
        .or_else(|| declarations.get("min-width"))
        .and_then(|value| parse_dimension_attr(value))
        .filter(|width| *width > 0.0);

    match (raw_width, raw_height) {
        (Some(width), Some(height)) => {
            if width * height < 8_000.0 {
                return None;
            }
            let fitted_width = width.clamp(140.0, 440.0);
            Some((height * (fitted_width / width)).clamp(84.0, 360.0))
        }
        (_, Some(height)) if height >= 64.0 => Some(height.clamp(84.0, 320.0)),
        _ => {
            let text_len = clean_element_text(element).chars().count();
            if text_len >= 120 {
                Some(150.0)
            } else {
                None
            }
        }
    }
}

fn should_skip_element(
    element: ElementRef<'_>,
    layout_hint: Option<&LayoutHint>,
    depth: usize,
) -> bool {
    let tag = element.value().name();
    if matches!(
        tag,
        "script" | "style" | "noscript" | "template" | "svg" | "canvas"
    ) {
        return true;
    }

    if element
        .value()
        .attr("aria-hidden")
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    {
        return true;
    }

    let score = content_score(element, layout_hint, depth);
    let threshold = if layout_hint.is_some() { -0.5 } else { -1.75 };
    score < threshold
}

fn content_score(element: ElementRef<'_>, layout_hint: Option<&LayoutHint>, depth: usize) -> f32 {
    let tag = element.value().name();
    let text = clean_element_text(element);
    let text_len = text.chars().count() as f32;
    let link_selector = Selector::parse("a").expect("valid selector");
    let link_text_len = element
        .select(&link_selector)
        .map(clean_element_text)
        .map(|text| text.chars().count() as f32)
        .sum::<f32>();
    let link_density = if text_len > 0.0 {
        link_text_len / text_len
    } else {
        0.0
    };

    let mut score = 0.0;
    if matches!(
        tag,
        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre" | "img" | "table" | "figure"
    ) {
        score += 2.0;
    }
    if text_len > 180.0 {
        score += 2.0;
    } else if text_len > 60.0 {
        score += 1.0;
    } else if text_len < 8.0 && !matches!(tag, "img" | "figure") {
        score -= 0.5;
    }
    if link_density > 0.65 {
        score -= 2.5;
    } else if link_density > 0.35 {
        score -= 1.0;
    }
    if depth <= 2 {
        score += 0.25;
    }

    let mut attr_penalty = element
        .value()
        .id()
        .into_iter()
        .chain(element.value().classes())
        .fold(0.0, |acc, value| acc + attribute_signal(value));
    if layout_hint.is_none() {
        attr_penalty *= 0.35;
    }
    score += attr_penalty;

    if let Some(hint) = layout_hint {
        if hint.likely_main_content {
            score += 2.5;
        }
        if !hint.visible {
            score -= 3.0;
        }
        if hint.is_aside {
            score += 0.5;
        }
        if let Some(role_hint) = &hint.role_hint {
            score += attribute_signal(role_hint);
        }
    }

    if element.value().attr("role").is_some_and(|role| {
        matches!(
            role,
            "navigation" | "search" | "banner" | "contentinfo" | "menu" | "complementary"
        )
    }) {
        score -= if layout_hint.is_some() { 3.0 } else { 0.75 };
    }

    score
}

fn attribute_signal(value: &str) -> f32 {
    let lower = value.to_ascii_lowercase();
    if [
        "nav",
        "navigation",
        "sidebar",
        "menu",
        "toolbar",
        "breadcrumb",
        "toc",
        "page-tools",
        "search",
        "share",
        "related",
        "promo",
        "advert",
        "subscribe",
        "portal",
        "footer",
        "header",
    ]
    .iter()
    .any(|keyword| lower.contains(keyword))
    {
        return -2.5;
    }
    if ["article", "content", "main", "infobox", "reference", "references", "thumb"]
        .iter()
        .any(|keyword| lower.contains(keyword))
    {
        return 1.0;
    }
    if ["ambox", "tmbox", "messagebox", "warning"]
        .iter()
        .any(|keyword| lower.contains(keyword))
    {
        return 0.75;
    }
    0.0
}

fn classify_image_role(element: ElementRef<'_>, layout_hint: Option<&LayoutHint>) -> ImageRole {
    let classes_and_ids = element
        .value()
        .id()
        .into_iter()
        .chain(element.value().classes())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let alt_text = element
        .value()
        .attr("alt")
        .map(normalize_text)
        .unwrap_or_default()
        .to_ascii_lowercase();

    if ["icon", "sprite", "logo", "badge"]
        .iter()
        .any(|needle| classes_and_ids.contains(needle) || alt_text.contains(needle))
    {
        return ImageRole::Icon;
    }
    if ["avatar", "profile"]
        .iter()
        .any(|needle| classes_and_ids.contains(needle))
    {
        return ImageRole::Avatar;
    }
    if layout_hint
        .as_ref()
        .and_then(|hint| hint.role_hint.as_ref())
        .is_some_and(|hint| hint.to_ascii_lowercase().contains("decorative"))
    {
        return ImageRole::Decorative;
    }
    if element
        .parent()
        .and_then(ElementRef::wrap)
        .is_some_and(|parent| parent.value().name() == "figure")
    {
        return ImageRole::Figure;
    }
    ImageRole::Content
}

fn collect_background_rules(document: &Html, snapshot: &SnapshotDocument) -> Vec<BackgroundRule> {
    let style_selector = Selector::parse("style").expect("valid selector");
    let mut rules = Vec::new();
    for node in document.select(&style_selector) {
        let css = node.text().collect::<Vec<_>>().join(" ");
        rules.extend(parse_background_rules(&css, snapshot));
    }
    rules
}

fn matching_background_sources(
    element: ElementRef<'_>,
    background_rules: &[BackgroundRule],
) -> Vec<String> {
    background_rules
        .iter()
        .filter(|rule| selector_matches(element, &rule.selector))
        .map(|rule| rule.source.clone())
        .collect()
}

fn selector_matches(element: ElementRef<'_>, selector: &SimpleSelector) -> bool {
    match selector {
        SimpleSelector::Class(class_name) => element
            .value()
            .classes()
            .any(|candidate| candidate.eq_ignore_ascii_case(class_name)),
        SimpleSelector::Id(id) => element
            .value()
            .id()
            .map(|candidate| candidate.eq_ignore_ascii_case(id))
            .unwrap_or(false),
        SimpleSelector::Tag(tag) => element.value().name().eq_ignore_ascii_case(tag),
    }
}

fn parse_background_rules(css: &str, snapshot: &SnapshotDocument) -> Vec<BackgroundRule> {
    let mut rules = Vec::new();
    for chunk in css.split('}') {
        let Some((selectors, declarations)) = chunk.split_once('{') else {
            continue;
        };
        let Some(url) = extract_background_image_url(declarations) else {
            continue;
        };
        let Some(resolved) = resolve_image_source(url, snapshot) else {
            continue;
        };
        for selector in selectors.split(',') {
            if let Some(selector) = parse_simple_selector(selector.trim()) {
                rules.push(BackgroundRule {
                    selector,
                    source: resolved.clone(),
                });
            }
        }
    }
    rules
}

fn parse_simple_selector(selector: &str) -> Option<SimpleSelector> {
    if selector.is_empty()
        || selector.contains(' ')
        || selector.contains('>')
        || selector.contains(':')
    {
        return None;
    }
    if let Some(class_name) = selector.strip_prefix('.') {
        return (!class_name.is_empty()).then(|| SimpleSelector::Class(class_name.to_string()));
    }
    if let Some(id) = selector.strip_prefix('#') {
        return (!id.is_empty()).then(|| SimpleSelector::Id(id.to_string()));
    }
    if selector
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Some(SimpleSelector::Tag(selector.to_string()));
    }
    None
}

fn extract_background_image_url(style: &str) -> Option<&str> {
    let lower = style.to_ascii_lowercase();
    let start = lower.find("background-image")?;
    let slice = &style[start..];
    let url_start = slice.find("url(")? + 4;
    let after_url = &slice[url_start..];
    let url_end = after_url.find(')')?;
    let raw = after_url[..url_end].trim();
    let trimmed = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(raw)
        .trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(trimmed)
    }
}

fn resolve_image_source(src: &str, snapshot: &SnapshotDocument) -> Option<String> {
    if src.is_empty() {
        return None;
    }
    if src.starts_with("data:") || src.starts_with("file://") {
        return Some(src.to_string());
    }
    if src.starts_with("http://") || src.starts_with("https://") {
        return Some(src.to_string());
    }

    match &snapshot.source {
        SnapshotSource::File(path) => {
            let Some(parent) = path.parent() else {
                return Some(src.to_string());
            };
            let resolved = parent.join(src);
            Some(resolved.to_string_lossy().to_string())
        }
        _ => resolve_against_snapshot_url(src, snapshot).or_else(|| Some(src.to_string())),
    }
}

fn resolve_element_image_source(element: ElementRef<'_>, snapshot: &SnapshotDocument) -> Option<String> {
    for candidate in image_source_candidates(element) {
        if let Some(resolved) = resolve_image_source(candidate, snapshot) {
            return Some(resolved);
        }
    }
    None
}

fn image_source_candidates<'a>(element: ElementRef<'a>) -> Vec<&'a str> {
    let mut candidates = Vec::new();
    let attrs = [
        "src",
        "srcset",
        "data-src",
        "data-srcset",
        "data-original",
        "data-image-src",
        "data-lazy-src",
        "data-url",
    ];

    for attr in attrs {
        let Some(value) = element.value().attr(attr).map(str::trim) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }

        if attr.ends_with("srcset") {
            if let Some(parsed) = parse_srcset_preferred_source(value) {
                candidates.push(parsed);
            }
        } else {
            candidates.push(value);
        }
    }

    candidates
}

fn parse_srcset_preferred_source(srcset: &str) -> Option<&str> {
    srcset
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.split_whitespace().next())
        .find(|src| !src.is_empty())
}

fn image_candidate_score(element: ElementRef<'_>, source: &str) -> f32 {
    let width = element
        .value()
        .attr("width")
        .and_then(parse_dimension_attr)
        .unwrap_or(0.0);
    let height = element
        .value()
        .attr("height")
        .and_then(parse_dimension_attr)
        .unwrap_or(0.0);
    let mut score = width * height;

    let alt = element
        .value()
        .attr("alt")
        .map(normalize_text)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if is_meaningful_alt_text(&alt) {
        score += 10_000.0;
    }
    if ["icon", "badge", "logo", "sprite"]
        .iter()
        .any(|needle| alt.contains(needle))
    {
        score -= 1_000_000.0;
    }

    let classes = element
        .value()
        .classes()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    if ["icon", "badge", "logo", "sprite"]
        .iter()
        .any(|needle| classes.contains(needle))
    {
        score -= 1_000_000.0;
    }
    if source.starts_with("data:image/svg+xml") && width <= 24.0 && height <= 24.0 {
        score -= 500_000.0;
    }

    score
}

fn resolve_against_snapshot_url(src: &str, snapshot: &SnapshotDocument) -> Option<String> {
    let base = snapshot.metadata.url.as_deref()?;
    let base_url = Url::parse(base).ok()?;
    base_url.join(src).ok().map(|url| url.to_string())
}

fn extract_table_data(element: ElementRef<'_>) -> TableData {
    let row_selector = Selector::parse("tr").expect("valid selector");
    let cell_selector = Selector::parse("th, td").expect("valid selector");
    let caption_selector = Selector::parse("caption").expect("valid selector");
    let rows = element
        .select(&row_selector)
        .map(|row| TableRow {
            cells: row
                .select(&cell_selector)
                .map(|cell| TableCell {
                    text: clean_element_text(cell),
                    header: cell.value().name().eq_ignore_ascii_case("th"),
                })
                .filter(|cell| !cell.text.is_empty())
                .collect::<Vec<_>>(),
        })
        .filter(|row| !row.cells.is_empty())
        .collect::<Vec<_>>();

    TableData {
        title: element
            .select(&caption_selector)
            .next()
            .map(clean_element_text)
            .filter(|text| !text.is_empty()),
        rows,
    }
}

fn clean_element_text(element: ElementRef<'_>) -> String {
    let mut out = String::new();
    append_clean_text(element, &mut out);
    normalize_text(&out)
}

fn append_clean_text(element: ElementRef<'_>, out: &mut String) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => {
                out.push_str(text);
                out.push(' ');
            }
            Node::Element(node) => {
                if should_skip_text_descendant(node.name()) {
                    continue;
                }
                if let Some(child_element) = ElementRef::wrap(child) {
                    append_clean_text(child_element, out);
                }
            }
            _ => {}
        }
    }
}

fn should_skip_text_descendant(tag: &str) -> bool {
    matches!(
        tag,
        "style" | "script" | "noscript" | "template" | "svg" | "canvas"
    )
}

fn collect_raw_preformatted_text(element: ElementRef<'_>) -> String {
    let mut out = String::new();
    append_preformatted_text(element, &mut out);
    out
}

fn append_preformatted_text(element: ElementRef<'_>, out: &mut String) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(node) => {
                if should_skip_text_descendant(node.name()) {
                    continue;
                }
                if let Some(child_element) = ElementRef::wrap(child) {
                    append_preformatted_text(child_element, out);
                }
            }
            _ => {}
        }
    }
}

fn estimate_image_height(element: ElementRef<'_>, layout_hint: Option<&LayoutHint>) -> f32 {
    if let Some(hint) = layout_hint {
        if let Some(height) = hint.rendered_height {
            return height.clamp(96.0, 360.0);
        }
        let width = hint.rendered_width.unwrap_or(hint.rect.width).max(1.0);
        let height = hint.rect.height.max(1.0);
        let fitted_width = width.clamp(160.0, 420.0);
        return (height * (fitted_width / width)).clamp(96.0, 360.0);
    }

    let width = element
        .value()
        .attr("width")
        .and_then(parse_dimension_attr)
        .filter(|width| *width > 0.0);
    let height = element
        .value()
        .attr("height")
        .and_then(parse_dimension_attr)
        .filter(|height| *height > 0.0);

    match (width, height) {
        (Some(width), Some(height)) => {
            let fitted_width = width.clamp(140.0, 420.0);
            (height * (fitted_width / width)).clamp(96.0, 320.0)
        }
        _ => 140.0,
    }
}

fn parse_dimension_attr(value: &str) -> Option<f32> {
    let numeric = value
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect::<String>();
    numeric.parse::<f32>().ok()
}

fn classify_section_role(text: &str) -> SectionRole {
    let lower = text.to_ascii_lowercase();
    if ["references", "notes", "further reading", "external links"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        SectionRole::References
    } else {
        SectionRole::Body
    }
}

fn parse_alignment(value: &str) -> Option<TextAlignment> {
    match value.to_ascii_lowercase().as_str() {
        "left" | "start" => Some(TextAlignment::Start),
        "center" => Some(TextAlignment::Center),
        "right" | "end" => Some(TextAlignment::End),
        _ => None,
    }
}

fn parse_float_mode(value: &str) -> Option<FloatMode> {
    match value.to_ascii_lowercase().as_str() {
        "left" => Some(FloatMode::Left),
        "right" => Some(FloatMode::Right),
        _ => None,
    }
}

fn is_meaningful_alt_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    !text.is_empty()
        && !matches!(
            lower.as_str(),
            "image" | "photo" | "thumbnail" | "icon" | "logo" | "diagram"
        )
}

fn normalize_text(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}
