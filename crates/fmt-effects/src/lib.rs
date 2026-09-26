//! Effect and effect-adjacent formats.
//!
//! * `.gps` — [`ParticleSystem`]: GIGL particle system (materials + keyframed emitters).
//! * `.gec` — [`EmitterCollection`]: an "effect": emitters from one `.gps`, placed at offsets.
//! * `.trns` — [`Transition`]: full-screen wipe transition with particle emitters.
//! * `.exf` — [`CustomFilter`]: list of object / adjective resources (`data\customfilters`).
//! * `.aaf` — [`AudioMetadata`]: per-sound `AudioItem` protobufs (type, volume, routing).
//!
//! `.gps`, `.gec` and `.trns` are dependency kind 6 (effect) in `.dps` lists; they are
//! serialized by the third-party GIGL particle library (`ThirdParty\gigl\prtcl`) through a
//! virtual stream (`vtable 0x0083d9d8`: `+0x04` string, `+0x2c` vec2, `+0x34` f32, `+0x40`/`+0x44`
//! u32, `+0x48` u16, `+0x58` u8, `+0x5c` raw bytes, `+0x60` [`AssetRef`]).

pub mod aaf;
pub mod asset;
pub mod exf;
mod float;
pub mod gec;
pub mod gps;
pub mod trns;

pub use aaf::{AudioItem, AudioMetadata, SoundType};
pub use asset::AssetRef;
pub use exf::CustomFilter;
pub use gec::{EmitterCollection, PlacedEmitter};
pub use gps::{Curve, Emitter, EmitterCurves, Material, ParticleSystem, Simulation};
pub use trns::{Transition, TransitionEmitter};

use scribble_core::{codec, Codec, ResPath};

/// The codec for a resource, if this crate handles it.
pub fn handler(path: &ResPath, _data: &[u8]) -> Option<&'static dyn Codec> {
    match path.ext.as_str() {
        "gps" => Some(codec!(ParticleSystem)),
        "gec" => Some(codec!(EmitterCollection)),
        "trns" => Some(codec!(Transition)),
        "exf" => Some(codec!(CustomFilter)),
        "aaf" => Some(codec!(AudioMetadata)),
        _ => None,
    }
}

/// Every codec this crate provides.
pub fn codecs() -> Vec<&'static dyn Codec> {
    vec![
        codec!(ParticleSystem),
        codec!(EmitterCollection),
        codec!(Transition),
        codec!(CustomFilter),
        codec!(AudioMetadata),
    ]
}
