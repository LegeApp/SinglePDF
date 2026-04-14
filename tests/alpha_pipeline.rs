use singlepdf::{
    BlockKind, DomProcessor, HtmlSnapshot, PageLayout, Paginator, PdfRenderer, SnapshotDocument,
    SnapshotMetadata, TableRole,
};
use singlepdf::model::SnapshotSource;

#[test]
fn parses_semantic_blocks_from_html() {
    let snapshot = HtmlSnapshot::from_html(
        r#"
        <html>
          <head><title>Alpha Test</title></head>
          <body>
            <h1>Document Title</h1>
            <p>First paragraph with enough text to become a normal content block.</p>
            <img alt="Portrait of Jacques Pépin" src="data:image/png;base64,AAAA" />
            <table><caption>Facts</caption><tr><td>A</td><td>B</td></tr></table>
          </body>
        </html>
        "#,
    );

    let parsed = DomProcessor::parse(&snapshot);
    assert_eq!(parsed.title.as_deref(), Some("Alpha Test"));
    assert!(parsed
        .blocks
        .iter()
        .any(|block| block.text == "Document Title"));
    assert!(parsed
        .blocks
        .iter()
        .any(|block| block.text.contains("First paragraph")));
    assert!(parsed
        .blocks
        .iter()
        .any(|block| matches!(block.kind, BlockKind::Image | BlockKind::Figure)));
    let table_block = parsed
        .blocks
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Table) && block.table_role == Some(TableRole::Data))
        .expect("data table should be preserved");
    assert_eq!(table_block.title.as_deref(), Some("Facts"));
    assert_eq!(table_block.table.as_ref().unwrap().rows[0].cells[0].text, "A");
}

#[test]
fn paginates_long_documents_into_multiple_pages() {
    let html = format!(
        "<html><body><h1>Long Doc</h1>{}</body></html>",
        (0..80)
            .map(|_| {
                "<p>Lorem ipsum dolor sit amet, consectetur adipiscing elit. Vestibulum non.</p>"
            })
            .collect::<String>()
    );
    let snapshot = HtmlSnapshot::from_html(html);
    let parsed = DomProcessor::parse(&snapshot);
    let pages = Paginator::new(PageLayout::default()).paginate(&parsed.blocks);

    assert!(pages.len() >= 2);
    assert!(pages.iter().all(|page| !page.blocks.is_empty()));
}

#[test]
fn ignores_style_text_inside_content_blocks() {
    let snapshot = HtmlSnapshot::from_html(
        r#"
        <html>
          <body>
            <p>Jacques <style>.mw-parser-output{font-size:85%}</style>Pépin</p>
            <table>
              <tr>
                <td>Cell <style>.infobox{display:none}</style>Value</td>
              </tr>
            </table>
          </body>
        </html>
        "#,
    );

    let parsed = DomProcessor::parse(&snapshot);
    let joined_text = parsed
        .blocks
        .iter()
        .map(|block| {
            let mut text = block.text.clone();
            if let Some(table) = &block.table {
                for row in &table.rows {
                    for cell in &row.cells {
                        text.push(' ');
                        text.push_str(&cell.text);
                    }
                }
            }
            text
        })
        .collect::<Vec<_>>()
        .join(" ");

    assert!(joined_text.contains("Jacques Pépin"));
    assert!(joined_text.contains("Cell Value"));
    assert!(!joined_text.contains(".mw-parser-output"));
    assert!(!joined_text.contains(".infobox"));
}

#[test]
fn routes_infoboxes_and_skips_navigation_tables() {
    let snapshot = HtmlSnapshot::from_html(
        r#"
        <html>
          <body>
            <table class="navbox"><tr><td>Navigation junk</td></tr></table>
            <table class="infobox">
              <caption>Useful infobox</caption>
              <tr><th>Born</th><td>1935</td></tr>
              <tr><th>Role</th><td>Chef</td></tr>
              <tr><th>Known for</th><td>Television</td></tr>
              <tr><th>Nationality</th><td>French</td></tr>
            </table>
          </body>
        </html>
        "#,
    );

    let parsed = DomProcessor::parse(&snapshot);
    assert!(!parsed.blocks.iter().any(|block| {
        block.table
            .as_ref()
            .is_some_and(|table| table.rows.iter().any(|row| row.cells.iter().any(|cell| cell.text.contains("Navigation junk"))))
    }));

    let infobox = parsed
        .blocks
        .iter()
        .find(|block| block.table_role == Some(TableRole::Infobox))
        .expect("infobox should be routed into a semantic aside block");
    assert!(matches!(infobox.kind, BlockKind::Aside));
    assert_eq!(infobox.title.as_deref(), Some("Useful infobox"));
}

