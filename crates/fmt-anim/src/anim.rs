//! `.anim`: keyframed part ("mesh") animation for a creature/human/vehicle rig.
//!
//! Loaded by `FUN_006ec550` (the animation-instance loader; the resource is fetched with loader
//! category 5, which is what `.dps` dependency kind 3 maps to in `FUN_00494b60`).
//!
//! ```text
//! u8  looping            // 0 = play once and hold the last pose, 1 = wrap around (+0x2a)
//! u8  part_count         // size of the rig the clip was exported for (skipped by the loader)
//! u8  track_count        // (+0x28)
//! track_count x {
//!     u8  channel        // 0x20 = rotation, 0x40 = translation (key stride 12 / 20 in memory)
//!     u8  part           // index into the rig's part list (see crate docs)
//!     u8  loop_track     // == 1: this track loops on its own length (never set in shipped files)
//!     u8  key_count
//!     key_count x {
//!         u16 frame      // key time in frames (engine keeps time as 20.12 fixed point: frame << 12)
//!         rotation:    i32 angle     // offset from the part's rest angle, 65536 = one full turn
//!         translation: i32 x, i32 y  // offset from the part's rest position, 20.12 fixed point
//!     }
//! }
//! u8  event_count
//! event_count x {
//!     u8  kind           // 0 = action frame, 1 = aim-down frame, 2 = aim-up frame
//!     u16 frame          // stored at instance +0x30 + 2 * kind
//! }
//! ```
//!
//! The loader reads byte 0 (`looping`), skips byte 1 (`part_count`) and starts the track list at
//! byte 3 (`FUN_006ec550`: `iVar6 = 3`). Keys are interpolated linearly (see [`crate::sample`]).
//! The clip length is the latest final key over all tracks.

use scribble_core::{bail, ensure, Context, Format, Reader, Result, Writer};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A whole `.anim` clip.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    /// Whether playback wraps around at the end (idle/walk/fly...) or holds the last pose
    /// (attack/death/jump...). Stored at `+0x2a` of the engine's animation instance.
    pub looping: bool,
    /// Number of parts in the rig the clip was authored for. Not read by the loader
    /// (`FUN_006ec550` skips byte 1), but constant per rig directory; tracks only reference parts
    /// `< part_count`.
    pub part_count: u8,
    /// Animated channels, in file order (the engine applies them in this order, so a later
    /// track for the same part and channel wins).
    pub tracks: Vec<Track>,
    /// Timing markers the game logic reads (e.g. the frame an attack connects).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
}

/// One animated channel of one part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Part (bone) index in the rig; see the crate docs for how parts are numbered.
    pub part: u8,
    /// Loop this track on its own last-key time instead of the clip length: the evaluators
    /// (`FUN_006eca70`, `FUN_006ecdd0`) wrap the clip time by the track's last key time when this
    /// byte `== 1`. No shipped file sets it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub loop_independently: bool,
    #[serde(flatten)]
    pub channel: Channel,
}

/// Which transform component a track drives, with its keys (sorted by frame).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Rotation offset from the part's rest angle.
    Rotation(Vec<RotationKey>),
    /// Position offset from the part's rest position.
    Translation(Vec<TranslationKey>),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RotationKey {
    pub frame: u16,
    /// Offset from the rest angle, in degrees (exactly `raw * 360 / 65536`).
    pub degrees: Angle,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TranslationKey {
    pub frame: u16,
    /// Offset from the rest x position (20.12 fixed point).
    pub x: Fx12,
    /// Offset from the rest y position (20.12 fixed point).
    pub y: Fx12,
}

/// A timing marker. The loader copies it into the animation instance (`+0x30 + 2 * kind`), but
/// game logic reads the per-slot copy in the object's animation table instead
/// (`FUN_00665d20(slot, kind)`; the `.so` stores `[action, aim_up, aim_down]`, which
/// `FUN_00665c00` reorders to kind order `[action, aim_down, aim_up]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub kind: EventKind,
    pub frame: u16,
}

