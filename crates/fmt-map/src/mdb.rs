//! `.mdb`: merit database — the merits (achievements) that can be earned in a level
//! (`data\merits\<level>.mdb`, level descriptor +0x0c).
//!
//! Loaded by `FUN_004fa620` into 0x48-byte merit records (`FUN_004f73d0` reads one entry).
//! The merit texts live in the text table named by the header (`<level>_$merits`); each merit
//! owns consecutive strings starting at `text_index` (three per merit in shipped files).
//!
//! ```text
//! u32 text_table         // -> every record +0x18
//! u16 count
//! count x {
//!     u16 id             // -> +0x04
//!     u16 text_index     // -> +0x1c
//!     u32 icon           // texture, -> +0x14
//!     u8  category       // -> +0x08; 0..7 are counted per category, 0xFF = none
//!     u8  unused[2]      // skipped (always 0xFF 0xFF)
//!     u8  unused_kind    // -> +0x24, never read (1, 3 or 0xFF)
//! }
//! ```

use scribble_core::{Context, Format, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeritDatabase {
    /// Text table holding the merit names/descriptions (`0xFFFFFFFF` = none).
    pub text_table: Option<ResRef>,
    pub merits: Vec<Merit>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Merit {
    /// Merit id (record +0x04): the key `FUN_004fa310` binary-searches, the bit of the save's
    /// earned-merit bitmap (`FUN_004f7490`: `DAT_008b3404+0x50` bit `id`), and what level
    /// scripts and merit rules refer to.
    pub id: u16,
    /// Index of the merit's first string in [`MeritDatabase::text_table`] (record +0x1c; the
    /// browser pages merits three strings at a time, `FUN_004f8360`).
    pub text_index: u16,
    /// Icon texture (record +0x14).
    pub icon: ResRef,
    /// Merit browser category (record +0x08; `FUN_004fa620` counts merits per category 0..7,
    /// `FUN_004f8360` lists a category's merits).
    pub category: MeritCategory,
    /// Two bytes skipped by the loader (`FUN_004f73d0`: `*param_3 = *param_3 + 3` after the
    /// category byte).
    #[serde(default = "unused_default", skip_serializing_if = "is_unused_default")]
    pub unused: [u8; 2],
    /// Byte stored at record +0x24 by `FUN_004f73d0` and only ever copied (`FUN_004f7780`,
    /// `FUN_004f7ca0`); no reader exists (searched every user of the merit accessors
    /// `FUN_004fa310`/`FUN_004fa380`/`FUN_004fa3e0`/`FUN_004fa450`, the record methods
    /// `FUN_004f7490`..`FUN_004f7a70` and the pending-merit copy at `+0x7f4`). In the data it
    /// looks like a merit class: 255 on the 78 named quest merits (`excalibur`,
    /// `youre_a_pirate`, ...; category `none`), 3 on `firestarter`, 1 on all others.
    pub unused_kind: u8,
}

fn unused_default() -> [u8; 2] {
    [0xff, 0xff]
}

fn is_unused_default(v: &[u8; 2]) -> bool {
    *v == [0xff, 0xff]
}

/// Merit browser categories (`meritbrowse.globalcategory.*Text`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeritCategory {
    Living,
    Food,
    Vehicle,
    Music,
    Tech,
    Weapon,
    Clothes,
    Misc,
    /// 0xFF: not listed in a category.
    None,
    #[serde(untagged)]
    Other(u8),
}

impl MeritCategory {
    fn from_u8(v: u8) -> Self {
        use MeritCategory::*;
        match v {
            0 => Living,
            1 => Food,
            2 => Vehicle,
            3 => Music,
            4 => Tech,
            5 => Weapon,
            6 => Clothes,
            7 => Misc,
            0xff => None,
            v => Other(v),
        }
    }
    fn to_u8(self) -> u8 {
        use MeritCategory::*;
        match self {
            Living => 0,
            Food => 1,
            Vehicle => 2,
            Music => 3,
            Tech => 4,
            Weapon => 5,
            Clothes => 6,
            Misc => 7,
            None => 0xff,
            Other(v) => v,
        }
    }
}

impl Format for MeritDatabase {
    const NAME: &'static str = "mdb";
    const DESCRIPTION: &'static str = "Merit database: merits available in a level";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let text_table = crate::common::opt_res(r.u32()?, ctx);
        let n = r.u16()?;
        let mut merits = Vec::with_capacity(n as usize);
        for _ in 0..n {
            merits.push(Merit {
                id: r.u16()?,
                text_index: r.u16()?,
                icon: ResRef::from_index(r.u32()?, ctx),
                category: MeritCategory::from_u8(r.u8()?),
                unused: r.array()?,
                unused_kind: r.u8()?,
            });
        }
        r.expect_end()?;
        Ok(MeritDatabase { text_table, merits })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u32(crate::common::opt_res_index(&self.text_table, ctx)?);
        w.u16(u16::try_from(self.merits.len())?);
        for m in &self.merits {
            w.u16(m.id).u16(m.text_index).u32(m.icon.to_index(ctx)?).u8(m.category.to_u8()).bytes(&m.unused).u8(m.unused_kind);
        }
        Ok(w.into_inner())
    }
}
