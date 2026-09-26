//! Vector art formats.
//!
//! * `.vec` — [`VectorArt`]: the vertex-coloured, per-bone triangle meshes every object is drawn
//!   with (`[platform]\datavector\...`). See [`vec`] for the binary layout.
//!
//! [`render`] turns art into drawable triangles ([`VectorArt::draw_triangles`],
//! [`VectorArt::part_meshes`]) and rasterizes it to RGBA ([`render_to_rgba`]).

pub mod render;
pub mod vec;

pub use render::{atlas_dimensions, render_to_rgba, MeshVertex, PartMesh, RenderOptions, Triangle, View};
pub use vec::{Bounds, Color, PaletteEntry, Part, PositionEncoding, VectorArt, Vertex};

use scribble_core::{codec, Codec, ResPath};

/// The codec for a resource, if this crate handles it.
pub fn handler(path: &ResPath, _data: &[u8]) -> Option<&'static dyn Codec> {
    match path.ext.as_str() {
        "vec" => Some(codec!(VectorArt)),
        _ => None,
    }
}

/// Every codec this crate provides.
pub fn codecs() -> Vec<&'static dyn Codec> {
    vec![codec!(VectorArt)]
}
