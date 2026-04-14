use std::{fs, io::Cursor, path::Path, sync::Arc};

use anyhow::Context;
use image::{DynamicImage, ImageReader, RgbaImage};
use resvg::{tiny_skia, usvg};
use toojpeg::{encode_jpeg, EncodeOptions, ImageFormat};

use crate::model::{EncodedImageAsset, ImageColorSpace, ImageEncoding};

pub fn encode_rgb8_to_jpeg(
    pixels: &[u8],
    width: u32,
    height: u32,
    quality: u8,
    chroma_subsampling_420: bool,
) -> anyhow::Result<EncodedImageAsset> {
    let mut bytes = Vec::new();
    encode_jpeg(
        pixels,
        EncodeOptions {
            width,
            height,
            format: ImageFormat::RGB,
            quality,
            baseline: true,
            optimized: true,
            downsample: chroma_subsampling_420,
        },
        &mut bytes,
    )
    .map_err(anyhow::Error::msg)
    .with_context(|| {
        format!(
            "failed to encode JPEG asset ({}x{}, quality={})",
            width, height, quality
        )
    })?;

    Ok(EncodedImageAsset {
        pixel_width: width,
        pixel_height: height,
        bits_per_component: 8,
        color_space: ImageColorSpace::Rgb8,
        encoding: ImageEncoding::Jpeg { quality },
        bytes,
        jbig2_globals: None,
    })
}

pub fn decode_data_url_to_jpeg_asset(
    data_url: &str,
    quality: u8,
    chroma_subsampling_420: bool,
) -> anyhow::Result<EncodedImageAsset> {
    let image = decode_data_url_to_dynamic_image(data_url)?;
    if matches!(image, DynamicImage::ImageRgb8(_)) {
        let (media_type, encoded) = split_data_url(data_url)?;
        if media_type.eq_ignore_ascii_case("image/jpeg") || media_type.eq_ignore_ascii_case("image/jpg")
        {
            let decoded = decode_base64(encoded)?;
            let dimensions = image::ImageReader::new(Cursor::new(&decoded))
                .with_guessed_format()
                .context("failed to detect embedded jpeg format")?
                .into_dimensions()
                .context("failed to read embedded jpeg dimensions")?;
            return Ok(EncodedImageAsset {
                pixel_width: dimensions.0,
                pixel_height: dimensions.1,
                bits_per_component: 8,
                color_space: ImageColorSpace::Rgb8,
                encoding: ImageEncoding::Jpeg { quality: 100 },
                bytes: decoded,
                jbig2_globals: None,
            });
        }
    }

    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    encode_rgb8_to_jpeg(rgb.as_raw(), width, height, quality, chroma_subsampling_420)
}

pub fn decode_image_source_to_jpeg_asset(
    source: &str,
    quality: u8,
    chroma_subsampling_420: bool,
) -> anyhow::Result<EncodedImageAsset> {
    let image = decode_image_source_to_dynamic_image(source)?;
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    encode_rgb8_to_jpeg(rgb.as_raw(), width, height, quality, chroma_subsampling_420)
}

pub fn decode_image_file_to_jpeg_asset(
    path: &Path,
    quality: u8,
    chroma_subsampling_420: bool,
) -> anyhow::Result<EncodedImageAsset> {
    let image = decode_image_file_to_dynamic_image(path)?;
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    encode_rgb8_to_jpeg(rgb.as_raw(), width, height, quality, chroma_subsampling_420)
}

pub fn decode_image_source_to_dynamic_image(source: &str) -> anyhow::Result<DynamicImage> {
    if source.starts_with("data:") {
        return decode_data_url_to_dynamic_image(source);
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        anyhow::bail!("network image fetching is not supported in the Rust renderer");
    }
    if let Some(path) = source.strip_prefix("file:///") {
        let normalized = path.replace('/', "\\");
        return decode_image_file_to_dynamic_image(Path::new(&normalized));
    }
    decode_image_file_to_dynamic_image(Path::new(source))
}

pub fn decode_image_file_to_dynamic_image(path: &Path) -> anyhow::Result<DynamicImage> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read image file {}", path.display()))?;
    decode_image_bytes_to_dynamic_image(&bytes, Some(path))
}

