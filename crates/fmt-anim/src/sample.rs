//! Evaluating an [`Animation`] at a point in time, reproducing the engine's fixed-point math.
//!
//! Engine model (`FUN_006ec550` load, `FUN_006eca70` evaluate, `FUN_006ed5c0` per-tick update):
//!
//! * Time is a 20.12 fixed-point frame count (`frame << 12`). Each evaluation adds the
//!   instance's speed (`+0x18 += +0x1c`), `0x1000` (one frame per tick) by default, and the game
//!   ticks at a fixed 60 Hz (see [`FRAMES_PER_SECOND`]).
//! * The clip length is the largest final-key frame over all tracks. A looping clip wraps time
//!   into `(0, length]`; a one-shot clip clamps it to `[0, length]` and holds the last pose.
//! * Before `key[0]` a track holds its first key, from its last key on it holds the last key,
//!   in between it interpolates linearly with a per-segment slope precomputed at load time:
//!   `k = round(4096 / frames_between_keys)`, `slope = (Δvalue * k) >> 12`, and
//!   `value = key.value + ((slope * (t - key.time)) >> 12)`. The integer reciprocal makes the
//!   slope slightly inexact, which this module reproduces.
//! * Key values are offsets from the part's rest pose (the loader adds the rest pose to every
//!   key; interpolating the offsets is equivalent). The rotation result is truncated to the
//!   engine's 16-bit angle.
//!
//! `.sao` background objects use a second evaluator, `FUN_006ecdd0` (reached through
//! `FUN_006ed710` from the background object's update, vtable `0x846560`), which sets the part
//! transforms directly instead of blending, with one extra rule: a two-key rotation track of a
//! clip exactly 600 frames long (`+0x14 == 0x258000`) is driven as the absolute angle
//! `±(i16)trunc(t / 2457600.0 * 65535.0)` (sign of the interpolated offset), ignoring the rest
//! angle. It makes `ferriswheeldowntown_spin` (the only such clip) turn exactly once per loop
//! where the integer slope (`round(4096/600) = 7`) would overshoot. [`Animation::sample_background`]
//! reproduces it; the result marks such parts with [`PartPose::rotation_is_absolute`].
//!
//! A cheat toggles one more variant: typing `KO DERF` (`FUN_006ec300` flips `DAT_008b3622`)
//! makes the loader compute rotation slopes on the 16-bit-wrapped key values, except for
//! `maxwell_idle` / `maxwell_run`. Not reproduced.

use crate::anim::{Angle, Animation, Channel, Fx12, Track};

/// One frame in the engine's 20.12 fixed-point time unit.
pub const FRAME: i32 = 0x1000;

/// Frames per second a viewer should assume. The engine advances animation time by one frame
/// per game update (speed `0x1000`), and updates at a fixed 60 Hz: the main loop `FUN_006f6650`
/// divides the elapsed milliseconds by `16.666666` (`DAT_00846fac` / `DAT_00846fa0`), runs that
/// many update ticks (1 to 5) and sleeps out the rest of the 16.67 ms frame.
pub const FRAMES_PER_SECOND: f64 = 60.0;

/// Clip length (fixed-point frames) that triggers the background evaluator's spin rule.
const SPIN_CLIP_LENGTH: i32 = 600 * FRAME;

/// The animated offsets of one part at a point in time. `None` = the clip does not animate that
/// channel of the part, so it stays at its rest value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PartPose {
    /// Rotation offset from the rest angle (65536 = one turn; not yet wrapped).
    pub rotation: Option<Angle>,
    /// Position offset `(x, y)` from the rest position.
    pub translation: Option<(Fx12, Fx12)>,
    /// `rotation` is the part's absolute angle, not an offset from its rest angle (only the
    /// background evaluator's spin rule produces this, see [`Animation::sample_background`]).
    pub rotation_is_absolute: bool,
}

/// An absolute part transform in the engine's units: what the rig stores as the rest pose and
/// what an evaluated animation produces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PartTransform {
    pub x: Fx12,
    pub y: Fx12,
    /// 16-bit angle, `0..65536` (65536 = one turn).
    pub angle: Angle,
}

/// Offsets for every part at one instant, indexed by part index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pose {
    pub parts: Vec<PartPose>,
}

impl Pose {
    /// Offsets of part `index` (all `None` if the clip doesn't touch it).
    pub fn part(&self, index: usize) -> PartPose {
        self.parts.get(index).copied().unwrap_or_default()
    }

