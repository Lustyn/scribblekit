//! Level (map) formats of Scribblenauts Unlimited.
//!
//! A playable level is described by a slot in the level table ([`LevelTable`], `.lvls`) that
//! names four resources:
//!
//! * `.tle` — [`TileMap`]: art pages and the collision tile grid.
//! * `.stp` — [`LevelSetup`]: colours, music, parallax background, exits, gravity.
//! * `.sod` — [`Scene`]: placed objects with their scripts, regions, doors, lights, ...
//! * `.mdb` — [`MeritDatabase`]: merits that can be earned.
//!
//! Supporting formats:
//!
//! * `.plf` — [`ParallaxLayers`]: background layers referenced by a level setup.
//! * `.nbtc` — [`CollisionTileset`]: collision shapes of the tiles used by tile maps.
//! * `.dpd` — [`PreloadList`]: resources preloaded for a scene.
//!
//! Scenes contain the level scripts: per-object triggers with actions ([`script`]). They are
//! the same engine classes as object behaviours and decode with the shared schema of
//! [`fmt_object::behaviour`]. The scene's other small records (object bodies, liquids, lights,
//! doors, ...) are described declaratively ([`schema`]) and decoded into ordered JSON records.
//!
//! Positions are 16.16 fixed point in tile units (16 px per tile) unless noted; 20.12 fixed
//! point values (the engine's "world" scale, `0x1000` = 1.0) use [`Fx12`].

pub mod common;
pub mod dpd;
pub mod lvls;
pub mod mdb;
pub mod nbtc;
pub mod plf;
pub mod schema;
pub mod script;
pub mod sod;
pub mod stp;
pub mod tle;

pub use common::{Fx12, Rgb, Rgba};
pub use dpd::PreloadList;
pub use lvls::{Level, LevelTable};
pub use mdb::{Merit, MeritCategory, MeritDatabase};
pub use nbtc::CollisionTileset;
pub use plf::{ParallaxLayer, ParallaxLayers};
pub use sod::{Scene, SceneObject};
pub use stp::{Exit, LevelLink, LevelSetup};
pub use tle::{CollisionGrid, TileLayer, TileMap, TilePath};

use scribble_core::{codec, Codec, ResPath};

/// The codec for a resource, if this crate handles it.
pub fn handler(path: &ResPath, _data: &[u8]) -> Option<&'static dyn Codec> {
    Some(match path.ext.as_str() {
        "lvls" => codec!(LevelTable),
        "tle" => codec!(TileMap),
        "stp" => codec!(LevelSetup),
        "mdb" => codec!(MeritDatabase),
        "plf" => codec!(ParallaxLayers),
        "nbtc" => codec!(CollisionTileset),
        "dpd" => codec!(PreloadList),
        "sod" => codec!(Scene),
        _ => return None,
    })
}

/// Every codec this crate provides.
pub fn codecs() -> Vec<&'static dyn Codec> {
    vec![
        codec!(LevelTable),
        codec!(TileMap),
        codec!(LevelSetup),
        codec!(MeritDatabase),
        codec!(ParallaxLayers),
        codec!(CollisionTileset),
        codec!(PreloadList),
        codec!(Scene),
    ]
}
