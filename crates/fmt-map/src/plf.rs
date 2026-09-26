//! `.plf`: parallax layer file — the scrolling background layers of a level
//! (`data\players\*.plf`, referenced by a level setup).
//!
//! Loaded by `FUN_006f3180`. Each layer is either a horizontally tiled background strip
//! (`FUN_005b56e0`) or a single positioned sprite (`FUN_005b5980`, taken when
//! `(flags & 1) == 0 && flags & 2`). Layers with bit 2 set create nothing but still use up a
//! depth slot (depth = layer index + 2). Only flag bits 0-3 are tested.
//!
//! ```text
//! u8 layer_count
//! layer_count x {
//!     u8  flags       // bit0 force strip, bit1 has x (sprite), bit2 disabled, bit3 has scale
//!     u32 texture     // 0xFFFFFFFF = none
//!     u16 strip_height // strip quad height (passed on as height*4); unused by sprites
//!     i32 distance    // 20.12 parallax distance factor (larger = further away, scrolls slower)
//!     -- if flags & 2: i16 x
//!     i16 y_offset    // strip: vertical offset; sprite: world y
//!     -- if flags & 8: i32 scale_x, scale_y   (20.12, default 1.0; sprites only)
//! }
//! ```

use crate::common::{opt_res, opt_res_index, Fx12};
use scribble_core::{ensure, Context, Format, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParallaxLayers {
    pub layers: Vec<ParallaxLayer>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParallaxLayer {
    /// Background texture (`null` = none).
    pub texture: Option<ResRef>,
    /// Flag bit 0: make a strip even when `x` is given (`FUN_006f3180` takes the sprite path
    /// only when `(bVar1 & 1) == 0`). Set on one layer, which has no `x`.
    #[serde(default, skip_serializing_if = "crate::common::is_false")]
    pub force_strip: bool,
    /// Flag bit 2: no renderer is created for the layer (`if ((bVar1 >> 2 & 1) == 0)
    /// {...create...}`), but it still takes a depth slot.
    #[serde(default, skip_serializing_if = "crate::common::is_false")]
    pub disabled: bool,
    /// Flag bits 4..7: never tested by the loader (never set in shipped files).
    #[serde(default, skip_serializing_if = "crate::common::is_zero_u8")]
    pub unused_flags: u8,
    /// Strip quad height: `FUN_005b56e0` -> `FUN_005af5e0(tex, ..., screen_w*2, height*4, ...)`.
    /// Read but not passed on for sprites.
    pub strip_height: u16,
    /// Parallax distance. Strips: texture u = camera x / distance (`FUN_005b60b0`; 1..16 in
    /// the data). Sprites (+0x510): position moves by f(distance) of the camera motion
    /// (`FUN_005b5a10`; 1.0 = fixed to the camera, 0.1..0.3 in the data).
    pub distance: Fx12,
    /// Flag bit 1: world x of a sprite layer (`(short)x << 12` -> `FUN_005b5980`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i16>,
    /// Strips: vertical offset (`*4` -> +0x514, subtracted in `FUN_005b60b0`); sprites: world y
    /// (`y << 12`).
    pub y_offset: i16,
    /// Flag bit 3: `[x, y]` scale of a sprite (+0x14/+0x18, width and height multipliers in
    /// `FUN_005b5a10`); read and ignored for strips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[Fx12; 2]>,
}

impl Format for ParallaxLayers {
    const NAME: &'static str = "plf";
    const DESCRIPTION: &'static str = "Parallax background layers of a level";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let n = r.u8()?;
        let mut layers = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let flags = r.u8()?;
            let texture = opt_res(r.u32()?, ctx);
            let strip_height = r.u16()?;
            let distance = Fx12(r.i32()?);
            let x = if flags & 2 != 0 { Some(r.i16()?) } else { None };
            let y_offset = r.i16()?;
            let scale = if flags & 8 != 0 { Some([Fx12(r.i32()?), Fx12(r.i32()?)]) } else { None };
            layers.push(ParallaxLayer {
                texture,
                force_strip: flags & 1 != 0,
                disabled: flags & 4 != 0,
                unused_flags: flags & 0xf0,
                strip_height,
                distance,
                x,
                y_offset,
                scale,
            });
        }
        r.expect_end()?;
        Ok(ParallaxLayers { layers })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u8(u8::try_from(self.layers.len())?);
        for l in &self.layers {
            ensure!(l.unused_flags & 0x0f == 0, "unused_flags may only hold bits 4..7");
            let flags = l.force_strip as u8 | (l.x.is_some() as u8) << 1 | (l.disabled as u8) << 2 | (l.scale.is_some() as u8) << 3 | l.unused_flags;
            w.u8(flags).u32(opt_res_index(&l.texture, ctx)?).u16(l.strip_height).i32(l.distance.0);
            if let Some(x) = l.x {
                w.i16(x);
            }
            w.i16(l.y_offset);
            if let Some([sx, sy]) = l.scale {
                w.i32(sx.0).i32(sy.0);
            }
        }
        Ok(w.into_inner())
    }
}
