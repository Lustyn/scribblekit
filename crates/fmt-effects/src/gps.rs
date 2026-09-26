//! `.gps`: GIGL particle system (`gigl\prtcl\System.cpp`) — a set of materials (shader +
//! textures) and emitters whose spawn parameters are keyframed random ranges.
//!
//! Loader: `FUN_007a8d80` (system), `FUN_007b0d60` (material), `FUN_007aa2b0` (emitter),
//! `FUN_007a9b80` (curve). Defaults for a new emitter are in `FUN_007aafb0`; the per-particle
//! spawn code that consumes every curve is `FUN_007aa530`, emission is `FUN_007ab280`.
//! Particles are then animated in closed form on the GPU by `[platform]\gpuprograms\determined.gp`
//! (position, rotation and size each follow `x0 + v/d*(1-e^(-d*age)) + a/d*age + ...`).
//!
//! ```text
//! u32 version                      // 1, 3, 4, 5 or 7 in shipped files
//! u32 material_count
//! material_count x {
//!     u32 id                       // key the emitters use to pick their material
//!     u32 unused_capacity          // read into a local and dropped (usually = max_particles)
//!     if version < 6 {
//!         u32 n
//!         n x particle             // saved particle vertices, 25 f32 (versions 0,1,4) or 26 f32
//!     }                            // (others, FUN_007b1680); read raw and freed by the loader
//!     u32 len; u8 simulation[len]  // "Determined" (GPU closed form) or "Discreet" (CPU stepped)
//!     AssetRef program             // kind 1, e.g. [PLATFORM]/gpuprograms/determined.gp
//!     AssetRef texture             // kind 2, the particle sprite ("detailTexture")
//!     AssetRef lookup_texture      // kind 2, colour/alpha over lifetime ("lookupTexture")
//! }
//! u32 emitter_count
//! emitter_count x {
//!     u32 material                 // a material id
//!     u32 len; u8 name[len]
//!     u8  discrete                 // +0x18: CPU-stepped simulation (matches "Discreet" material)
//!     u8  looping                  // +0x19: restart after `duration`
//!     if version > 2 {
//!         u8  prewarm              // +0x1a: simulate a full cycle before first shown
//!         u32 draw_order           // +0x1c: low 16 bits of the sort key
//!         u16 max_particles        // +0x20: particle pool size (default 64)
//!     }
//!     if version > 6 { u8 unscaled_motion }   // +0x1b: speed/forces ignore the emitter's scale
//!     f32 duration_min, duration_max          // +0x28: emitter cycle length (seconds)
//!     17 x Curve                              // +0x30, 0x44 bytes each, see EmitterCurves
//!     f32 uv_offset[2], uv_scale[2]           // +0x4b4: sprite sub-rectangle in the texture
//!     if version > 4 { u16 lookup_row }       // +0x4c4: row of the lookup texture
//! }
//! Curve = u32 key_count (always 8; FUN_007a9b80 reads it into a local and always reads 8 keys)
//!         + 8 x { f32 min, f32 max }
//! ```
//!
//! Version checks: `FUN_007b0d60` (material, `< 6` saved particles), `FUN_007aa2b0` (emitter:
//! `> 2`, `> 6`, `> 4`). The version is passed on to both readers by `FUN_007a8d80`.

