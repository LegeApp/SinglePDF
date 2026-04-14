use singlepdf::{DomProcessor, HtmlSnapshot};

#[test]
fn suppresses_stylesheet_background_images_without_geometry_hints() {
    let snapshot = HtmlSnapshot::from_html(
        r#"
        <html>
          <head>
            <style>
              .hero { background-image: url("file:///tmp/class.png"); }
              #banner { background-image: url("file:///tmp/id.png"); }
              section { background-image: url("file:///tmp/tag.png"); }
              .hero .nested { background-image: url("file:///tmp/ignored.png"); }
            </style>
          </head>
          <body>
            <div class="hero">Hero</div>
            <div id="banner">Banner</div>
            <section>Section</section>
          </body>
        </html>
        "#,
    );
    let parsed = DomProcessor::parse(&snapshot);
    let sources = parsed
        .blocks
        .iter()
        .filter_map(|block| block.source.as_deref())
        .collect::<Vec<_>>();

    assert!(!sources.contains(&"file:///tmp/class.png"));
    assert!(!sources.contains(&"file:///tmp/id.png"));
    assert!(!sources.contains(&"file:///tmp/tag.png"));
    assert!(!sources.contains(&"file:///tmp/ignored.png"));
}
