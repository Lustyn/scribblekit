//! `.dps`: dependency list — resources the game preloads together with the owning resource
//! (a level's map pieces, a menu's textures, the dictionary's tables, ...).
//!
//! ```text
//! u32 count
//! n x {
//!     u8  kind      // loader category, see DependencyKind
//!     u32 resource  // pmindex index, 0xFFFFFFFF = empty slot
//! }
//! ```
//!
//! `n` is normally `count`; a few UI files store `count` one lower than the number of records
//! actually present, which is preserved in [`Dependencies::declared_count`].
//!
//! The preloader is `FUN_00494b60`: it reads `count` records, skips empty (`0xFFFFFFFF`), zero
//! and out-of-range (`>= 0x10000`) indices and resources already cached (`FUN_0048f390`), and
//! loads each with `FUN_00492a10(index, cache_type)` where the cache type comes from the kind:
//!
//! ```text
//! kind  1 -> 7  object   (the type the .so loaders FUN_006bb9f0 / FUN_006c05f0 request)
//! kind  2 -> 9  adjective (the type the .sa loader FUN_006507c0 requests)
//! kind  3 -> 5  animation (the type the .anim loader FUN_006ec460 requests)
//! kind  5 -> 8  vector
//! kind  7 -> 6  audio stream (the type the streaming-audio code FUN_0051f2f0 requests)
//! other -> 4  plain file (the type FUN_00494b60 uses for the .dps itself)
//! ```

use scribble_core::{Context, Format, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dependencies {
    pub dependencies: Vec<Dependency>,
    /// Stored count when it differs from `dependencies.len()`: `FUN_00494b60` only preloads the
    /// first `count` records, so the records past it are never loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_count: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dependency {
    pub kind: DependencyKind,
    /// The resource, or `null` for an empty slot.
    pub resource: Option<ResRef>,
}

/// Which cache type the preloader loads the resource as (`FUN_00494b60`'s switch, see the
/// module docs). Kinds 0, 4 and 6 all load as a plain file (cache type 4); their names come
/// from the resources they point at in shipped files (`data`: `.gp`, `.sod`, `.tle`, `.uib`, ...;
/// `texture`: only `.[texture]`/`.nbtc`; `effect`: only `.gec`/`.gps`/`.trns`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// 0: plain file (cache type 4): maps, UI layouts, shaders, text, `.sao`, other lists.
    Data,
    /// 1: `.so` scribble object (cache type 7, as requested by the `.so` loader `FUN_006bb9f0`).
    Object,
    /// 2: `.sa` adjective (cache type 9, as requested by the `.sa` loader `FUN_006507c0`).
    Adjective,
    /// 3: `.anim` mesh animation (cache type 5, as requested by `FUN_006ec460`).
    Animation,
    /// 4: texture (plain file; `.[texture]` DDS and `.nbtc` atlases in shipped files).
    Texture,
    /// 5: `.vec` vector art (cache type 8).
    Vector,
    /// 6: particle / scripted effect (plain file; `.gec`, `.gps`, `.trns` in shipped files).
    Effect,
    /// 7: streamed audio (cache type 6, as requested by the audio streamer `FUN_0051f2f0`,
    /// "Audio_S"); no shipped list uses it.
    Audio,
    #[serde(untagged)]
    Other(u8),
}

impl DependencyKind {
    fn from_u8(v: u8) -> Self {
        use DependencyKind::*;
        match v {
            0 => Data,
            1 => Object,
            2 => Adjective,
            3 => Animation,
            4 => Texture,
            5 => Vector,
            6 => Effect,
            7 => Audio,
            v => Other(v),
        }
    }
    fn to_u8(self) -> u8 {
        use DependencyKind::*;
        match self {
            Data => 0,
            Object => 1,
            Adjective => 2,
            Animation => 3,
            Texture => 4,
            Vector => 5,
            Effect => 6,
            Audio => 7,
            Other(v) => v,
        }
    }
}

const EMPTY: u32 = u32::MAX;

impl Format for Dependencies {
    const NAME: &'static str = "dps";
    const DESCRIPTION: &'static str = "Dependency list: resources preloaded with the owner";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let count = r.u32()?;
        let mut dependencies = Vec::new();
        while !r.at_end() {
            let kind = DependencyKind::from_u8(r.u8()?);
            let idx = r.u32()?;
            dependencies.push(Dependency { kind, resource: (idx != EMPTY).then(|| ResRef::from_index(idx, ctx)) });
        }
        let declared_count = (count as usize != dependencies.len()).then_some(count);
        Ok(Dependencies { dependencies, declared_count })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u32(self.declared_count.unwrap_or(self.dependencies.len() as u32));
        for d in &self.dependencies {
            w.u8(d.kind.to_u8());
            w.u32(match &d.resource {
                Some(r) => r.to_index(ctx)?,
                None => EMPTY,
            });
        }
        Ok(w.into_inner())
    }
}
