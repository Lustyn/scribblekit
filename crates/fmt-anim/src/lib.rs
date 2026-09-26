//! Mesh/part animation (`.anim`, `data\meshanim\...`).
//!
//! # What an animation is
//!
//! Scribblenauts rigs are 2D cut-out puppets: an object is a hierarchy of rigid vector-art
//! parts (head, legs, wings...). An [`Animation`] is a set of tracks, each driving the
//! **rotation** or the **translation** of one part with linearly interpolated keys. Values are
//! offsets from the part's rest pose. See [`anim`] for the byte layout and [`sample`] for exact
//! playback semantics.
//!
//! # Binding tracks to parts
//!
//! [`Track::part`] indexes the object's *part list* ("skeleton"), built by `FUN_0069ea20`: it
//! walks the object's scene hierarchy depth-first (`FUN_0069a060(5, prev)`) and takes every
//! mesh-part node (node type 5, constructed by `FUN_006b4760` while parsing the object's node
//! tree in `FUN_006b6c00`, each referencing a piece of the object's `.vec` art) in order.
//! So part `n` is the n-th vector-art part node of the object in depth-first order, i.e. the
//! order the parts are stored in the object's hierarchy (`FUN_0069ea20` also writes the index to
//! node `+0x5d` and to the part's render object, which uses it as the `.vec` part id). The mesh
//! part's own u16 (`MeshPart::part` in `fmt_object`) is *not* this index: it is the part's draw
//! rank (`FUN_004c5b10` sorts the parts by it). The rig size is [`Animation::part_count`], which
//! is constant for all clips of a rig directory (and ignored by the loader).
//!
//! Each part record (0x28 bytes) keeps pointers into the node's transform and a copy of its
//! rest pose: `+0x08` rest x, `+0x10` rest y (20.12 fixed point), `+0x18` rest angle (u16,
//! 65536 = one turn), `+0x1a` the part's draw-depth offset within the object. Every tick `FUN_006ed5c0` resets each part to its rest pose, then each
//! playing clip moves it to `rest + offset` (blending by the clip's 0..0x1000 weight during
//! transitions), then `FUN_006ed540` adds procedural offsets. Use [`Pose::apply`] to do the
//! same with a rest pose taken from the rig.
//!
//! # Choosing the clip
//!
//! Clips live in `data\meshanim\<category>\<rig>\<rig>_<name>.anim` (`cow_walk`, `cow_idle`,
//! ...). The engine picks them by *slot* number through the object's animation table, not by
//! name; [`slots`] documents the slot -> name mapping (14 = idle, 33 = walk, 24 = run, ...).
//!
//! # Key engine functions
//!
//! | address      | role |
//! |--------------|------|
//! | `FUN_006ec550` | parse `.anim` into an animation instance (keys + rest pose, slopes, length, events) |
//! | `FUN_006ec460` | fetch the resource (loader category 5) |
//! | `FUN_006ed0f0` | construct an animation instance (resource index, speed; deferred-load queue) |
//! | `FUN_006eca70` | evaluate one instance and blend it into the part transforms |
//! | `FUN_006ecdd0` | evaluate one instance setting transforms directly (`.sao` backgrounds, via `FUN_006ed710`; 600-frame spin rule) |
//! | `FUN_006ed5c0` | per-tick update of an animation controller (reset to rest, run layers) |
//! | `FUN_006eda70` | play a clip on a layer with a cross-fade |
//! | `FUN_00665d50` | play the clip in a given slot of the object's animation table |
//! | `FUN_00665d20` | read an event frame (`slot`, `kind`) |
//! | `FUN_0069ea20` | build the part list from the object's mesh-part nodes |
//! | `FUN_00494b60` | preload `.dps` dependencies (kind 3 -> loader category 5) |
//! | `FUN_006f6650` | main loop: fixed 60 Hz update ticks (16.67 ms, `DAT_00846fac`) |
//! | `FUN_00544460` | shoot aiming: picks a frame between the action and aim events |

pub mod anim;
pub mod sample;
pub mod slots;

pub use anim::{Angle, Animation, Channel, Event, EventKind, Fx12, RotationKey, Track, TranslationKey};
pub use sample::{PartPose, PartTransform, Pose, FRAME, FRAMES_PER_SECOND};
pub use slots::{slot_for_file, slot_name, SLOT_NAMES};

use scribble_core::{Codec, ResPath};

/// The codec for a resource, if this crate handles it.
pub fn handler(path: &ResPath, _data: &[u8]) -> Option<&'static dyn Codec> {
    (path.ext == "anim").then(|| scribble_core::codec!(Animation))
}

/// Every codec this crate provides.
pub fn codecs() -> Vec<&'static dyn Codec> {
    vec![scribble_core::codec!(Animation)]
}