use crate::asset::{AssetKind, AssetRef};
use crate::float::{from_json, to_json};
use scribble_core::{bail, ensure, Context, Format, Reader, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParticleSystem {
    /// File version; decides which optional fields are stored (tested by `FUN_007b0d60` and
    /// `FUN_007aa2b0`, see the module docs).
    pub version: u32,
    pub materials: Vec<Material>,
    pub emitters: Vec<Emitter>,
}

/// Rendering setup shared by an emitter's particles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// Identifier referenced by [`Emitter::material`]: `FUN_007a8d80` stores each material in a
    /// map under this key and looks emitters' `material` up in it
    /// (`_wassert("iter != map.end()", System.cpp:0x130)`).
    pub id: u32,
    /// Particle capacity recorded by the editor (= the emitter's `max_particles` in 751 of 808
    /// emitters). Never used: `FUN_007b0d60` reads it into a local (`local_24`) it never touches
    /// again; the emitter's `max_particles` sizes the pool.
    pub unused_capacity: u32,
    /// Versions < 6 only: particle vertices saved with the file. `FUN_007b0d60` reads
    /// `n * FUN_007b1680(version)` bytes (100 for versions 0/1/4, else 104) into a temporary
    /// buffer and frees it. The records are the vertices `FUN_007aa530` builds (the
    /// `determined.gp` inputs): mass[4], position[2], velocity[2], force[2], drag,
    /// rotation[4] (angle, angular velocity, 0, angular drag), scale[4] (size, growth, 0,
    /// growth drag), birth_time, lifetime, uv_offset[2], uv_scale[2]; version 5 appends
    /// lookup_pos, versions 2/3 start with one extra float (0 in all 162 shipped records).
    /// Checked on shipped data: force == mass.x * (0, ±9.8) + mass.y * (5, 0).
    #[serde(default, skip_serializing_if = "Vec::is_empty", with = "crate::float::rows")]
    pub unused_saved_particles: Vec<Vec<f32>>,
    pub simulation: Simulation,
    /// GPU program (`.gp`).
    pub program: AssetRef,
    /// Particle sprite texture (`detailTexture` in the shader).
    pub texture: AssetRef,
    /// Gradient sampled by (age / lifetime, lookup_row): colour and alpha over the particle's life
    /// (`lookupTexture`).
    pub lookup_texture: AssetRef,
}

/// How particles are simulated: the material type name, looked up by `FUN_007a3890` among the
/// types the game registers in `FUN_00604dc0` (`"Determined"` -> `FUN_00604390`,
/// `"Discreet"` -> `FUN_00604300`, which steps particles on the CPU in `FUN_00602760`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Simulation {
    /// Closed-form motion evaluated in the vertex shader.
    Determined,
    /// Stepped ("discrete") simulation; the emitter's `discrete` flag is set with it.
    Discreet,
    #[serde(untagged)]
    Other(String),
}

impl Simulation {
    fn from_name(s: String) -> Self {
        match s.as_str() {
            "Determined" => Simulation::Determined,
            "Discreet" => Simulation::Discreet,
            _ => Simulation::Other(s),
        }
    }
    fn name(&self) -> &str {
        match self {
            Simulation::Determined => "Determined",
            Simulation::Discreet => "Discreet",
            Simulation::Other(s) => s,
        }
    }
}

/// One particle emitter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Emitter {
    /// [`Material::id`] of the material to draw with.
    pub material: u32,
    /// Name other resources (`.gec`, `.trns`) use to instantiate this emitter.
    pub name: String,
    /// `+0x18`: particles are stepped on the CPU instead of evaluated in closed form —
    /// `FUN_007aa530` skips the spawn-time force (`FUN_007a3860`) and integrates position,
    /// angle and size itself when it is set. Matches `Simulation::Discreet` in every shipped file.
    pub discrete: bool,
    /// `+0x19`: restart when the cycle (`duration`) ends (`FUN_007ab280` returns when clear);
    /// bit 31 of the draw sort key (`FUN_007a8d80`).
    pub looping: bool,
    /// Version > 2, `+0x1a`: a looping emitter starts already a full cycle in (`FUN_007a6220`
    /// emits for `-duration` when `looping && prewarm`); bit 30 of the draw sort key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prewarm: Option<bool>,
    /// Version > 2, `+0x1c`: draw sort key, low 16 bits (`FUN_007a8d80` builds
    /// `order & 0xffff | prewarm << 30 | looping << 31`; high bits 0 in all shipped files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draw_order: Option<u32>,
    /// Version > 2, `+0x20`: particle pool size (default 64, `FUN_007ab7d0`; passed to
    /// `FUN_007b0210` by `FUN_007a8d80`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_particles: Option<u16>,
    /// Version > 6, `+0x1b`: `FUN_007a99f0` sets the instance's motion factor to `1 / scale`
    /// instead of 1, which multiplies gravity/wind weights and speed in `FUN_007aa530`, so
    /// particle motion does not grow with the effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unscaled_motion: Option<bool>,
    /// `+0x28`: cycle length in seconds, picked uniformly in `[min, max]`. Curves are sampled
    /// over it (`FUN_007aa530`: key = (t - start) / duration, clamped to 0..7).
    #[serde(with = "crate::float::pair")]
    pub duration: [f32; 2],
    /// `+0x4b4`: top-left of the sprite's sub-rectangle in `texture` (UV units; copied into the
    /// particle vertex `vertexTexcoord.xy` by `FUN_007aa530`).
    #[serde(with = "crate::float::pair")]
    pub uv_offset: [f32; 2],
    /// `+0x4bc`: size of the sprite's sub-rectangle in `texture` (UV units, `vertexTexcoord.zw`).
    #[serde(with = "crate::float::pair")]
    pub uv_scale: [f32; 2],
    /// Version > 4, `+0x4c4`: row of `lookup_texture` used — `FUN_007aa530` writes
    /// `(row % rows + 0.5) / rows` as `vertexLookupPos`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookup_row: Option<u16>,
    pub curves: EmitterCurves,
}

