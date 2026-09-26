//! `.dpd`: level preload list — every object, adjective, animation, texture, vector drawing
//! and effect a level's scene uses, preloaded before the level starts
//! (`data\mapdata\{sandbox,events,collision_maps}\*.dpd`, referenced as the last entry of a
//! level link in `.stp` files and loaded through `FUN_004958b0`).
//!
//! The binary layout is identical to `.dps` dependency lists:
//!
//! ```text
//! u32 count
//! count x { u8 kind; u32 resource }
//! ```

use fmt_common::Dependencies;
use scribble_core::{Context, Format, Result};
use serde::{Deserialize, Serialize};

/// Resources preloaded for a level; see [`fmt_common::Dependencies`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PreloadList(pub Dependencies);

impl Format for PreloadList {
    const NAME: &'static str = "dpd";
    const DESCRIPTION: &'static str = "Level preload list: resources a level's scene uses";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        Ok(PreloadList(Dependencies::decode(data, ctx)?))
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        self.0.encode(ctx)
    }
}