/// What an [`Event`] marks. Kinds 0..2 are what the engine initialises (`+0x30 = -1`,
/// `+0x34 = 0xffff` in `FUN_006ec550`) and reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// The frame the action happens: attack/melee hit, bite in `eat`, object used in `fiddle`
    /// (`FUN_00541e50`), release in `throw`, grab in `pickup` (`FUN_0053f710`). In `shoot`
    /// it is the level-aim frame.
    Action,
    /// Kind 1, `shoot` only: the frame posed for aiming straight down. `FUN_00544460` takes
    /// `a = atan2(dy, dx)` to the target (y down, mirrored when the target is behind) and plays
    /// the frame `action + a / (pi/2) * (aim_down - action)` when `a > 0` (target below). In
    /// `bipedanimation_shoot` this is frame 0, with the arm hanging down.
    AimDown,
    /// Kind 2, `shoot` only: the frame posed for aiming straight up, used the same way when the
    /// target is above (`a <= 0`); `bipedanimation_shoot` raises the arm by frame 20.
    AimUp,
    /// Any other marker. Twelve shipped clips carry a kind 3 (`maxwell_shoot`, `dog_throw`,
    /// `dog_swimidle` and nine `*_swimairattack`); the loader stores it at `+0x36` of the instance,
    /// and no reader of the instance's event copy was found (the `.so` tables hold only kinds 0-2).
    #[serde(untagged)]
    Other(u8),
}

impl EventKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => EventKind::Action,
            1 => EventKind::AimDown,
            2 => EventKind::AimUp,
            v => EventKind::Other(v),
        }
    }
    pub fn to_u8(self) -> u8 {
        match self {
            EventKind::Action => 0,
            EventKind::AimDown => 1,
            EventKind::AimUp => 2,
            EventKind::Other(v) => v,
        }
    }
}

const CHANNEL_ROTATION: u8 = 0x20;
const CHANNEL_TRANSLATION: u8 = 0x40;

fn is_false(b: &bool) -> bool {
    !*b
}

// ---------------------------------------------------------------------------------------------
// Value types

/// A binary angle: raw `i32` where 65536 = one full turn (the engine keeps part angles as u16).
/// Serialized as degrees; every raw value maps to an exactly representable decimal.
/// Deserialization rounds to the nearest raw step (360/65536 degrees).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Angle(pub i32);

impl Angle {
    /// Raw units per full turn.
    pub const TURN: i32 = 0x10000;
    pub fn degrees(self) -> f64 {
        self.0 as f64 * 45.0 / 8192.0
    }
    pub fn radians(self) -> f64 {
        self.0 as f64 * std::f64::consts::TAU / Self::TURN as f64
    }
    pub fn from_degrees(deg: f64) -> Option<Self> {
        let raw = (deg * 8192.0 / 45.0).round();
        (raw.is_finite() && raw >= i32::MIN as f64 && raw <= i32::MAX as f64).then_some(Angle(raw as i32))
    }
    /// The angle reduced to the engine's 16-bit range `0..65536`.
    pub fn wrapped(self) -> Angle {
        Angle(self.0 & 0xffff)
    }
}

impl fmt::Debug for Angle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}deg", self.degrees())
    }
}

impl Serialize for Angle {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_f64(self.degrees())
    }
}

impl<'de> Deserialize<'de> for Angle {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = f64::deserialize(d)?;
        Angle::from_degrees(v).ok_or_else(|| serde::de::Error::custom(format!("angle {v} out of range")))
    }
}

/// Signed 20.12 fixed-point number (raw `i32 / 4096`), the engine's unit for positions, blend
/// weights and animation time. Serialized as an exact decimal; deserialization rounds to the
/// nearest 1/4096.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Fx12(pub i32);

impl Fx12 {
    pub const ONE: Fx12 = Fx12(0x1000);
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / 4096.0
    }
    pub fn from_f64(v: f64) -> Option<Self> {
        let raw = (v * 4096.0).round();
        (raw.is_finite() && raw >= i32::MIN as f64 && raw <= i32::MAX as f64).then_some(Fx12(raw as i32))
    }
}

