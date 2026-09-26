//! `.trns`: screen transition (level/area change): a full-screen wipe drawn with
//! `[platform]\gpuprograms\transition.gp` plus particle emitters placed around the screen.
//!
//! Reader `FUN_007ad400`, writer `FUN_007ac5f0`, shader parameter lookup `FUN_007acc70`,
//! start `FUN_007ac3e0`, per-frame draw `FUN_007ac1d0`.
//!
//! ```text
//! u32 count
//! count x {
//!     u32 len; u8 emitter[len]   // emitter name in the transition particle system
//!     f32 x, y                   // screen-relative: spawned at (x * width, height * (1 + y))
//! }
//! AssetRef program       // kind 1, the transition shader
//! AssetRef front_image   // kind 2, wipe texture ("frontImage")
//! AssetRef back_image    // kind 2, texture faded in behind it ("backImage")
//! f32 time_start         // +0xac "timeStart"; overwritten with the clock when started
//! f32 wipe_speed         // +0x94 "speedWipe"
//! f32 fade_time          // +0x98 "timeFade"
//! f32 fade_start         // +0x9c "timeFadeStart"
//! f32 emitter_stop_delay // +0xa0 seconds into the outro before the emitters are stopped
//! f32 outro_duration     // +0xa4 length of the outro phase
//! ```

use crate::asset::{AssetKind, AssetRef};
use scribble_core::{Context, Format, Reader, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub emitters: Vec<TransitionEmitter>,
    /// The transition shader (`transition.gp`; asset kind 1, stream slot `+0x60`, `FUN_007ad400`).
    pub program: AssetRef,
    /// Wipe image scrolled across the screen (shader texture `frontImage`, `FUN_007acc70`).
    pub front_image: AssetRef,
    /// Image faded in behind the wipe (shader texture `backImage`).
    pub back_image: AssetRef,
    /// `+0xac`, shader parameter `timeStart` (name bound in `FUN_007acc70`): the clock time the
    /// transition began. Overwritten by the start function `FUN_007ac3e0`; the stored value is
    /// whatever the editor's clock was.
    #[serde(with = "crate::float::scalar")]
    pub time_start: f32,
    /// `+0x94`, shader parameter `speedWipe` (set per frame by `FUN_007ac1d0`): horizontal
    /// texture scroll speed of the wipe (UV units per second).
    #[serde(with = "crate::float::scalar")]
    pub wipe_speed: f32,
    /// `+0x98`, shader parameter `timeFade`: seconds the back image takes to fade in.
    #[serde(with = "crate::float::scalar")]
    pub fade_time: f32,
    /// `+0x9c`, shader parameter `timeFadeStart`: seconds after the start before the fade begins.
    #[serde(with = "crate::float::scalar")]
    pub fade_start: f32,
    /// `+0xa0`: seconds into the outro phase before the particle emitters are stopped
    /// (`FUN_007ac1d0` compares it with `now - outro_start`).
    #[serde(with = "crate::float::scalar")]
    pub emitter_stop_delay: f32,
    /// `+0xa4`: length of the outro phase in seconds; the transition ends when
    /// `1 - (now - outro_start) / outro_duration` drops below 0 (`FUN_007ac1d0`).
    #[serde(with = "crate::float::scalar")]
    pub outro_duration: f32,
}

/// A particle emitter started with the transition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransitionEmitter {
    /// Emitter name in the transition particle system (string, slot `+0x04`).
    pub emitter: String,
    /// Screen-relative position (two f32, slot `+0x34`): spawned at `(x * width, height * (1 + y))`.
    #[serde(with = "crate::float::pair")]
    pub position: [f32; 2],
}

impl Format for Transition {
    const NAME: &'static str = "trns";
    const DESCRIPTION: &'static str = "Screen transition: wipe shader, images, timing and particle emitters";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let n = r.u32()?;
        let mut emitters = Vec::new();
        for _ in 0..n {
            emitters.push(TransitionEmitter { emitter: r.str_u32()?, position: [r.f32()?, r.f32()?] });
        }
        let t = Transition {
            emitters,
            program: AssetRef::read(&mut r, AssetKind::Program, ctx)?,
            front_image: AssetRef::read(&mut r, AssetKind::Texture, ctx)?,
            back_image: AssetRef::read(&mut r, AssetKind::Texture, ctx)?,
            time_start: r.f32()?,
            wipe_speed: r.f32()?,
            fade_time: r.f32()?,
            fade_start: r.f32()?,
            emitter_stop_delay: r.f32()?,
            outro_duration: r.f32()?,
        };
        r.expect_end()?;
        Ok(t)
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u32(self.emitters.len() as u32);
        for e in &self.emitters {
            w.str_u32(&e.emitter)?;
            w.f32(e.position[0]).f32(e.position[1]);
        }
        self.program.write(&mut w, AssetKind::Program, ctx)?;
        self.front_image.write(&mut w, AssetKind::Texture, ctx)?;
        self.back_image.write(&mut w, AssetKind::Texture, ctx)?;
        w.f32(self.time_start).f32(self.wipe_speed).f32(self.fade_time).f32(self.fade_start);
        w.f32(self.emitter_stop_delay).f32(self.outro_duration);
        Ok(w.into_inner())
    }
}
