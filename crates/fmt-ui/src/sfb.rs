//! `.sfb`: sprite-flipbook description for a texture strip — which rectangles of the texture are
//! animation cells, the frame sequence and the named loops. Used by map-art animations,
//! effects, particles and a few menu spinners; the pixels live in the matching `.[texture]`.
//!
//! Reader: `FUN_006ef060` (keeps pointers into the loaded bytes); frames are played by
//! `FUN_006ef6c0`, drawn by `FUN_006ef330` / `FUN_006ef9d0`.
//!
//! ```text
//! u16 frame_flags        // bits 0-1 add u16s to every frame record (0 in shipped files)
//! u16 cell_count
//! cell_count x { u16 x, u16 y, u16 width, u16 height }       // texture rectangles
//! u16 frame_count
//! frame_count x { u16 cell, u16 duration, i16 origin_x, i16 origin_y, u16 extra[..] }
//! u16 animation_count
//! animation_count x { u16 looping, u16 first_frame, u16 last_frame }
//! ```
//!
//! `origin_*` is the offset of the cell's top-left corner from the sprite's anchor (always
//! minus half the cell size in shipped files); `duration` is in game ticks.

use scribble_core::{ensure, Context, Format, Reader, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpriteFrames {
    /// Frame record layout (`FUN_006ef060`: stride 4 u16, +2 if bit 0, +1 if bit 1, 7 if both;
    /// the stride is kept at +0x1a and used by every frame access). Other bits are not read.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub frame_flags: u16,
    /// Texture rectangles `[x, y, width, height]` (`FUN_006ef330` reads them as the quad's
    /// source rectangle).
    pub cells: Vec<[u16; 4]>,
    pub frames: Vec<Frame>,
    pub animations: Vec<Animation>,
}

fn is_zero(v: &u16) -> bool {
    *v == 0
}

/// Extra u16s per frame for `frame_flags`.
fn extra_len(flags: u16) -> usize {
    (if flags & 1 != 0 { 2 } else { 0 }) + (if flags & 2 != 0 { 1 } else { 0 })
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Index into `cells` (`FUN_006ef330`).
    pub cell: u16,
    /// Ticks the frame is shown (`FUN_006ef6c0` adds `duration << 12` to the frame timer).
    pub duration: u16,
    /// Cell offset from the anchor (`FUN_006ef9d0`).
    pub origin: [i16; 2],
    /// The u16s added by `frame_flags` (none in shipped files; the frame accessors
    /// `FUN_006ef330`, `FUN_006ef6c0`, `FUN_006ef9d0` read only the first four).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    /// Non-zero: restart at `first_frame` after `last_frame`; 0: stop on the last frame
    /// (`FUN_006ef6c0`). Stored as u16.
    pub looping: u16,
    /// Frame range; played backwards when `first_frame > last_frame` (`FUN_006ef6c0` steps
    /// towards the last frame from either side).
    pub first_frame: u16,
    pub last_frame: u16,
}

impl Format for SpriteFrames {
    const NAME: &'static str = "sfb";
    const DESCRIPTION: &'static str = "Sprite flipbook: texture cells, frames and animation loops";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let frame_flags = r.u16()?;
        let extra = extra_len(frame_flags);
        let n = r.u16()?;
        let cells = (0..n).map(|_| Ok([r.u16()?, r.u16()?, r.u16()?, r.u16()?])).collect::<Result<_>>()?;
        let n = r.u16()?;
        let frames = (0..n)
            .map(|_| {
                Ok(Frame {
                    cell: r.u16()?,
                    duration: r.u16()?,
                    origin: [r.i16()?, r.i16()?],
                    extra: (0..extra).map(|_| r.u16()).collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?;
        let n = r.u16()?;
        let animations = (0..n)
            .map(|_| Ok(Animation { looping: r.u16()?, first_frame: r.u16()?, last_frame: r.u16()? }))
            .collect::<Result<_>>()?;
        r.expect_end()?;
        Ok(SpriteFrames { frame_flags, cells, frames, animations })
    }

    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        let extra = extra_len(self.frame_flags);
        let mut w = Writer::new();
        w.u16(self.frame_flags).u16(self.cells.len() as u16);
        for c in &self.cells {
            for v in c {
                w.u16(*v);
            }
        }
        w.u16(self.frames.len() as u16);
        for f in &self.frames {
            ensure!(f.extra.len() == extra, "frame_flags {} needs {extra} extra values per frame", self.frame_flags);
            w.u16(f.cell).u16(f.duration).i16(f.origin[0]).i16(f.origin[1]);
            for v in &f.extra {
                w.u16(*v);
            }
        }
        w.u16(self.animations.len() as u16);
        for a in &self.animations {
            w.u16(a.looping).u16(a.first_frame).u16(a.last_frame);
        }
        Ok(w.into_inner())
    }
}