/// The emitter's spawn parameters. Each [`Curve`] has 8 keys spread evenly over the emitter's
/// cycle; at spawn time the current key's `[min, max]` range is sampled uniformly.
/// Offsets `+0x30 + 0x44*i` in the emitter (keys at `+4`); angles are radians, distances world
/// units. Every curve except `emission_rate` (read by `FUN_007ab280`) is sampled by the spawn
/// function `FUN_007aa530` into the particle vertex that `determined.gp` evaluates. The three
/// drag curves are divisors in the shader and are clamped to >= 0.01 on load (`FUN_007aa160`
/// on `+0x294`, `+0x360`, `+0x42c` in `FUN_007aa2b0`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmitterCurves {
    /// `i=0`: particles per second (default 3; `FUN_007ab280`).
    pub emission_rate: Curve,
    /// `i=1`: `mass.x`, times the emitter scale. The force callback `FUN_00600dc0` (slot `+0x10`
    /// of the particle environment `FUN_00604dc0`) returns `mass.x * (0, 9.8) + mass.y * (5, 0)`.
    pub gravity_scale: Curve,
    /// `i=2`: `mass.y`, weight of the wind force (5, 0), times the emitter scale.
    pub wind_scale: Curve,
    /// `i=3`: sampled into `mass.z` (`vertexMass.z`) but never used: `FUN_00600dc0` reads only
    /// `mass[0..1]` (and returns 0 unless given >= 2 components), and neither `determined.gp`
    /// nor `simulated.gp` references `vertexMass`. 0 or 1 in shipped files.
    pub unused_mass_z: Curve,
    /// `i=4`: sampled into `mass.w` (`vertexMass.w`), never used (as `unused_mass_z`).
    pub unused_mass_w: Curve,
    /// `i=5`: spawn position X relative to the emitter (transformed by the emitter matrix).
    pub offset_x: Curve,
    /// `i=6`: spawn position Y relative to the emitter.
    pub offset_y: Curve,
    /// `i=7`: initial speed (default 3), times the motion factor.
    pub speed: Curve,
    /// `i=8`: direction of the initial velocity (radians, emitter space; `cos`/`sin`).
    pub direction: Curve,
    /// `i=9`: linear velocity damping (default 0.1) — `vertexUnused.z`, the `d` of the shader's
    /// `x0 + v/d*(1-e^(-d*age)) + ...`.
    pub drag: Curve,
    /// `i=10`: initial sprite angle (default ±π) — `vertexRotation.x`.
    pub rotation: Curve,
    /// `i=11`: initial angular velocity (default ±0.1π) — `vertexRotation.y`.
    pub angular_velocity: Curve,
    /// `i=12`: angular velocity damping (default 0.1) — `vertexRotation.w`.
    pub angular_drag: Curve,
    /// `i=13`: initial size (default 3..5), times the emitter scale — `vertexScale.x`.
    pub size: Curve,
    /// `i=14`: initial growth rate (default 1..2), times the emitter scale — `vertexScale.y`.
    pub growth: Curve,
    /// `i=15`: growth damping (default 0.1) — `vertexScale.w`.
    pub growth_drag: Curve,
    /// `i=16`: particle lifetime in seconds (default 10..13) — `vertexLifetime.y`.
    pub lifetime: Curve,
}

