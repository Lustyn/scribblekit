//! Renders a handful of vector art files to PNG in `target/vec-previews/` for eyeballing.

use fmt_vec::{atlas_dimensions, render_to_rgba, RenderOptions, VectorArt, View};
use scribble_core::Format;
use std::path::Path;

const SAMPLES: &[&str] = &[
    "mammal\\large\\cow_texture.vec",
    "maxwell_texture.vec",
    "reptile\\water\\treefrog_texture.vec",
    "audio\\instrument\\cowbell.vec",
    "organic\\bone\\cowskull.vec",
    "plants\\deciduoustree\\mimosa.vec",
    "_emotes\\emoteapple.vec",
    "_mapanim\\windmillfarm_texture.vec",
    "particles\\dirtparticlebig.vec",
    "organic\\bone\\dinosaurbones_texture.vec",
    "clothes\\head\\cowboyhat.vec",
    "_objstamps\\cowboy_head.vec",
];

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

#[test]
fn render_previews() {
    let Some(root) = scribble_core::testing::extracted_root() else { return };
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/vec-previews");
    std::fs::create_dir_all(&out).unwrap();
    let ctx = scribble_core::Context::empty();
    for s in SAMPLES {
        let path = root.join("[platform]").join("datavector").join(s.replace('\\', "/"));
        let Ok(data) = std::fs::read(&path) else { continue };
        let art = VectorArt::decode(&data, &ctx).unwrap();
        let stem = s.rsplit('\\').next().unwrap().trim_end_matches(".vec");

        // The art's texture as laid out (atlas), on white.
        let (w, h) = atlas_dimensions(&art, 512);
        let mut opts = RenderOptions { width: w, height: h, view: View::Atlas, background: [255, 255, 255, 255], ..RenderOptions::new(w, h) };
        let rgba = render_to_rgba(&art, &opts);
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        write_png(&out.join(format!("{stem}.atlas.png")), w, h, &rgba);

        // Fit to content with the part outlines drawn in red, on transparent.
        opts = RenderOptions { outline_color: Some([255, 0, 0, 255]), ..RenderOptions::new(384, 384) };
        write_png(&out.join(format!("{stem}.outline.png")), 384, 384, &render_to_rgba(&art, &opts));

        // Each part on its own, addressed by bone id, for multi-part (animated) art.
        if art.parts.len() > 1 {
            for part in &art.parts {
                assert!(art.part_mesh(part.bone).is_some());
                opts = RenderOptions { only_bone: Some(part.bone), ..RenderOptions::new(128, 128) };
                write_png(&out.join(format!("{stem}.bone{}.png", part.bone)), 128, 128, &render_to_rgba(&art, &opts));
            }
        }
    }
}