#[test]
fn marks_reference_sections_for_compact_rendering() {
    let snapshot = HtmlSnapshot::from_html(
        r#"
        <html>
          <body>
            <h2>References</h2>
            <ul>
              <li>Retrieved 2024-01-01.</li>
            </ul>
          </body>
        </html>
        "#,
    );

    let parsed = DomProcessor::parse(&snapshot);
    assert!(parsed
        .blocks
        .iter()
        .any(|block| matches!(block.kind, BlockKind::ReferenceList)));
}

#[test]
fn prefers_main_content_root_over_text_heavy_reference_subtree() {
    let snapshot_json = r##"
    {
      "metadata": {
        "title": "Root Selection",
        "url": "https://example.com/wiki"
      },
      "html": "<html><body><main data-singlepdf-id=\"root\"><h1 data-singlepdf-id=\"title\">Article Title</h1><p data-singlepdf-id=\"lead\">Lead paragraph with real article content that should not be dropped.</p><div data-singlepdf-id=\"refs\"><h2 data-singlepdf-id=\"refs-title\">References</h2><p data-singlepdf-id=\"refs-text\">Ref one. Ref two. Ref three. Ref four. Ref five. Ref six. Ref seven. Ref eight. Ref nine. Ref ten.</p></div></main></body></html>",
      "hints": [
        {
          "id": "root",
          "tag_name": "main",
          "rect": { "x": 0.0, "y": 0.0, "width": 960.0, "height": 1800.0 },
          "display": "block",
          "rendered_width": 960.0,
          "rendered_height": 1800.0,
          "float": null,
          "is_aside": false,
          "visible": true,
          "text_align": "left",
          "background_color": null,
          "likely_main_content": true,
          "role_hint": "article"
        },
        {
          "id": "refs",
          "tag_name": "div",
          "rect": { "x": 0.0, "y": 600.0, "width": 720.0, "height": 420.0 },
          "display": "block",
          "rendered_width": 720.0,
          "rendered_height": 420.0,
          "float": null,
          "is_aside": false,
          "visible": true,
          "text_align": "left",
          "background_color": null,
          "likely_main_content": true,
          "role_hint": "references"
        }
      ]
    }
    "##;

    let snapshot: singlepdf::SnapshotDocument =
        serde_json::from_str(snapshot_json).expect("snapshot JSON should parse");
    let parsed = DomProcessor::parse(&snapshot);
    let joined = parsed
        .blocks
        .iter()
        .map(|block| block.text.clone())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(joined.contains("Article Title"));
    assert!(joined.contains("Lead paragraph with real article content"));
    assert!(joined.contains("Ref one."));
}

#[test]
fn resolves_relative_image_sources_against_snapshot_url() {
    let snapshot = SnapshotDocument {
        source: SnapshotSource::Url("https://example.com/shop/item".to_string()),
        metadata: SnapshotMetadata {
            title: Some("Relative Image".to_string()),
            url: Some("https://example.com/shop/item".to_string()),
        },
        html: r#"
        <html>
          <body>
            <img src="/assets/product.png" alt="Product image" />
          </body>
        </html>
        "#
        .to_string(),
        hints: Vec::new(),
    };

    let parsed = DomProcessor::parse(&snapshot);
    let image = parsed
        .blocks
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Image | BlockKind::Figure))
        .expect("image block should be preserved");

    assert_eq!(
        image.source.as_deref(),
        Some("https://example.com/assets/product.png")
    );
}

#[test]
fn svg_data_url_images_do_not_abort_pdf_rendering() {
    let snapshot = HtmlSnapshot::from_html(
        r#"
        <html>
          <head><title>SVG Fallback</title></head>
          <body>
            <h1>SVG Fallback</h1>
            <img
              alt="Vector badge"
              src="data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI2NCIgaGVpZ2h0PSI2NCI+PHJlY3Qgd2lkdGg9IjY0IiBoZWlnaHQ9IjY0IiBmaWxsPSIjMDBhY2ZmIi8+PHRleHQgeD0iMzIiIHk9IjM3IiBmb250LXNpemU9IjE2IiB0ZXh0LWFuY2hvcj0ibWlkZGxlIiBmaWxsPSJ3aGl0ZSI+U1ZHPC90ZXh0Pjwvc3ZnPg=="
            />
            <p>Body text should still render.</p>
          </body>
        </html>
        "#,
    );

    let parsed = DomProcessor::parse(&snapshot);
    let pages = Paginator::new(PageLayout::default()).paginate(&parsed.blocks);
    let bytes = PdfRenderer::bytes_from_pages(&pages).expect("pdf render should succeed");

    assert!(bytes.starts_with(b"%PDF-"));
}
