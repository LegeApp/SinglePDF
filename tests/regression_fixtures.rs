use std::{fs, path::PathBuf};

use serde::Deserialize;
use singlepdf::{BlockKind, DomProcessor, HtmlSnapshot, PageLayout, Paginator, PdfRenderer};

#[derive(Debug, Deserialize)]
struct FixtureManifest {
    fixtures: Vec<FixtureCase>,
}

#[derive(Debug, Deserialize)]
struct FixtureCase {
    name: String,
    html_path: String,
    enabled: Option<bool>,
    expected_pages_min: usize,
    expected_pages_max: usize,
    expected_image_blocks_min: usize,
    expected_image_blocks_max: usize,
    max_skipped_images: usize,
}

#[test]
#[ignore = "Runs against local saved SingleFile fixture HTML files"]
fn singlefile_regression_fixtures() {
    let manifest_path = std::env::var("SINGLEPDF_REGRESSION_MANIFEST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("regression")
                .join("manifest.json")
        });
    let manifest_text = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", manifest_path.display()));
    let manifest: FixtureManifest = serde_json::from_str(&manifest_text)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", manifest_path.display()));

    let paginator = Paginator::new(PageLayout::default());
    let mut executed = 0usize;

    for fixture in manifest
        .fixtures
        .iter()
        .filter(|fixture| fixture.enabled.unwrap_or(true))
    {
        let html_path = PathBuf::from(&fixture.html_path);
        if !html_path.exists() {
            eprintln!(
                "Skipping fixture '{}' because file is missing: {}",
                fixture.name,
                html_path.display()
            );
            continue;
        }

        executed += 1;
        let snapshot = HtmlSnapshot::from_html_file(&html_path)
            .unwrap_or_else(|error| panic!("fixture '{}': snapshot load failed: {error}", fixture.name));
        let parsed = DomProcessor::parse(&snapshot);
        let pages = paginator.paginate(&parsed.blocks);
        let (_, report) = PdfRenderer::bytes_and_report_from_pages(&pages)
            .unwrap_or_else(|error| panic!("fixture '{}': render failed: {error}", fixture.name));

        let image_blocks = parsed
            .blocks
            .iter()
            .filter(|block| {
                matches!(block.kind, BlockKind::Image | BlockKind::Figure) && block.source.is_some()
            })
            .count();

        assert!(
            pages.len() >= fixture.expected_pages_min && pages.len() <= fixture.expected_pages_max,
            "fixture '{}': page_count={} not in [{}..={}]",
            fixture.name,
            pages.len(),
            fixture.expected_pages_min,
            fixture.expected_pages_max
        );
        assert!(
            image_blocks >= fixture.expected_image_blocks_min
                && image_blocks <= fixture.expected_image_blocks_max,
            "fixture '{}': image_blocks={} not in [{}..={}]",
            fixture.name,
            image_blocks,
            fixture.expected_image_blocks_min,
            fixture.expected_image_blocks_max
        );
        assert!(
            report.skipped_images <= fixture.max_skipped_images,
            "fixture '{}': skipped_images={} exceeds {}",
            fixture.name,
            report.skipped_images,
            fixture.max_skipped_images
        );
    }

    assert!(
        executed > 0,
        "no enabled fixture files were found; update {}",
        manifest_path.display()
    );
}