pub fn decode_image_bytes_to_dynamic_image(
    bytes: &[u8],
    path_hint: Option<&Path>,
) -> anyhow::Result<DynamicImage> {
    if matches!(
        path_hint
            .and_then(|path| path.extension().and_then(|ext| ext.to_str())),
        Some(ext) if ext.eq_ignore_ascii_case("svg")
    ) {
        return render_svg_bytes_to_dynamic_image(bytes, path_hint.and_then(|path| path.parent()));
    }

    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("failed to detect image format")?
        .decode()
        .context("failed to decode image")
}

pub fn decode_data_url_to_dynamic_image(data_url: &str) -> anyhow::Result<DynamicImage> {
    let (media_type, encoded) = split_data_url(data_url)?;
    if media_type.contains("svg") {
        let decoded = decode_base64(encoded)?;
        return render_svg_bytes_to_dynamic_image(&decoded, None);
    }
    let decoded = decode_base64(encoded)?;
    decode_image_bytes_to_dynamic_image(&decoded, None)
}

fn render_svg_bytes_to_dynamic_image(
    svg_data: &[u8],
    resources_dir: Option<&Path>,
) -> anyhow::Result<DynamicImage> {
    let mut options = usvg::Options::default();
    options.resources_dir = resources_dir.map(Path::to_path_buf);
    options.fontdb = Arc::clone(svg_fontdb());
    let tree =
        usvg::Tree::from_data(svg_data, &options).context("failed to parse SVG source")?;
    let pixmap_size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .context("failed to allocate SVG pixmap")?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    let rgba = unpremultiply_rgba(pixmap.data(), pixmap.width(), pixmap.height())
        .context("failed to materialize SVG raster pixels")?;
    Ok(DynamicImage::ImageRgba8(rgba))
}

fn svg_fontdb() -> &'static Arc<usvg::fontdb::Database> {
    static FONT_DB: std::sync::OnceLock<Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    FONT_DB.get_or_init(|| {
        let mut options = usvg::Options::default();
        options.fontdb_mut().load_system_fonts();
        Arc::clone(&options.fontdb)
    })
}

fn unpremultiply_rgba(data: &[u8], width: u32, height: u32) -> anyhow::Result<RgbaImage> {
    let mut out = Vec::with_capacity(data.len());
    for pixel in data.chunks_exact(4) {
        let b = pixel[0];
        let g = pixel[1];
        let r = pixel[2];
        let a = pixel[3];
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let alpha = a as u32;
        let unpremultiply = |component: u8| -> u8 {
            (((component as u32 * 255) + (alpha / 2)) / alpha).min(255) as u8
        };
        out.push(unpremultiply(r));
        out.push(unpremultiply(g));
        out.push(unpremultiply(b));
        out.push(a);
    }

    image::RgbaImage::from_raw(width, height, out)
        .context("failed to convert SVG pixels into an RGBA image")
}

fn split_data_url(data_url: &str) -> anyhow::Result<(&str, &str)> {
    let Some(payload) = data_url.strip_prefix("data:") else {
        anyhow::bail!("unsupported image source: expected data URL");
    };
    let Some((header, encoded)) = payload.split_once(',') else {
        anyhow::bail!("malformed data URL");
    };
    if !header.ends_with(";base64") {
        anyhow::bail!("only base64-encoded data URLs are supported");
    }
    let media_type = header.trim_end_matches(";base64");
    Ok((media_type, encoded))
}

fn decode_base64(input: &str) -> anyhow::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut chunk = [0u8; 4];
    let mut chunk_len = 0usize;

    for byte in input.bytes() {
        if matches!(byte, b' ' | b'\r' | b'\n' | b'\t') {
            continue;
        }
        chunk[chunk_len] = byte;
        chunk_len += 1;
        if chunk_len == 4 {
            decode_base64_chunk(chunk, &mut output)?;
            chunk_len = 0;
        }
    }

    if chunk_len != 0 {
        anyhow::bail!("invalid base64 length");
    }

    Ok(output)
}

