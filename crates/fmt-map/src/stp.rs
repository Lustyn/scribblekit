//! `.stp`: level setup — sky/ground colours, music, parallax background, exits to the
//! neighbouring levels, special textures and gravity.
//!
//! Loaded by `FUN_004b24d0` (level descriptor +0x08). Everything is optional and gated by a
//! flag word; fields follow in this order. The loader only tests bits 0-2, 4-13 and 15-18
//! (bit 14 only to skip its word); bit 19's value is never read.
//!
//! ```text
//! u32 flags
//! bit14: u32 unused_prefix                   (skipped: `iVar7 = local_15c + 8`)
//! bit0 : u8 r, g, b      sky colour          (-> +0xb8, shader uSkyColor)
//! bit1 : u32 parallax    .plf                (-> background object +0x14)
//! bit2 : i16 top, bottom unused water rows   (-> +0x7b88/+0x7b8a, never read)
//! bit4 : u8 unused; u8 n; n x u32 music      (.wav playlist; at most 8 are used)
//! bit5 : LevelLink exit_left
//! bit6 : LevelLink exit_right
//! bit7 : u32 exit_left arrival script
//! bit8 : u32 exit_right arrival script
//! bit9 : LevelLink exit_up
//! bit10: LevelLink exit_down
//! bit11: u32 exit_up arrival script
//! bit12: u32 exit_down arrival script
//! bit13: u8 exits whose arrival offset is kept from the top/left edge (bit n = exit n;
//!        default 0x0f, -> exit table +0x2c4)
//! bit15: i32 x, y        unused start position (16.16 tiles, -> +0x7cf8, never read)
//! bit16: u8 unused; i8 n; n x { i8 slot; u32 texture }   environment textures (FUN_004dfbb0)
//! bit17: i32 gravity     (20.12, default 1.0, -> +0x7d00)
//! bit18: u8 r, g, b      ground colour       (/255 -> +0xc0.. floats, shader uGroundColor)
//! bit19: u32 .trns       level title transition (never read by the engine)
//!
//! LevelLink = u32 scene (.sod), setup (.stp), tile_map (.tle), merits (.mdb),
//!             dependencies (.dps), preload (.dpd)       -- read by FUN_004ae8a0
//! ```
//!
//! Exits are numbered 0 left, 1 right, 2 up, 3 down: `FUN_00641650` builds exit 0's trigger box
//! at `x = 0x14000` (1.25 tiles from the left edge), exit 1 at `width - 0x14000`, exit 2 at
//! `y = 0x14000` (top) and exit 3 at `height - 0x14000`; on arrival `FUN_005f14a0` puts the
//! player at the opposite edge (exit 0 -> `x = width - 1/16`, exit 1 -> `x = 0x1000`, ...).

