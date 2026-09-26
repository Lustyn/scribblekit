//! Small formats shared across many directories.
//!
//! * `.dps` — [`Dependencies`]: the list of resources to preload alongside a resource.

pub mod dps;

pub use dps::{Dependencies, Dependency, DependencyKind};
use scribble_core::{codec, Codec, ResPath};

/// The codec for a resource, if this crate handles it.
pub fn handler(path: &ResPath, _data: &[u8]) -> Option<&'static dyn Codec> {
    match path.ext.as_str() {
        "dps" => Some(codec!(Dependencies)),
        _ => None,
    }
}

/// Every codec this crate provides.
pub fn codecs() -> Vec<&'static dyn Codec> {
    vec![codec!(Dependencies)]
}
