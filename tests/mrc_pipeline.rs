use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

use image::{Rgba, RgbaImage};
use singlepdf::{
    compression,
    model::{
        BlockKind, BlockStyle, EdgeInsets, FlowBlock, LogicalPage, PaginationPolicy, PositionedBlock,
        Rect, TextAlignment, TextStyle,
    },
    PdfRenderer, SinglePdfProfile,
};
use singlepdf::profile::{DefaultRasterMode, MrcMode};

fn write_sample_image() -> PathBuf {
    let mut image = RgbaImage::from_pixel(40, 40, Rgba([255, 255, 255, 255]));
    for y in 8..32 {
        for x in 10..30 {
            image.put_pixel(x, y, Rgba([0, 0, 0, 255]));
        }
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("singlepdf-mrc-{unique}.png"));
    image.save(&path).unwrap();
    path
}

fn image_block(path: &str) -> FlowBlock {
    FlowBlock {
        kind: BlockKind::Image,
        text: String::new(),
        title: None,
        caption: None,
        source: Some(path.to_string()),
        table: None,
        estimated_height: 120.0,
        style: BlockStyle {
            text: TextStyle {
                font_size_pt: 10.0,
                line_height_pt: 12.0,
                bold: false,
                italic: false,
                color: singlepdf::Color { r: 0, g: 0, b: 0 },
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
        },
        spacing_before: 0.0,
        spacing_after: 0.0,
        pagination: PaginationPolicy::Atomic,
        layout_hint: None,
        table_role: None,
        image_role: None,
        section_role: None,
    }
}

#[test]
fn block_compression_prefers_mrc_assets() {
    let path = write_sample_image();
    let mut profile = SinglePdfProfile::default();
    profile.compression.default_raster_mode = DefaultRasterMode::MrcPreferred;
    profile.compression.page_mrc = MrcMode::Enabled;
    profile.compression.block_mrc = MrcMode::Enabled;
    let result = compression::compress_image_source(path.to_str().unwrap(), &profile.compression).unwrap();
    fs::remove_file(&path).ok();

    match result.raster {
        singlepdf::model::RasterAsset::Mrc(mrc) => {
            assert!(mrc.background.is_some());
            assert!(mrc.mask.is_some());
        }
        other => panic!("expected MRC asset, got {other:?}"),
    }
}

#[test]
fn default_block_compression_prefers_single_jpeg2000_assets() {
    let path = write_sample_image();
    let profile = SinglePdfProfile::default();
    let result =
        compression::compress_image_source(path.to_str().unwrap(), &profile.compression).unwrap();
    fs::remove_file(&path).ok();

    match result.raster {
        singlepdf::model::RasterAsset::Single(image) => {
            assert!(matches!(
                image.encoding,
                singlepdf::model::ImageEncoding::Jpeg2000
            ));
        }
        other => panic!("expected direct JPEG2000 asset, got {other:?}"),
    }
}

#[test]
fn monochrome_block_prefers_gray_foreground_layer() {
    let path = write_sample_image();
    let mut profile = SinglePdfProfile::default();
    profile.compression.default_raster_mode = DefaultRasterMode::MrcPreferred;
    profile.compression.page_mrc = MrcMode::Enabled;
    profile.compression.block_mrc = MrcMode::Enabled;
    let result =
        compression::compress_image_source(path.to_str().unwrap(), &profile.compression).unwrap();
    fs::remove_file(&path).ok();

    match result.raster {
        singlepdf::model::RasterAsset::Mrc(mrc) => {
            let foreground = mrc.foreground.expect("foreground layer");
            assert!(matches!(
                foreground.color_space,
                singlepdf::model::ImageColorSpace::Gray8
            ));
            assert!(matches!(
                foreground.encoding,
                singlepdf::model::ImageEncoding::Jpeg2000
            ));
        }
        other => panic!("expected MRC asset, got {other:?}"),
    }
}

#[test]
fn pdf_renderer_emits_jpx_filter_for_default_image_blocks() {
    let path = write_sample_image();
    let page = LogicalPage {
        number: 1,
        width_pt: 612.0,
        height_pt: 792.0,
        blocks: vec![PositionedBlock {
            rect: Rect {
                x: 72.0,
                y: 72.0,
                w: 220.0,
                h: 160.0,
            },
            text_rect: None,
            image_rect: Some(Rect {
                x: 72.0,
                y: 72.0,
                w: 220.0,
                h: 160.0,
            }),
            block: image_block(path.to_str().unwrap()),
            lines: Vec::new(),
        }],
    };
    let pdf = PdfRenderer::bytes_from_pages(&[page]).unwrap();
    fs::remove_file(&path).ok();

    let pdf_text = String::from_utf8_lossy(&pdf);
    assert!(
        pdf_text.contains("JPXDecode"),
        "expected JPEG2000 image filters in PDF output"
    );
}