    /// Apply the offsets to a rig's rest pose (indexed by part), the way the engine does:
    /// `x = rest.x + dx`, `y = rest.y + dy`, `angle = (rest.angle + dangle) & 0xffff`
    /// (`angle & 0xffff` alone when [`PartPose::rotation_is_absolute`]).
    /// Parts the clip doesn't animate keep their rest transform.
    pub fn apply(&self, rest: &[PartTransform]) -> Vec<PartTransform> {
        rest.iter()
            .enumerate()
            .map(|(i, r)| {
                let p = self.part(i);
                let (x, y) = match p.translation {
                    Some((dx, dy)) => (Fx12(r.x.0.wrapping_add(dx.0)), Fx12(r.y.0.wrapping_add(dy.0))),
                    None => (r.x, r.y),
                };
                let angle = match p.rotation {
                    Some(d) if p.rotation_is_absolute => d.wrapped(),
                    Some(d) => Angle(r.angle.0.wrapping_add(d.0)).wrapped(),
                    None => r.angle,
                };
                PartTransform { x, y, angle }
            })
            .collect()
    }
}

/// `round(4096 / dt) ` as computed by the loader (x87: `1/dt * 4096`, ±0.5, truncate).
fn reciprocal(dt_frames: i32) -> i64 {
    if dt_frames == 0 {
        return 0;
    }
    let v = 1.0 / dt_frames as f32 as f64 * 4096.0;
    (if v > 0.0 { v + 0.5 } else { v - 0.5 }).trunc() as i64
}

fn slope(v0: i32, v1: i32, f0: u16, f1: u16) -> i32 {
    let k = reciprocal(f1 as i32 - f0 as i32);
    ((v1.wrapping_sub(v0) as i64 * k) >> 12) as i32
}

fn step(slope: i32, dt: i32) -> i32 {
    ((slope as i64 * dt as i64) >> 12) as i32
}

/// Where `t` falls among `frames`: `Hold(i)` = exactly key `i`, `Lerp(i, dt)` = between key `i`
/// and `i + 1`, `dt` fixed-point time past key `i`.
enum Seg {
    Hold(usize),
    Lerp(usize, i32),
}

fn locate(frames: &[u16], t: i32) -> Option<Seg> {
    let n = frames.len();
    let time = |i: usize| frames[i] as i32 * FRAME;
    if n == 0 {
        return None;
    }
    if t <= time(0) {
        return Some(Seg::Hold(0));
    }
    if t >= time(n - 1) {
        return Some(Seg::Hold(n - 1));
    }
    for j in 1..n {
        if time(j) == t {
            return Some(Seg::Hold(j));
        }
        if t <= time(j) {
            return Some(Seg::Lerp(j - 1, t - time(j - 1)));
        }
    }
    Some(Seg::Hold(0))
}

impl Track {
    /// This track's value at fixed-point time `t` (already wrapped to the clip), written into
    /// `pose`. Honors [`Track::loop_independently`]. `spin` enables the background evaluator's
    /// 600-frame rule (`FUN_006ecdd0`); it is only true when the clip is exactly 600 frames.
    fn evaluate_into(&self, t: i32, pose: &mut PartPose, spin: bool) {
        let clip_t = t;
        let mut t = t;
        if self.loop_independently {
            let len = self.end_frame() as i32 * FRAME;
            if len > 0 {
                while t > len {
                    t -= len;
                }
                while t < 0 {
                    t += len;
                }
            }
        }
        let frames = self.frames();
        let Some(seg) = locate(&frames, t) else { return };
        match &self.channel {
            Channel::Rotation(k) => {
                let v = match seg {
                    Seg::Hold(i) => k[i].degrees.0,
                    Seg::Lerp(i, dt) => {
                        let s = slope(k[i].degrees.0, k[i + 1].degrees.0, k[i].frame, k[i + 1].frame);
                        let delta = step(s, dt);
                        if spin && k.len() == 2 {
                            // `fild t; fdiv 2457600.0; fstp f32; fmul 65535.0; _ftol; movzx; cwde`
                            // (x87 in D3D's single-precision mode), negated when the interpolated
                            // offset is negative; the rest angle is not added.
                            let turn = (clip_t as f32 / SPIN_CLIP_LENGTH as f32) * 65535.0f32;
                            let a = turn as i32 as u16 as i16 as i32;
                            pose.rotation = Some(Angle(if delta < 0 { -a } else { a }));
                            pose.rotation_is_absolute = true;
                            return;
                        }
                        k[i].degrees.0.wrapping_add(delta)
                    }
                };
                pose.rotation = Some(Angle(v));
                pose.rotation_is_absolute = false;
            }
            Channel::Translation(k) => {
                let (x, y) = match seg {
                    Seg::Hold(i) => (k[i].x.0, k[i].y.0),
                    Seg::Lerp(i, dt) => {
                        let (a, b) = (&k[i], &k[i + 1]);
                        let sx = slope(a.x.0, b.x.0, a.frame, b.frame);
                        let sy = slope(a.y.0, b.y.0, a.frame, b.frame);
                        (a.x.0.wrapping_add(step(sx, dt)), a.y.0.wrapping_add(step(sy, dt)))
                    }
                };
                pose.translation = Some((Fx12(x), Fx12(y)));
            }
        }
    }
}