use crate::common::{Fx12, Rgb};
use scribble_core::{bail, ensure, Context, Format, Fx16, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelSetup {
    /// Flag bit 14: a word right after the flags that `FUN_004b24d0` skips
    /// (`if ((bVar3 & 0x40) != 0) iVar7 = local_15c + 8;`). Never set in shipped files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_prefix: Option<u32>,
    /// Sky colour: packed to 15 bits into +0xb8, expanded to floats +0xa8..+0xb4 and bound to
    /// the shader uniform `uSkyColor` (`FUN_0061b000`, uniform table `FUN_0061f0c0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sky_color: Option<Rgb>,
    /// Parallax background layers (`.plf`), stored on the level's background object (+0x14).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallax: Option<ResRef>,
    /// Flag bit 2: `[top, bottom]` tile rows stored at +0x7b88/+0x7b8a (a value equal to the map
    /// height becomes 0xFFFF). Nothing reads them (only `FUN_004b24d0` and the reset
    /// `FUN_0049fd50` touch the offsets). In the data they bound the water: top is 0 in the
    /// underwater maps and the liquid's top row in coastal ones; bottom is always the height.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_water_rows: Option<[i16; 2]>,
    /// Music playlist: track 0 -> +0x194, the rest -> +0x198.., count clamped to 8 at +0x1b8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub music: Option<Playlist>,
    /// Exit 0, through the left edge (trigger box at `x = 0x14000`, `FUN_00641650`).
    #[serde(default, skip_serializing_if = "Exit::is_empty")]
    pub exit_left: Exit,
    /// Exit 1, through the right edge (`x = width - 0x14000`).
    #[serde(default, skip_serializing_if = "Exit::is_empty")]
    pub exit_right: Exit,
    /// Exit 2, through the top (`y = 0x14000`; only created when `FUN_004a3e30` unlocks the
    /// target scene), e.g. from under water to the surface.
    #[serde(default, skip_serializing_if = "Exit::is_empty")]
    pub exit_up: Exit,
    /// Exit 3, through the bottom (`y = height - 0x14000`; same unlock gate).
    #[serde(default, skip_serializing_if = "Exit::is_empty")]
    pub exit_down: Exit,
    /// Exits (`left`/`right`/`up`/`down`, unknown bits `bit_N`) whose arrival keeps the
    /// player's offset from the top/left edge. `FUN_00641eb0` passes `(flags >> exit) & 1` to
    /// `FUN_004ad1d0` (transition +0x4c); when clear the saved offset is `height - y` /
    /// `width - x` and `FUN_005f14a0` measures it from the bottom/right edge of the new map.
    /// Default when absent is all four (`c_beach` clears `down`, `c_reef` clears `up`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_left_aligned_exits: Option<Vec<String>>,
    /// Flag bit 15: a position in tiles stored at +0x7cf8/+0x7cfc; never read (only written by
    /// `FUN_004b24d0` and the reset `FUN_0049fd50`). Looks like a start position in the data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_start_position: Option<[Fx16; 2]>,
    /// Textures of the level's background object (`FUN_004dfbb0`, run on background +0xb0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_textures: Option<EnvironmentTextures>,
    /// Gravity multiplier (+0x7d00, default 1.0; `FUN_004bfe00`:
    /// `DAT_00897f80 = gravity * 0x28f`, the physics gravity per step). 0.5 on the moon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gravity: Option<Fx12>,
    /// Ground colour: `/255` into floats +0xc0..+0xc8, bound to the shader uniform
    /// `uGroundColor` (`FUN_0061b000`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground_color: Option<Rgb>,
    /// Flag bit 19: a `.trns` transition (the level-name `env_trans` effects). The loader never
    /// tests bit 19 and nothing else reads the setup, so the engine ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_title_transition: Option<ResRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    /// Leading byte, skipped by the loader (the count is read from byte 1:
    /// `uVar15 = *(byte *)(local_15c + 1 + ...)`; always 0).
    #[serde(default, skip_serializing_if = "crate::common::is_zero_u8")]
    pub unused: u8,
    pub tracks: Vec<ResRef>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Exit {
    /// The level this exit leads to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<LevelLink>,
    /// Event script run on arrival instead of placing the player at the edge: the exit trigger
    /// keeps it at +0x11c, `FUN_00641eb0` hands it to the transition (+0x48) and
    /// `FUN_005f14a0` runs it (`FUN_004afb10(script)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrival_script: Option<ResRef>,
}

impl Exit {
    pub fn is_empty(&self) -> bool {
        self.level.is_none() && self.arrival_script.is_none()
    }
}

/// The resources of another level (`FUN_004ae8a0` reads the six words; the order matches the
/// level descriptor, `FUN_004e1a50`). `dependencies` and `preload` are streamed in ahead
/// (`FUN_004958b0`) and kept for the exit taken (`FUN_00641eb0`, others freed by
/// `FUN_00640e10`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelLink {
    pub scene: ResRef,
    pub setup: ResRef,
    pub tile_map: ResRef,
    pub merits: ResRef,
    /// Dependency list preloaded for the level (`.dps`).
    pub dependencies: ResRef,
    /// Preload list (`.dpd`).
    pub preload: ResRef,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentTextures {
    /// Leading byte, skipped by `FUN_004dfbb0` (`*param_3 = *param_3 + 1` before the count;
    /// always 0).
    #[serde(default, skip_serializing_if = "crate::common::is_zero_u8")]
    pub unused: u8,
    /// Slot assignments in file order (`FUN_004dfbb0`: slot i -> `param_1[i+1]`, texture
    /// preloaded into `param_1[i+7]` except for slot 5).
    pub slots: Vec<EnvironmentTexture>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentTexture {
    pub slot: EnvironmentSlot,
    pub texture: Option<ResRef>,
}

/// Texture slots of the background object (`FUN_004dfbb0`; fetched by `FUN_004dfad0` /
/// `FUN_004dfa70`, which are only ever called with 3, 4 and 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentSlot {
    /// Forced to none by the loader (`if ((iVar8 == 0) || (iVar8 == 1)) param_1[iVar8+1] = -1`).
    #[serde(rename = "unused_0")]
    Unused0,
    /// Forced to none, like slot 0.
    #[serde(rename = "unused_1")]
    Unused1,
    /// Stored and preloaded but never fetched (no accessor call with 2).
    #[serde(rename = "unused_2")]
    Unused2,
    /// Water mask (`<level>_mask_wat`): `FUN_00738080` binds it for water.gp with UV scale
    /// `1/(width*16)`, `1/(height*16)`; none falls back to blanktexture.
    WaterMask,
    /// Reflection texture (`<theme>_reflect_NN`): `FUN_0061b000` binds `FUN_004dfa70(4)` to the
    /// uniform `uReflectionTexture`.
    Reflection,
    /// A `.vec` drawing rendered with vectormeshoccluder.gp over the whole map
    /// (`FUN_00449340`: `FUN_004dfad0(5)` -> `FUN_005afa80(vec, 0x420)`); not preloaded.
    ShadowOccluder,
    #[serde(untagged)]
    Other(i8),
}

impl EnvironmentSlot {
    fn from_i8(v: i8) -> Self {
        use EnvironmentSlot::*;
        match v {
            0 => Unused0,
            1 => Unused1,
            2 => Unused2,
            3 => WaterMask,
            4 => Reflection,
            5 => ShadowOccluder,
            v => Other(v),
        }
    }
    fn to_i8(self) -> i8 {
        use EnvironmentSlot::*;
        match self {
            Unused0 => 0,
            Unused1 => 1,
            Unused2 => 2,
            WaterMask => 3,
            Reflection => 4,
            ShadowOccluder => 5,
            Other(v) => v,
        }
    }
}

const EXIT_FLAG_NAMES: [&str; 4] = ["left", "right", "up", "down"];

pub(crate) fn flag_names(v: u32, names: &[&str], bits: u32) -> Vec<String> {
    (0..bits)
        .filter(|b| v >> b & 1 != 0)
        .map(|b| match names.get(b as usize) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => format!("bit_{b}"),
        })
        .collect()
}

pub(crate) fn flag_value(list: &[String], names: &[&str]) -> Result<u32> {
    let mut v = 0u32;
    for s in list {
        let bit = if let Some(i) = names.iter().position(|n| !n.is_empty() && n == s) {
            i as u32
        } else if let Some(n) = s.strip_prefix("bit_") {
            n.parse::<u32>().map_err(|_| anyhow::anyhow!("bad flag {s:?}"))?
        } else {
            bail!("unknown flag {s:?} (expected one of {names:?} or bit_N)");
        };
        ensure!(bit < 32, "flag bit {bit} out of range");
        v |= 1 << bit;
    }
    Ok(v)
}

fn read_link(r: &mut Reader, ctx: &Context) -> Result<LevelLink> {
    let mut f = || -> Result<ResRef> { Ok(ResRef::from_index(r.u32()?, ctx)) };
    Ok(LevelLink { scene: f()?, setup: f()?, tile_map: f()?, merits: f()?, dependencies: f()?, preload: f()? })
}

fn write_link(w: &mut Writer, l: &LevelLink, ctx: &Context) -> Result<()> {
    for r in [&l.scene, &l.setup, &l.tile_map, &l.merits, &l.dependencies, &l.preload] {
        w.u32(r.to_index(ctx)?);
    }
    Ok(())
}

fn read_rgb(r: &mut Reader) -> Result<Rgb> {
    Ok(Rgb(r.array()?))
}

const KNOWN_FLAGS: u32 = 0x000F_FFF7;

impl Format for LevelSetup {
    const NAME: &'static str = "stp";
    const DESCRIPTION: &'static str = "Level setup: colours, music, parallax background, exits and physics of a level";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let flags = r.u32()?;
        ensure!(flags & !KNOWN_FLAGS == 0, "unknown setup flags {flags:#x}");
        let has = |b: u32| flags >> b & 1 != 0;
        let res = |r: &mut Reader| -> Result<ResRef> { Ok(ResRef::from_index(r.u32()?, ctx)) };
        let unused_prefix = if has(14) { Some(r.u32()?) } else { None };
        let sky_color = if has(0) { Some(read_rgb(&mut r)?) } else { None };
        let parallax = if has(1) { Some(res(&mut r)?) } else { None };
        let unused_water_rows = if has(2) { Some([r.i16()?, r.i16()?]) } else { None };
        let music = if has(4) {
            let unused = r.u8()?;
            let n = r.u8()?;
            let tracks = (0..n).map(|_| res(&mut r)).collect::<Result<_>>()?;
            Some(Playlist { unused, tracks })
        } else {
            None
        };
        let mut exits: [Exit; 4] = Default::default();
        if has(5) {
            exits[0].level = Some(read_link(&mut r, ctx)?);
        }
        if has(6) {
            exits[1].level = Some(read_link(&mut r, ctx)?);
        }
        if has(7) {
            exits[0].arrival_script = Some(res(&mut r)?);
        }
        if has(8) {
            exits[1].arrival_script = Some(res(&mut r)?);
        }
        if has(9) {
            exits[2].level = Some(read_link(&mut r, ctx)?);
        }
        if has(10) {
            exits[3].level = Some(read_link(&mut r, ctx)?);
        }
        if has(11) {
            exits[2].arrival_script = Some(res(&mut r)?);
        }
        if has(12) {
            exits[3].arrival_script = Some(res(&mut r)?);
        }
        let top_left_aligned_exits = if has(13) { Some(flag_names(r.u8()? as u32, &EXIT_FLAG_NAMES, 8)) } else { None };
        let unused_start_position = if has(15) { Some([Fx16(r.i32()?), Fx16(r.i32()?)]) } else { None };
        let environment_textures = if has(16) {
            let unused = r.u8()?;
            let n = r.i8()?;
            let mut slots = Vec::new();
            for _ in 0..n.max(0) {
                let slot = EnvironmentSlot::from_i8(r.i8()?);
                let tex = r.u32()?;
                slots.push(EnvironmentTexture { slot, texture: crate::common::opt_res(tex, ctx) });
            }
            ensure!(n >= 0, "negative environment texture count");
            Some(EnvironmentTextures { unused, slots })
        } else {
            None
        };
        let gravity = if has(17) { Some(Fx12(r.i32()?)) } else { None };
        let ground_color = if has(18) { Some(read_rgb(&mut r)?) } else { None };
        let unused_title_transition = if has(19) { Some(res(&mut r)?) } else { None };
        r.expect_end()?;
        let [exit_left, exit_right, exit_up, exit_down] = exits;
        Ok(LevelSetup {
            unused_prefix,
            sky_color,
            parallax,
            unused_water_rows,
            music,
            exit_left,
            exit_right,
            exit_up,
            exit_down,
            top_left_aligned_exits,
            unused_start_position,
            environment_textures,
            gravity,
            ground_color,
            unused_title_transition,
        })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let bit = |b: bool, n: u32| (b as u32) << n;
        let flags = bit(self.sky_color.is_some(), 0)
            | bit(self.parallax.is_some(), 1)
            | bit(self.unused_water_rows.is_some(), 2)
            | bit(self.music.is_some(), 4)
            | bit(self.exit_left.level.is_some(), 5)
            | bit(self.exit_right.level.is_some(), 6)
            | bit(self.exit_left.arrival_script.is_some(), 7)
            | bit(self.exit_right.arrival_script.is_some(), 8)
            | bit(self.exit_up.level.is_some(), 9)
            | bit(self.exit_down.level.is_some(), 10)
            | bit(self.exit_up.arrival_script.is_some(), 11)
            | bit(self.exit_down.arrival_script.is_some(), 12)
            | bit(self.top_left_aligned_exits.is_some(), 13)
            | bit(self.unused_prefix.is_some(), 14)
            | bit(self.unused_start_position.is_some(), 15)
            | bit(self.environment_textures.is_some(), 16)
            | bit(self.gravity.is_some(), 17)
            | bit(self.ground_color.is_some(), 18)
            | bit(self.unused_title_transition.is_some(), 19);
        let mut w = Writer::new();
        w.u32(flags);
        if let Some(v) = self.unused_prefix {
            w.u32(v);
        }
        if let Some(c) = &self.sky_color {
            w.bytes(&c.0);
        }
        if let Some(p) = &self.parallax {
            w.u32(p.to_index(ctx)?);
        }
        if let Some([a, b]) = self.unused_water_rows {
            w.i16(a).i16(b);
        }
        if let Some(m) = &self.music {
            w.u8(m.unused).u8(u8::try_from(m.tracks.len())?);
            for t in &m.tracks {
                w.u32(t.to_index(ctx)?);
            }
        }
        let script = |w: &mut Writer, s: &Option<ResRef>| -> Result<()> {
            if let Some(s) = s {
                w.u32(s.to_index(ctx)?);
            }
            Ok(())
        };
        if let Some(l) = &self.exit_left.level {
            write_link(&mut w, l, ctx)?;
        }
        if let Some(l) = &self.exit_right.level {
            write_link(&mut w, l, ctx)?;
        }
        script(&mut w, &self.exit_left.arrival_script)?;
        script(&mut w, &self.exit_right.arrival_script)?;
        if let Some(l) = &self.exit_up.level {
            write_link(&mut w, l, ctx)?;
        }
        if let Some(l) = &self.exit_down.level {
            write_link(&mut w, l, ctx)?;
        }
        script(&mut w, &self.exit_up.arrival_script)?;
        script(&mut w, &self.exit_down.arrival_script)?;
        if let Some(f) = &self.top_left_aligned_exits {
            w.u8(u8::try_from(flag_value(f, &EXIT_FLAG_NAMES)?)?);
        }
        if let Some([x, y]) = self.unused_start_position {
            w.i32(x.0).i32(y.0);
        }
        if let Some(t) = &self.environment_textures {
            w.u8(t.unused).i8(i8::try_from(t.slots.len())?);
            for s in &t.slots {
                w.i8(s.slot.to_i8()).u32(crate::common::opt_res_index(&s.texture, ctx)?);
            }
        }
        if let Some(g) = self.gravity {
            w.i32(g.0);
        }
        if let Some(c) = &self.ground_color {
            w.bytes(&c.0);
        }
        script(&mut w, &self.unused_title_transition)?;
        Ok(w.into_inner())
    }
}
