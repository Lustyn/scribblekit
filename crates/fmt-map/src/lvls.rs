//! `.lvls`: the level table (`data\mapdata\s3 default level table.lvls`, pmindex 8179).
//!
//! Loaded by `FUN_004e1300(0x1ff3)` into a global array of 0x38-byte level descriptors
//! (`DAT_008ad438`). A descriptor bundles the four resources that make up a playable level:
//! the tile map (`.tle`), the level setup (`.stp`), the scene (`.sod`) and the merit list
//! (`.mdb`). Slots are addressed by index (the world map uses fixed slot numbers), so empty
//! slots are kept. Descriptor +0x10 is not stored: `FUN_004e15e0` fills it at start-up with the
//! scene's `budget_cost` (or the saved value) for every slot without `unlocks_rewatch`.
//!
//! ```text
//! u8  unused            // always 0; FUN_004e1300 starts reading at byte 1
//! u8  slot_count
//! slot_count x {
//!     u8 present        // 0 = empty slot (nothing else follows)
//!     -- if present:
//!     u8  flags         // bit 0 -> descriptor+0x14 (unlocks_rewatch)
//!     u32 tile_map      // .tle   -> descriptor+0x00
//!     u32 setup         // .stp   -> descriptor+0x08
//!     u32 scene         // .sod   -> descriptor+0x04
//!     u32 merits        // .mdb   -> descriptor+0x0c
//!     u8  name_len; char name[name_len]   // e.g. "S_MOUNTAINHORNE", at most 32 chars
//! }
//! ```

use crate::common::is_zero_u8;
use scribble_core::{bail, ensure, Context, Format, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelTable {
    /// First byte of the file; always 0. Never read: `FUN_004e1300` takes the slot count from
    /// byte 1 (`DAT_008ad43c = *(byte *)(local_4c + 1)`) and starts the slots at byte 2.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub unused: u8,
    /// Level slots by index; `null` is an empty slot.
    pub levels: Vec<Option<Level>>,
}

/// One level descriptor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Level {
    /// Internal level name (`S_DOWNTOWN`, `E2_RIVER`, ...).
    pub name: String,
    /// Flag bit 0 (descriptor+0x14, `FUN_004e1300`: `*(byte *)(slot + 0x14) = flags & 1`).
    /// While such a level is loaded the main menu shows its `mainmenu.rewatch` (rewatch the
    /// intro/ending) and `mainmenu.creditB` (credits) buttons (`FUN_004a2fd0` tests the current
    /// descriptor copy `DAT_008a8700+0x7c8c`), and the level's scene is left out of the
    /// start-up budget scan (`FUN_004e15e0`). Set on the story scenes `E2_RIVER`,
    /// `E2_SUBURBIA_01` and `E3_SUBURBIA_01`.
    #[serde(default, skip_serializing_if = "crate::common::is_false")]
    pub unlocks_rewatch: bool,
    /// Tile/collision map (`.tle`).
    pub tile_map: ResRef,
    /// Level setup: sky, music, parallax, exits (`.stp`).
    pub setup: ResRef,
    /// Scene: placed objects and scripts (`.sod`).
    pub scene: ResRef,
    /// Merits available in the level (`.mdb`).
    pub merits: ResRef,
}

impl Format for LevelTable {
    const NAME: &'static str = "lvls";
    const DESCRIPTION: &'static str = "Level table: tile map, setup, scene and merit resources of every level slot";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let unused = r.u8()?;
        let n = r.u8()?;
        let mut levels = Vec::with_capacity(n as usize);
        for _ in 0..n {
            match r.u8()? {
                0 => levels.push(None),
                1 => {
                    let flags = r.u8()?;
                    ensure!(flags <= 1, "unexpected level flags {flags:#x}");
                    let tile_map = ResRef::from_index(r.u32()?, ctx);
                    let setup = ResRef::from_index(r.u32()?, ctx);
                    let scene = ResRef::from_index(r.u32()?, ctx);
                    let merits = ResRef::from_index(r.u32()?, ctx);
                    let name = r.str_u8()?;
                    levels.push(Some(Level { name, unlocks_rewatch: flags == 1, tile_map, setup, scene, merits }));
                }
                v => bail!("unexpected slot marker {v}"),
            }
        }
        r.expect_end()?;
        Ok(LevelTable { unused, levels })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u8(self.unused);
        w.u8(u8::try_from(self.levels.len())?);
        for l in &self.levels {
            match l {
                None => {
                    w.u8(0);
                }
                Some(l) => {
                    w.u8(1).u8(l.unlocks_rewatch as u8);
                    w.u32(l.tile_map.to_index(ctx)?);
                    w.u32(l.setup.to_index(ctx)?);
                    w.u32(l.scene.to_index(ctx)?);
                    w.u32(l.merits.to_index(ctx)?);
                    w.str_u8(&l.name)?;
                }
            }
        }
        Ok(w.into_inner())
    }
}
