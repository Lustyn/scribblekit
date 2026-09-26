//! `.exf`: custom filter (`data\customfilters\`) — a list of objects and/or adjectives the game
//! draws from for a purpose named by the file: adjectives by category (`asadjbycat_*`,
//! `asadjbysubcat_*`), auto-spawner pools (`autospawnerby[sub]cat_*`), doppelganger / genie /
//! word-slot / survival / tutorial / rumble pools, multiplayer adjective groups, ...
//!
//! Every id is the u16 pmindex index of a `.so` (object) or `.sa` (adjective) resource.
//!
//! The game's filter object loads a file by resource index (`FUN_00674390` -> `FUN_004658e0`)
//! and keeps the u32 count; entries are read only by `FUN_00674420` (entry `i`, `i < count`, the
//! u16 at `4 + 2*i`) and `FUN_006744c0` (random entry). Users: genie wishes (`FUN_00547b30`),
//! tutorials (`FUN_00723150`..), rumble power-ups (`FUN_0063af30`), multiplayer groups
//! (`FUN_00703b90`).
//!
//! ```text
//! u32 count
//! u16 resource[count]    // pmindex indices (.so / .sa); not necessarily sorted
//! u8  unused             // 0 in every shipped file; never read
//! ```

use scribble_core::{ensure, Context, Format, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomFilter {
    /// The objects / adjectives in the filter, in stored order.
    pub resources: Vec<ResRef>,
    /// Trailing byte (0 in all 359 shipped files). Never read: the accessors `FUN_00674420` /
    /// `FUN_006744c0` only index entries below the stored count.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unused_trailer: u8,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

impl Format for CustomFilter {
    const NAME: &'static str = "exf";
    const DESCRIPTION: &'static str = "Custom filter: a list of object/adjective resources";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let n = r.u32()?;
        let resources = (0..n).map(|_| Ok(ResRef::from_index(r.u16()? as u32, ctx))).collect::<Result<_>>()?;
        let unused_trailer = r.u8()?;
        r.expect_end()?;
        Ok(CustomFilter { resources, unused_trailer })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u32(self.resources.len() as u32);
        for res in &self.resources {
            let i = res.to_index(ctx)?;
            ensure!(i <= u16::MAX as u32, "resource index {i} does not fit in 16 bits");
            w.u16(i as u16);
        }
        w.u8(self.unused_trailer);
        Ok(w.into_inner())
    }
}