impl fmt::Debug for Fx12 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_f64())
    }
}

impl Serialize for Fx12 {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_f64(self.to_f64())
    }
}

impl<'de> Deserialize<'de> for Fx12 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = f64::deserialize(d)?;
        Fx12::from_f64(v).ok_or_else(|| serde::de::Error::custom(format!("{v} out of 20.12 fixed-point range")))
    }
}

// ---------------------------------------------------------------------------------------------
// Codec

impl Track {
    /// Key frames of this track, in order.
    pub fn frames(&self) -> Vec<u16> {
        match &self.channel {
            Channel::Rotation(k) => k.iter().map(|k| k.frame).collect(),
            Channel::Translation(k) => k.iter().map(|k| k.frame).collect(),
        }
    }
    /// Number of keys.
    pub fn len(&self) -> usize {
        match &self.channel {
            Channel::Rotation(k) => k.len(),
            Channel::Translation(k) => k.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Frame of the last key (0 for an empty track).
    pub fn end_frame(&self) -> u16 {
        match &self.channel {
            Channel::Rotation(k) => k.last().map_or(0, |k| k.frame),
            Channel::Translation(k) => k.last().map_or(0, |k| k.frame),
        }
    }
}

impl Format for Animation {
    const NAME: &'static str = "anim";
    const DESCRIPTION: &'static str = "Keyframed rotation/translation animation of a rig's parts";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let looping = r.bool()?;
        let part_count = r.u8()?;
        let track_count = r.u8()?;
        let mut tracks = Vec::with_capacity(track_count as usize);
        for _ in 0..track_count {
            let channel = r.u8()?;
            let part = r.u8()?;
            let loop_independently = r.bool()?;
            let n = r.u8()? as usize;
            let channel = match channel {
                CHANNEL_ROTATION => Channel::Rotation(
                    (0..n).map(|_| Ok(RotationKey { frame: r.u16()?, degrees: Angle(r.i32()?) })).collect::<Result<_>>()?,
                ),
                CHANNEL_TRANSLATION => Channel::Translation(
                    (0..n)
                        .map(|_| Ok(TranslationKey { frame: r.u16()?, x: Fx12(r.i32()?), y: Fx12(r.i32()?) }))
                        .collect::<Result<_>>()?,
                ),
                c => bail!("unknown track channel {c:#04x} at {:#x}", r.pos() - 4),
            };
            tracks.push(Track { part, loop_independently, channel });
        }
        let event_count = r.u8()?;
        let events = (0..event_count)
            .map(|_| Ok(Event { kind: EventKind::from_u8(r.u8()?), frame: r.u16()? }))
            .collect::<Result<_>>()?;
        r.expect_end()?;
        Ok(Animation { looping, part_count, tracks, events })
    }

    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.bool(self.looping).u8(self.part_count);
        w.u8(u8::try_from(self.tracks.len()).map_err(|_| scribble_core::anyhow!("more than 255 tracks"))?);
        for t in &self.tracks {
            let n = u8::try_from(t.len()).map_err(|_| scribble_core::anyhow!("track for part {} has more than 255 keys", t.part))?;
            let channel = match t.channel {
                Channel::Rotation(_) => CHANNEL_ROTATION,
                Channel::Translation(_) => CHANNEL_TRANSLATION,
            };
            w.u8(channel).u8(t.part).bool(t.loop_independently).u8(n);
            match &t.channel {
                Channel::Rotation(keys) => {
                    for k in keys {
                        w.u16(k.frame).i32(k.degrees.0);
                    }
                }
                Channel::Translation(keys) => {
                    for k in keys {
                        w.u16(k.frame).i32(k.x.0).i32(k.y.0);
                    }
                }
            }
        }
        ensure!(self.events.len() <= 255, "more than 255 events");
        w.u8(self.events.len() as u8);
        for e in &self.events {
            w.u8(e.kind.to_u8()).u16(e.frame);
        }
        Ok(w.into_inner())
    }
}
