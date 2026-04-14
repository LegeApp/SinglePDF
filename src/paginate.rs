use std::collections::VecDeque;

use crate::{
    model::{
        BlockKind, FlowBlock, LogicalPage, PageLayout, PaginationPolicy, PositionedBlock, Rect,
        TableData, TableRole, TextAlignment,
    },
    text_layout::TextMeasurer,
};

const FLOAT_GUTTER_PT: f32 = 18.0;
const MIN_WIDOW_ORPHAN_LINES: usize = 2;

pub struct Paginator {
    layout: PageLayout,
    measurer: TextMeasurer,
}

#[derive(Clone, Copy)]
struct ActiveLane {
    x: f32,
    y_end: f32,
}

impl Paginator {
    pub fn new(layout: PageLayout) -> Self {
        Self {
            layout,
            measurer: TextMeasurer::new().expect("system text measurement fonts should load"),
        }
    }

    pub fn paginate(&self, blocks: &[FlowBlock]) -> Vec<LogicalPage> {
        let mut queue = VecDeque::from(blocks.to_vec());
        let mut pages = Vec::new();
        let content_width =
            self.layout.width_pt - self.layout.margin.left - self.layout.margin.right;
        let content_height =
            self.layout.height_pt - self.layout.margin.top - self.layout.margin.bottom;
        let mut page = LogicalPage {
            number: 1,
            width_pt: self.layout.width_pt,
            height_pt: self.layout.height_pt,
            blocks: Vec::new(),
        };
        let mut cursor_y = self.layout.margin.top;
        let mut lane: Option<ActiveLane> = None;

        while let Some(block) = queue.pop_front() {
            if lane.is_some_and(|active| cursor_y >= active.y_end) {
                lane = None;
            }

            if self.should_float_aside(&block, lane, content_width) {
                let width = self.float_width(&block, content_width);
                let (rect, positioned) = self.layout_atomic_block(
                    &block,
                    self.layout.margin.left + content_width - width,
                    cursor_y + block.spacing_before,
                    width,
                );
                let needed_bottom = rect.y + rect.h + block.spacing_after;
                if needed_bottom > self.layout.margin.top + content_height && !page.blocks.is_empty() {
                    pages.push(page);
                    page = LogicalPage {
                        number: pages.len() + 1,
                        width_pt: self.layout.width_pt,
                        height_pt: self.layout.height_pt,
                        blocks: Vec::new(),
                    };
                    cursor_y = self.layout.margin.top;
                    lane = None;
                    queue.push_front(block);
                    continue;
                }
                lane = Some(ActiveLane {
                    x: rect.x,
                    y_end: needed_bottom,
                });
                page.blocks.push(positioned);
                continue;
            }

            let (x, width) = self.available_flow_region(lane, content_width, cursor_y, &block);
            let available_height = self.layout.margin.top + content_height - (cursor_y + block.spacing_before);
            if available_height <= 0.0 && !page.blocks.is_empty() {
                pages.push(page);
                page = LogicalPage {
                    number: pages.len() + 1,
                    width_pt: self.layout.width_pt,
                    height_pt: self.layout.height_pt,
                    blocks: Vec::new(),
                };
                cursor_y = self.layout.margin.top;
                lane = None;
                queue.push_front(block);
                continue;
            }

            if matches!(block.pagination, PaginationPolicy::KeepWithNext)
                && !page.blocks.is_empty()
                && available_height < self.estimate_following_height(&block, queue.front(), width)
            {
                pages.push(page);
                page = LogicalPage {
                    number: pages.len() + 1,
                    width_pt: self.layout.width_pt,
                    height_pt: self.layout.height_pt,
                    blocks: Vec::new(),
                };
                cursor_y = self.layout.margin.top;
                lane = None;
                queue.push_front(block);
                continue;
            }

            match self.layout_flow_block(block.clone(), x, cursor_y, width, available_height) {
                FlowPlacement::Placed(positioned) => {
                    cursor_y = positioned.rect.y + positioned.rect.h + positioned.block.spacing_after;
                    page.blocks.push(positioned);
                }
                FlowPlacement::PlacedWithRemainder { positioned, remainder } => {
                    page.blocks.push(positioned);
                    pages.push(page);
                    page = LogicalPage {
                        number: pages.len() + 1,
                        width_pt: self.layout.width_pt,
                        height_pt: self.layout.height_pt,
                        blocks: Vec::new(),
                    };
                    cursor_y = self.layout.margin.top;
                    lane = None;
                    queue.push_front(remainder);
                }
                FlowPlacement::NeedsPageBreak(block) => {
                    if !page.blocks.is_empty() {
                        pages.push(page);
                        page = LogicalPage {
                            number: pages.len() + 1,
                            width_pt: self.layout.width_pt,
                            height_pt: self.layout.height_pt,
                            blocks: Vec::new(),
                        };
                        cursor_y = self.layout.margin.top;
                        lane = None;
                        queue.push_front(block);
                    } else {
                        let (_, positioned) = self.layout_atomic_block(
                            &block,
                            x,
                            self.layout.margin.top + block.spacing_before,
                            width,
                        );
                        cursor_y = positioned.rect.y + positioned.rect.h + positioned.block.spacing_after;
                        page.blocks.push(positioned);
                    }
                }
            }
        }

        if !page.blocks.is_empty() || pages.is_empty() {
            pages.push(page);
        }

        pages
    }