fn decode_base64_chunk(chunk: [u8; 4], out: &mut Vec<u8>) -> anyhow::Result<()> {
    let mut values = [0u8; 4];
    let mut padding = 0usize;
    for (index, byte) in chunk.into_iter().enumerate() {
        match byte {
            b'=' => {
                values[index] = 0;
                padding += 1;
            }
            _ => values[index] = decode_base64_value(byte)?,
        }
    }

    out.push((values[0] << 2) | (values[1] >> 4));
    if padding < 2 {
        out.push((values[1] << 4) | (values[2] >> 2));
    }
    if padding == 0 {
        out.push((values[2] << 6) | values[3]);
    }
    Ok(())
}

fn decode_base64_value(byte: u8) -> anyhow::Result<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte - b'A'),
        b'a'..=b'z' => Ok(byte - b'a' + 26),
        b'0'..=b'9' => Ok(byte - b'0' + 52),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => anyhow::bail!("invalid base64 byte"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        decode_data_url_to_dynamic_image, decode_data_url_to_jpeg_asset,
        decode_image_file_to_jpeg_asset, encode_rgb8_to_jpeg,
    };
    use crate::model::ImageEncoding;

    #[test]
    fn encodes_small_rgb_jpeg() {
        let pixels = vec![
            255, 0, 0, 0, 255, 0, //
            0, 0, 255, 255, 255, 255,
        ];

        let encoded = encode_rgb8_to_jpeg(&pixels, 2, 2, 82, true).unwrap();
        assert!(encoded.bytes.len() > 32);
        assert_eq!(&encoded.bytes[0..2], &[0xFF, 0xD8]);
        assert_eq!(encoded.bits_per_component, 8);
        assert!(encoded.jbig2_globals.is_none());
        match encoded.encoding {
            ImageEncoding::Jpeg { quality } => assert_eq!(quality, 82),
            _ => panic!("expected jpeg encoding"),
        }
    }

    #[test]
    fn decodes_data_url_and_reencodes_as_jpeg() {
        let pixels = vec![
            255, 0, 0, 0, 255, 0, //
            0, 0, 255, 255, 255, 255,
        ];
        let source_jpeg = encode_rgb8_to_jpeg(&pixels, 2, 2, 90, true).unwrap();
        let data_url = format!(
            "data:image/jpeg;base64,{}",
            encode_base64(&source_jpeg.bytes)
        );
        let encoded = decode_data_url_to_jpeg_asset(&data_url, 80, true).unwrap();
        assert_eq!(encoded.pixel_width, 2);
        assert_eq!(encoded.pixel_height, 2);
        assert_eq!(&encoded.bytes[0..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn passes_through_jpeg_data_urls_without_reencoding() {
        let pixels = vec![
            255, 0, 0, 0, 255, 0, //
            0, 0, 255, 255, 255, 255,
        ];
        let source_jpeg = encode_rgb8_to_jpeg(&pixels, 2, 2, 91, true).unwrap();
        let data_url = format!(
            "data:image/jpeg;base64,{}",
            encode_base64(&source_jpeg.bytes)
        );
        let encoded = decode_data_url_to_jpeg_asset(&data_url, 80, true).unwrap();

        assert_eq!(encoded.pixel_width, 2);
        assert_eq!(encoded.pixel_height, 2);
        assert_eq!(encoded.bytes, source_jpeg.bytes);
    }

    #[test]
    fn decodes_png_file_and_reencodes_as_jpeg() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("SingleFile")
            .join("src")
            .join("ui")
            .join("resources")
            .join("icon_16.png");
        let encoded = decode_image_file_to_jpeg_asset(&path, 80, true).unwrap();
        assert!(encoded.pixel_width > 0);
        assert!(encoded.pixel_height > 0);
        assert_eq!(&encoded.bytes[0..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn decodes_svg_data_url_to_dynamic_image() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="10"><rect width="12" height="10" fill="#ff0000"/></svg>"##;
        let data_url = format!("data:image/svg+xml;base64,{}", encode_base64(svg.as_bytes()));
        let image = decode_data_url_to_dynamic_image(&data_url).unwrap();
        assert_eq!(image.width(), 12);
        assert_eq!(image.height(), 10);
    }

    fn encode_base64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);
            let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;

            out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(ALPHABET[(n & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }
}
