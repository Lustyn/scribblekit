//! Scribble objects, adjectives and object metadata.
//!
//! * `.so`  — [`ScribbleObject`]: a spawnable noun (properties, relationships, container
//!   contents, behaviours/actions, the node tree with its vector art, mesh parts, physics shapes
//!   and hotspots, animation and sound tables, dependencies). Loader: `FUN_006bbd70`.
//! * `.sao` — [`SimpleObject`]: background decoration (node tree + idle animation).
//!   Loader: `FUN_006e8e40`.
//! * `.sa`  — [`Adjective`]: property modifiers, added behaviours, conflicting adjectives.
//!   Loader: `FUN_006507c0`.
//! * `.odt` — [`ObjectDetailsTable`]: per-object metadata flags (`scribbleobject.odt`).
//!
//! # How an object is drawn
//!
//! An object's appearance is its node tree ([`tree::Node`], `Body::root`). Vector nodes
//! ([`tree::VectorNode`]) name a `.vec` (often a `*_texture.vec` atlas); their mesh-part
//! children ([`tree::MeshPart`]) each take the atlas piece whose bone id is the part's tree-order
//! index (`FUN_0069ea20` stores it at `+0x548`; the stored `draw_order` only sorts parts,
//! `FUN_004c5b10`) and draw it on a quad (`corners[i].position`, sampling `corners[i].uv`) placed at the node's
//! `x`/`y`/`angle`, relative to its parent. Mesh parts in tree order form the part list that
//! `.anim` tracks index. The animation table (`Body::animations`) maps animation slots (idle,
//! walk, ...) to `.anim` resources.

pub mod behaviour;
pub mod deps;
pub mod odt;
pub mod record;
pub mod refs;
pub mod sa;
pub mod sao;
pub mod so;
pub mod tree;
mod util;

pub use behaviour::{Action, Behaviour, Modifier};
pub use odt::{ObjectDetails, ObjectDetailsTable};
pub use sa::Adjective;
pub use sao::SimpleObject;
pub use so::{Body, ScribbleObject};
pub use tree::{Node, NodeKind};

use scribble_core::{codec, Codec, ResPath};

/// The codec for a resource, if this crate handles it.
pub fn handler(path: &ResPath, _data: &[u8]) -> Option<&'static dyn Codec> {
    match path.ext.as_str() {
        "so" => Some(codec!(ScribbleObject)),
        "sao" => Some(codec!(SimpleObject)),
        "sa" => Some(codec!(Adjective)),
        "odt" if path.file == "scribbleobject.odt" => Some(codec!(ObjectDetailsTable)),
        _ => None,
    }
}

/// Every codec this crate provides.
pub fn codecs() -> Vec<&'static dyn Codec> {
    vec![codec!(ScribbleObject), codec!(SimpleObject), codec!(Adjective), codec!(ObjectDetailsTable)]
}