    fn should_float_aside(&self, block: &FlowBlock, lane: Option<ActiveLane>, content_width: f32) -> bool {
        lane.is_none()
            && content_width >= 420.0
            && matches!(block.table_role, Some(TableRole::Infobox))
            && block.layout_hint.as_ref().is_some_and(|hint| {
                hint.float_mode.is_some() || hint.is_aside || hint.rect.width < hint.rect.height * 1.6
            })
    }

    fn float_width(&self, block: &FlowBlock, content_width: f32) -> f32 {
        block.layout_hint
            .as_ref()
            .and_then(|hint| hint.rendered_width.or(Some(hint.rect.width)))
            .map(|width| width.clamp(180.0, (content_width * 0.42).min(250.0)))
            .unwrap_or((content_width * 0.34).clamp(180.0, 230.0))
    }

    fn available_flow_region(
        &self,
        lane: Option<ActiveLane>,
        content_width: f32,
        cursor_y: f32,
        block: &FlowBlock,
    ) -> (f32, f32) {
        let mut x = self.layout.margin.left;
        let mut width = content_width;
        if let Some(active) = lane {
            if cursor_y < active.y_end {
                width = (active.x - self.layout.margin.left - FLOAT_GUTTER_PT).max(180.0);
            }
        }
        let preferred = self.preferred_flow_width(block, width);
        if preferred < width {
            x += match block.style.alignment {
                TextAlignment::Center => (width - preferred) * 0.5,
                TextAlignment::End => width - preferred,
                TextAlignment::Start => 0.0,
            };
            width = preferred;
        }
        (x, width)
    }

    fn preferred_flow_width(&self, block: &FlowBlock, available_width: f32) -> f32 {
        match block.kind {
            BlockKind::Figure | BlockKind::Image => block
                .layout_hint
                .as_ref()
                .and_then(|hint| hint.rendered_width.or(Some(hint.rect.width)))
                .map(|width| width.clamp(160.0, available_width))
                .unwrap_or(available_width.min(360.0)),
            BlockKind::Aside => block
                .layout_hint
                .as_ref()
                .and_then(|hint| hint.rendered_width.or(Some(hint.rect.width)))
                .map(|width| width.clamp(180.0, available_width))
                .unwrap_or(available_width),
            _ => available_width,
        }
    }

    fn estimate_following_height(
        &self,
        block: &FlowBlock,
        next_block: Option<&FlowBlock>,
        width: f32,
    ) -> f32 {
        let current = self.measure_block_height(block, width) + block.spacing_before + block.spacing_after;
        let next = next_block
            .map(|next| self.measure_block_height(next, width) + next.spacing_before + next.spacing_after)
            .unwrap_or(0.0);
        current + next.min(72.0)
    }

