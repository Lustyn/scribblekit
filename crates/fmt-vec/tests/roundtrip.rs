use fmt_vec::vec::{dequantize, quantize};

#[test]
fn all_vec_files() {
    scribble_core::testing::check_all::<fmt_vec::VectorArt>(|p| p.ext == "vec");
}

#[test]
fn quantization_is_bijective() {
    for q in i16::MIN..=i16::MAX {
        assert_eq!(quantize(dequantize(q)).unwrap(), q);
    }
}

#[test]
fn color_json() {
    let c: fmt_vec::Color = serde_json::from_str("\"#e79cc6\"").unwrap();
    assert_eq!(c.0, 0xFFE7_9CC6);
    let c: fmt_vec::Color = serde_json::from_str("\"#e7c69c00\"").unwrap();
    assert_eq!(c.0, 0x00E7_C69C);
    assert_eq!(serde_json::to_string(&c).unwrap(), "\"#e7c69c00\"");
}