/// Eight `[min, max]` keys over the emitter's cycle.
///
/// JSON: a single number when every key is the same fixed value, a `[min, max]` pair when every
/// key is the same range, otherwise the list of 8 pairs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CurveRepr", into = "CurveRepr")]
pub struct Curve(pub [[f32; 2]; 8]);

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum CurveRepr {
    Constant(f64),
    Range([f64; 2]),
    Keys(Vec<[f64; 2]>),
}

fn key_to_json(k: [f32; 2]) -> [f64; 2] {
    [to_json(k[0]), to_json(k[1])]
}

fn key_from_json(k: [f64; 2]) -> [f32; 2] {
    [from_json(k[0]), from_json(k[1])]
}

impl From<Curve> for CurveRepr {
    fn from(c: Curve) -> Self {
        let bits = |k: [f32; 2]| [k[0].to_bits(), k[1].to_bits()];
        let first = c.0[0];
        if c.0.iter().all(|&k| bits(k) == bits(first)) {
            if first[0].to_bits() == first[1].to_bits() {
                CurveRepr::Constant(to_json(first[0]))
            } else {
                CurveRepr::Range(key_to_json(first))
            }
        } else {
            CurveRepr::Keys(c.0.iter().map(|&k| key_to_json(k)).collect())
        }
    }
}

impl TryFrom<CurveRepr> for Curve {
    type Error = String;
    fn try_from(r: CurveRepr) -> Result<Self, String> {
        Ok(Curve(match r {
            CurveRepr::Constant(v) => [[from_json(v); 2]; 8],
            CurveRepr::Range(k) => [key_from_json(k); 8],
            CurveRepr::Keys(v) => {
                let keys: Vec<_> = v.into_iter().map(key_from_json).collect();
                keys.try_into().map_err(|v: Vec<_>| format!("a curve needs 8 keys, got {}", v.len()))?
            }
        }))
    }
}

const KEYS: u32 = 8;

impl Curve {
    fn read(r: &mut Reader) -> Result<Self> {
        let n = r.u32()?;
        ensure!(n == KEYS, "curve key count {n} at {:#x} (expected {KEYS})", r.pos() - 4);
        let mut keys = [[0f32; 2]; 8];
        for k in &mut keys {
            *k = [r.f32()?, r.f32()?];
        }
        Ok(Curve(keys))
    }
    fn write(&self, w: &mut Writer) {
        w.u32(KEYS);
        for k in &self.0 {
            w.f32(k[0]).f32(k[1]);
        }
    }
}

macro_rules! curves {
    ($($f:ident),*) => {
        impl EmitterCurves {
            fn read(r: &mut Reader) -> Result<Self> {
                Ok(EmitterCurves { $($f: Curve::read(r)?),* })
            }
            fn write(&self, w: &mut Writer) {
                $(self.$f.write(w);)*
            }
        }
    };
}
curves!(
    emission_rate, gravity_scale, wind_scale, unused_mass_z, unused_mass_w, offset_x, offset_y, speed, direction, drag, rotation,
    angular_velocity, angular_drag, size, growth, growth_drag, lifetime
);

/// Floats per saved particle for pre-6 versions (`FUN_007b1680`).
fn saved_particle_floats(version: u32) -> usize {
    match version {
        0 | 1 | 4 => 25,
        _ => 26,
    }
}

/// The value to write for a field that exists iff `present` (missing values take `default`).
fn opt<T: Copy>(v: Option<T>, present: bool, what: &str, default: T) -> Result<Option<T>> {
    if present {
        Ok(Some(v.unwrap_or(default)))
    } else if v.is_some() {
        bail!("{what} cannot be stored in this file version")
    } else {
        Ok(None)
    }
}