impl Animation {
    /// Clip length in fixed-point frames, as the loader computes it: the largest last-key time
    /// over all tracks (`FUN_006ec550`, instance `+0x14`), at least the `0xfff` the constructor
    /// `FUN_006ed0f0` starts it at.
    ///
    /// Note: a handful of shipped clips (bee, elephant, maxwell...) end one track with a stray
    /// key at frame 65535 (an exporter artifact), which makes the engine's clip length ~18
    /// minutes; this is reported faithfully.
    pub fn duration_fixed(&self) -> i32 {
        self.tracks.iter().map(|t| t.end_frame() as i32 * FRAME).fold(0xfff, i32::max)
    }

    /// Clip length in frames (see [`Animation::duration_fixed`]).
    pub fn duration_frames(&self) -> f64 {
        self.duration_fixed() as f64 / FRAME as f64
    }

    /// Clip length in seconds at [`FRAMES_PER_SECOND`].
    pub fn duration_seconds(&self) -> f64 {
        self.duration_frames() / FRAMES_PER_SECOND
    }

    /// Map an elapsed playback time (fixed-point frames since the clip started) to the clip's
    /// local time: wrapped into `(0, length]` when looping (0 stays 0), clamped to
    /// `[0, length]` otherwise.
    pub fn local_time_fixed(&self, elapsed: i32) -> i32 {
        let d = self.duration_fixed();
        if self.looping {
            if elapsed > d {
                (elapsed - 1) % d + 1
            } else if elapsed < 0 {
                elapsed.rem_euclid(d)
            } else {
                elapsed
            }
        } else {
            elapsed.clamp(0, d)
        }
    }

    /// Evaluate every track at clip-local fixed-point time `t` (`0..=duration_fixed()`), with
    /// the engine's exact integer arithmetic. The returned pose has one entry per part up to
    /// `max(part_count, highest animated part + 1)`.
    pub fn sample_fixed(&self, t: i32) -> Pose {
        self.evaluate(t, false)
    }

    fn evaluate(&self, t: i32, spin: bool) -> Pose {
        let n = self.tracks.iter().map(|t| t.part as usize + 1).fold(self.part_count as usize, usize::max);
        let mut pose = Pose { parts: vec![PartPose::default(); n] };
        for track in &self.tracks {
            track.evaluate_into(t, &mut pose.parts[track.part as usize], spin);
        }
        pose
    }

    /// Like [`Animation::sample_fixed`], but reproducing the evaluator `.sao` background objects
    /// use (`FUN_006ecdd0`), including its 600-frame spin rule (see the module docs).
    pub fn sample_background_fixed(&self, t: i32) -> Pose {
        self.evaluate(t, self.duration_fixed() == SPIN_CLIP_LENGTH)
    }

    /// Like [`Animation::sample`], with the background-object evaluator
    /// ([`Animation::sample_background_fixed`]).
    pub fn sample_background(&self, frames: f64) -> Pose {
        let t = (frames * FRAME as f64).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        self.sample_background_fixed(self.local_time_fixed(t))
    }

    /// Evaluate the clip `frames` frames after it started playing (fractional frames allowed),
    /// applying looping/clamping. This is the main entry point for a viewer.
    pub fn sample(&self, frames: f64) -> Pose {
        let t = (frames * FRAME as f64).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
        self.sample_fixed(self.local_time_fixed(t))
    }

    /// Evaluate the clip `seconds` after it started, at [`FRAMES_PER_SECOND`].
    pub fn sample_seconds(&self, seconds: f64) -> Pose {
        self.sample(seconds * FRAMES_PER_SECOND)
    }

    /// Frame of the first event of `kind`, if the clip has one.
    pub fn event_frame(&self, kind: crate::EventKind) -> Option<u16> {
        self.events.iter().find(|e| e.kind == kind).map(|e| e.frame)
    }
}
