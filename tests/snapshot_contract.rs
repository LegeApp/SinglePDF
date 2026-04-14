use singlepdf::{HtmlSnapshot, SnapshotDocument};

#[test]
fn parses_structured_snapshot_json() {
    let json = r#"
    {
      "metadata": {
        "title": "Snapshot Title",
        "url": "https://example.com/article"
      },
      "html": "<html><body><p data-singlepdf-id=\"spdf-1\">Hello</p></body></html>",
      "hints": [
        {
          "id": "spdf-1",
          "tag_name": "p",
          "rect": { "x": 10.0, "y": 20.0, "width": 320.0, "height": 40.0 },
          "display": "block",
          "rendered_width": 320.0,
          "rendered_height": 40.0,
          "float": null,
          "is_aside": false,
          "visible": true,
          "text_align": "left",
          "background_color": null,
          "likely_main_content": true,
          "role_hint": "article"
        }
      ]
    }
    "#;

    let snapshot: SnapshotDocument = serde_json::from_str(json).expect("snapshot JSON should parse");
    assert_eq!(snapshot.metadata.title.as_deref(), Some("Snapshot Title"));
    assert_eq!(snapshot.metadata.url.as_deref(), Some("https://example.com/article"));
    assert_eq!(snapshot.hints.len(), 1);
    assert_eq!(snapshot.hints[0].id, "spdf-1");
    assert!(snapshot.hints[0].likely_main_content);
}

#[test]
fn raw_html_path_keeps_working_as_legacy_fallback() {
    let snapshot = HtmlSnapshot::from_html("<html><body><p>Hello</p></body></html>");
    assert_eq!(snapshot.metadata.title, None);
    assert!(snapshot.hints.is_empty());
    assert!(snapshot.html.contains("<p>Hello</p>"));
}