impl Format for ParticleSystem {
    const NAME: &'static str = "gps";
    const DESCRIPTION: &'static str = "Particle system: materials and keyframed particle emitters";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let version = r.u32()?;
        let n = r.u32()?;
        let mut materials = Vec::new();
        for _ in 0..n {
            let id = r.u32()?;
            let unused_capacity = r.u32()?;
            let mut unused_saved_particles = Vec::new();
            if version < 6 {
                let count = r.u32()?;
                let floats = saved_particle_floats(version);
                for _ in 0..count {
                    unused_saved_particles.push((0..floats).map(|_| r.f32()).collect::<Result<Vec<_>>>()?);
                }
            }
            let simulation = Simulation::from_name(r.str_u32()?);
            let program = AssetRef::read(&mut r, AssetKind::Program, ctx)?;
            let texture = AssetRef::read(&mut r, AssetKind::Texture, ctx)?;
            let lookup_texture = AssetRef::read(&mut r, AssetKind::Texture, ctx)?;
            materials.push(Material { id, unused_capacity, unused_saved_particles, simulation, program, texture, lookup_texture });
        }
        let n = r.u32()?;
        let mut emitters = Vec::new();
        for _ in 0..n {
            let material = r.u32()?;
            let name = r.str_u32()?;
            let discrete = r.bool()?;
            let looping = r.bool()?;
            let (mut prewarm, mut draw_order, mut max_particles) = (None, None, None);
            if version > 2 {
                prewarm = Some(r.bool()?);
                draw_order = Some(r.u32()?);
                max_particles = Some(r.u16()?);
            }
            let unscaled_motion = if version > 6 { Some(r.bool()?) } else { None };
            let duration = [r.f32()?, r.f32()?];
            let curves = EmitterCurves::read(&mut r)?;
            let uv_offset = [r.f32()?, r.f32()?];
            let uv_scale = [r.f32()?, r.f32()?];
            let lookup_row = if version > 4 { Some(r.u16()?) } else { None };
            emitters.push(Emitter {
                material,
                name,
                discrete,
                looping,
                prewarm,
                draw_order,
                max_particles,
                unscaled_motion,
                duration,
                uv_offset,
                uv_scale,
                lookup_row,
                curves,
            });
        }
        r.expect_end()?;
        Ok(ParticleSystem { version, materials, emitters })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let v = self.version;
        let mut w = Writer::new();
        w.u32(v).u32(self.materials.len() as u32);
        for m in &self.materials {
            w.u32(m.id).u32(m.unused_capacity);
            if v < 6 {
                let floats = saved_particle_floats(v);
                w.u32(m.unused_saved_particles.len() as u32);
                for p in &m.unused_saved_particles {
                    ensure!(p.len() == floats, "version {v} saved particles have {floats} floats, got {}", p.len());
                    p.iter().for_each(|&f| {
                        w.f32(f);
                    });
                }
            } else {
                ensure!(m.unused_saved_particles.is_empty(), "unused_saved_particles need version < 6");
            }
            w.str_u32(m.simulation.name())?;
            m.program.write(&mut w, AssetKind::Program, ctx)?;
            m.texture.write(&mut w, AssetKind::Texture, ctx)?;
            m.lookup_texture.write(&mut w, AssetKind::Texture, ctx)?;
        }
        w.u32(self.emitters.len() as u32);
        for e in &self.emitters {
            w.u32(e.material);
            w.str_u32(&e.name)?;
            w.bool(e.discrete).bool(e.looping);
            if let Some(p) = opt(e.prewarm, v > 2, "prewarm", false)? {
                w.bool(p);
            }
            if let Some(d) = opt(e.draw_order, v > 2, "draw_order", 0)? {
                w.u32(d);
            }
            if let Some(m) = opt(e.max_particles, v > 2, "max_particles", 64)? {
                w.u16(m);
            }
            if let Some(u) = opt(e.unscaled_motion, v > 6, "unscaled_motion", false)? {
                w.bool(u);
            }
            w.f32(e.duration[0]).f32(e.duration[1]);
            e.curves.write(&mut w);
            w.f32(e.uv_offset[0]).f32(e.uv_offset[1]).f32(e.uv_scale[0]).f32(e.uv_scale[1]);
            if let Some(l) = opt(e.lookup_row, v > 4, "lookup_row", 0)? {
                w.u16(l);
            }
        }
        Ok(w.into_inner())
    }
}
