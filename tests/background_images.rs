use singlepdf::{DomProcessor, HtmlSnapshot};

#[test]
fn includes_inline_background_images_with_meaningful_size_without_hints() {
    let snapshot = HtmlSnapshot::from_html(
        r#"
        <html>
          <body>
            <div style="background-image: url('file:///tmp/example.png'); min-height: 120px;">
              Caption text
            </div>
          </body>
        </html>
        "#,
    );
    let parsed = DomProcessor::parse(&snapshot);

    assert!(parsed
        .blocks
        .iter()
        .any(|block| block.source.as_deref() == Some("file:///tmp/example.png")));
}

#[test]
fn suppresses_icon_like_background_images_without_hints() {
    let snapshot = HtmlSnapshot::from_html(
        r#"
        <html>
          <body>
            <div class="icon-button" style="background-image: url('file:///tmp/icon.png'); width: 20px; height: 20px;"></div>
          </body>
        </html>
        "#,
    );
    let parsed = DomProcessor::parse(&snapshot);

    assert!(!parsed
        .blocks
        .iter()
        .any(|block| block.source.as_deref() == Some("file:///tmp/icon.png")));
}
