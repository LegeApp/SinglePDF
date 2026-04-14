use singlepdf::compression::encode_jpx::encode_rgb8_asset;
use singlepdf::profile::JpxForegroundMode;

#[test]
fn repeat_jp2_encode_in_process() {
    let width = 64u32;
    let height = 64u32;
    let rgb = vec![128u8; (width * height * 3) as usize];
    for _ in 0..50 {
        let asset = encode_rgb8_asset(&rgb, width, height, &JpxForegroundMode::Lossless)
            .expect("encode_rgb8_asset");
        assert!(!asset.bytes.is_empty());
    }
}