    fn layout_flow_block(
        &self,
        block: FlowBlock,
        x: f32,
        cursor_y: f32,
        width: f32,
        available_height: f32,
    ) -> FlowPlacement {
        match block.pagination {
            PaginationPolicy::Atomic | PaginationPolicy::KeepWithNext => {
                let (rect, positioned) =
                    self.layout_atomic_block(&block, x, cursor_y + block.spacing_before, width);
                if rect.h > available_height && cursor_y > self.layout.margin.top {
                    FlowPlacement::NeedsPageBreak(block)
                } else {
                    FlowPlacement::Placed(positioned)
                }
            }
            PaginationPolicy::SplitParagraph | PaginationPolicy::SplitPreformatted => {
                self.layout_split_text_block(block, x, cursor_y, width, available_height)
            }
            PaginationPolicy::SplitTable => self.layout_split_table_block(block, x, cursor_y, width, available_height),
        }
    }

    fn layout_atomic_block(
        &self,
        block: &FlowBlock,
        x: f32,
        y: f32,
        width: f32,
    ) -> (Rect, PositionedBlock) {
        let lines = self.wrap_block(block, width);
        let height = self.measure_block_height_with_lines(block, width, &lines);
        let rect = Rect { x, y, w: width, h: height };
        let (text_rect, image_rect) = self.inner_rects(block, rect, &lines);
        (
            rect,
            PositionedBlock {
                rect,
                text_rect,
                image_rect,
                block: block.clone(),
                lines,
            },
        )
    }

    fn layout_split_text_block(
        &self,
        block: FlowBlock,
        x: f32,
        cursor_y: f32,
        width: f32,
        available_height: f32,
    ) -> FlowPlacement {
        let lines = self.wrap_block(&block, width);
        let total_height = self.measure_block_height_with_lines(&block, width, &lines);
        if total_height <= available_height || cursor_y == self.layout.margin.top {
            let (_rect, positioned) =
                self.layout_atomic_block(&block, x, cursor_y + block.spacing_before, width);
            return FlowPlacement::Placed(positioned);
        }

        let fit_lines = self.fittable_line_count(&block, available_height);
        if fit_lines < MIN_WIDOW_ORPHAN_LINES || lines.len().saturating_sub(fit_lines) < MIN_WIDOW_ORPHAN_LINES {
            return FlowPlacement::NeedsPageBreak(block);
        }

        let head_lines = lines[..fit_lines].to_vec();
        let tail_lines = lines[fit_lines..].to_vec();
        let head_text = join_lines_for_block(&block, &head_lines);
        let tail_text = join_lines_for_block(&block, &tail_lines);
        let mut first = block.clone();
        let mut remainder = block;
        first.text = head_text;
        remainder.text = tail_text;
        let (rect, positioned) =
            self.layout_atomic_block(&first, x, cursor_y + first.spacing_before, width);
        if rect.h > available_height {
            FlowPlacement::NeedsPageBreak(remainder)
        } else {
            FlowPlacement::PlacedWithRemainder {
                positioned,
                remainder,
            }
        }
    }

    fn layout_split_table_block(
        &self,
        block: FlowBlock,
        x: f32,
        cursor_y: f32,
        width: f32,
        available_height: f32,
    ) -> FlowPlacement {
        let Some(table) = block.table.as_ref() else {
            return self.layout_split_text_block(block, x, cursor_y, width, available_height);
        };

        let column_widths = estimate_table_column_widths(table, width, &self.measurer);
        let row_heights = estimate_table_row_heights(table, &column_widths, &self.measurer);
        let title_height = self.block_title_height(&block, width);
        let available_rows_height =
            available_height - block.style.padding.top - block.style.padding.bottom - title_height;
        if available_rows_height <= 24.0 {
            return FlowPlacement::NeedsPageBreak(block);
        }

        let mut used = 0.0;
        let mut split_index = 0usize;
        for height in &row_heights {
            if used + height > available_rows_height {
                break;
            }
            used += height;
            split_index += 1;
        }

        if split_index == 0 {
            return FlowPlacement::NeedsPageBreak(block);
        }

        let total_rows = table.rows.len();
        let first_rows = table.rows[..split_index].to_vec();
        let remainder_rows = table.rows[split_index..].to_vec();

        if split_index >= total_rows || cursor_y == self.layout.margin.top {
            let (rect, positioned) =
                self.layout_atomic_block(&block, x, cursor_y + block.spacing_before, width);
            if rect.h > available_height && cursor_y > self.layout.margin.top {
                FlowPlacement::NeedsPageBreak(block)
            } else {
                FlowPlacement::Placed(positioned)
            }
        } else {
            let mut first = block.clone();
            let mut remainder = block;
            first.table = Some(TableData {
                title: first.title.clone(),
                rows: first_rows,
            });
            remainder.table = Some(TableData {
                title: None,
                rows: remainder_rows,
            });
            let (_, positioned) =
                self.layout_atomic_block(&first, x, cursor_y + first.spacing_before, width);
            FlowPlacement::PlacedWithRemainder { positioned, remainder }
        }
    }

    fn wrap_block(&self, block: &FlowBlock, width_pt: f32) -> Vec<String> {
        let inner_width = inner_width(block, width_pt);
        match block.kind {
            BlockKind::Image | BlockKind::Table | BlockKind::Aside => block
                .title
                .clone()
                .into_iter()
                .chain(block.caption.clone())
                .flat_map(|text| self.measurer.wrap_text(&text, inner_width, &block.style.text))
                .collect(),
            BlockKind::Figure => block
                .caption
                .clone()
                .into_iter()
                .flat_map(|text| self.measurer.wrap_text(&text, inner_width, &block.style.text))
                .collect(),
            BlockKind::Preformatted => block
                .text
                .lines()
                .map(|line| line.trim_end().to_string())
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>(),
            _ => self.measurer.wrap_text(&block.text, inner_width - block.style.indent_pt, &block.style.text),
        }
    }

    fn measure_block_height(&self, block: &FlowBlock, width: f32) -> f32 {
        let lines = self.wrap_block(block, width);
        self.measure_block_height_with_lines(block, width, &lines)
    }

    fn measure_block_height_with_lines(&self, block: &FlowBlock, width: f32, lines: &[String]) -> f32 {
        let padding = block.style.padding.top + block.style.padding.bottom;
        match block.kind {
            BlockKind::Image => padding + block.estimated_height.max(72.0) + caption_height(lines, &block.style.text),
            BlockKind::Figure => {
                padding
                    + block.estimated_height.max(96.0)
                    + if lines.is_empty() { 0.0 } else { 6.0 + caption_height(lines, &block.style.text) }
            }
            BlockKind::Table | BlockKind::Aside if block.table.is_some() => {
                padding + self.block_title_height(block, width) + table_height(block.table.as_ref().unwrap(), width, &self.measurer)
            }
            _ => padding + lines.len().max(1) as f32 * block.style.text.line_height_pt
        }
    }

    fn block_title_height(&self, block: &FlowBlock, width: f32) -> f32 {
        block.title
            .as_ref()
            .map(|title| {
                self.measurer
                    .wrap_text(title, inner_width(block, width), &block.style.text)
                    .len() as f32
                    * block.style.text.line_height_pt
                    + 4.0
            })
            .unwrap_or(0.0)
    }

    fn inner_rects(&self, block: &FlowBlock, rect: Rect, lines: &[String]) -> (Option<Rect>, Option<Rect>) {
        let text_rect = Rect {
            x: rect.x + block.style.padding.left + block.style.indent_pt,
            y: rect.y + block.style.padding.top,
            w: (rect.w - block.style.padding.left - block.style.padding.right - block.style.indent_pt).max(12.0),
            h: (rect.h - block.style.padding.top - block.style.padding.bottom).max(0.0),
        };

        match block.kind {
            BlockKind::Image => (
                (!lines.is_empty()).then_some(Rect {
                    x: text_rect.x,
                    y: rect.y + rect.h - block.style.padding.bottom - caption_height(lines, &block.style.text),
                    w: text_rect.w,
                    h: caption_height(lines, &block.style.text),
                }),
                Some(Rect {
                    x: rect.x + block.style.padding.left,
                    y: rect.y + block.style.padding.top,
                    w: rect.w - block.style.padding.left - block.style.padding.right,
                    h: block.estimated_height.max(72.0),
                }),
            ),
            BlockKind::Figure => (
                (!lines.is_empty()).then_some(Rect {
                    x: text_rect.x,
                    y: rect.y + rect.h - block.style.padding.bottom - caption_height(lines, &block.style.text),
                    w: text_rect.w,
                    h: caption_height(lines, &block.style.text),
                }),
                Some(Rect {
                    x: rect.x + block.style.padding.left,
                    y: rect.y + block.style.padding.top,
                    w: rect.w - block.style.padding.left - block.style.padding.right,
                    h: block.estimated_height.max(96.0),
                }),
            ),
            _ => (Some(text_rect), None),
        }
    }

    fn fittable_line_count(&self, block: &FlowBlock, available_height: f32) -> usize {
        let usable = available_height - block.style.padding.top - block.style.padding.bottom;
        if usable <= 0.0 {
            return 0;
        }
        (usable / block.style.text.line_height_pt).floor() as usize
    }
}

enum FlowPlacement {
    Placed(PositionedBlock),
    PlacedWithRemainder { positioned: PositionedBlock, remainder: FlowBlock },
    NeedsPageBreak(FlowBlock),
}

fn inner_width(block: &FlowBlock, width_pt: f32) -> f32 {
    (width_pt
        - block.style.padding.left
        - block.style.padding.right
        - block.style.indent_pt)
        .max(24.0)
}

fn caption_height(lines: &[String], style: &crate::model::TextStyle) -> f32 {
    lines.len() as f32 * style.line_height_pt
}

fn join_lines_for_block(block: &FlowBlock, lines: &[String]) -> String {
    if matches!(block.kind, BlockKind::Preformatted) {
        lines.join("\n")
    } else {
        lines.join(" ")
    }
}

pub fn estimate_table_column_widths(
    table: &TableData,
    width_pt: f32,
    measurer: &TextMeasurer,
) -> Vec<f32> {
    let column_count = table.rows.iter().map(|row| row.cells.len()).max().unwrap_or(1);
    let min_width = (width_pt / column_count as f32).min(140.0).max(72.0);
    let probe_style = crate::model::TextStyle {
        font_size_pt: 9.5,
        line_height_pt: 11.5,
        bold: false,
        italic: false,
        color: crate::model::Color { r: 0, g: 0, b: 0 },
    };
    let mut weights = vec![1.0f32; column_count];

    for row in &table.rows {
        for (index, cell) in row.cells.iter().enumerate() {
            let width = measurer.measure_text_width(&cell.text, &probe_style);
            weights[index] = weights[index].max((width / 24.0).clamp(1.0, 12.0));
        }
    }

    let total_weight = weights.iter().sum::<f32>().max(1.0);
    let mut widths = weights
        .iter()
        .map(|weight| (width_pt * (*weight / total_weight)).max(min_width))
        .collect::<Vec<_>>();
    let width_sum = widths.iter().sum::<f32>();
    if width_sum > width_pt {
        let scale = width_pt / width_sum;
        widths.iter_mut().for_each(|width| *width *= scale);
    } else if let Some(last) = widths.last_mut() {
        *last += width_pt - width_sum;
    }
    widths
}

pub fn estimate_table_row_heights(
    table: &TableData,
    column_widths: &[f32],
    measurer: &TextMeasurer,
) -> Vec<f32> {
    table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    let width = *column_widths.get(index).unwrap_or(&72.0);
                    let style = crate::model::TextStyle {
                        font_size_pt: 9.5,
                        line_height_pt: 11.5,
                        bold: cell.header,
                        italic: false,
                        color: crate::model::Color { r: 0, g: 0, b: 0 },
                    };
                    let lines = measurer.wrap_text(&cell.text, (width - 12.0).max(32.0), &style);
                    (lines.len().max(1) as f32 * 11.5 + 12.0).max(24.0)
                })
                .fold(24.0, f32::max)
        })
        .collect()
}

fn table_height(table: &TableData, width_pt: f32, measurer: &TextMeasurer) -> f32 {
    estimate_table_row_heights(
        table,
        &estimate_table_column_widths(table, width_pt, measurer),
        measurer,
    )
    .into_iter()
    .sum()
}
