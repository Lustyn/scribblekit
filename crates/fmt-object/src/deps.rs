//! The dependency list embedded at the end of `.so` bodies, `.sa` files and at the start of
//! `.sao` files: every resource the engine must have loaded before the object can be built
//! (read by `FUN_006bbc50` / `FUN_006bb9f0` for objects, `FUN_00650600` for adjectives; the
//! `.sao` loader `FUN_006e8e40` skips its list: `offset + 8 + count * 5 + 1`).

use crate::util::enum8;
use scribble_core::{Context, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

enum8!(
    /// Loader category of a dependency (top 3 bits of the kind byte), same numbering as `.dps`
    /// (`fmt_common::DependencyKind`): the readers store `kind_byte >> 5` in the object's /
    /// adjective's dependency map (`FUN_006bbc50`, `FUN_006bb9f0`, `FUN_00650600`) and the
    /// asynchronous `.so` preloader `FUN_004975a0` maps it to a cache type exactly like the
    /// `.dps` preloader `FUN_00494b60` (`switch(bVar2 >> 5)`: 1 -> 7, 2 -> 9, 3 -> 5, 5 -> 8,
    /// 7 -> 6, else 4). The editor's save (`FUN_006b1d00`) tags contained objects with kind 1
    /// and animation-table entries with kind 3.
    DependencyKind {
        /// Plain file (cache type 4): the `.gp` GPU programs of objects.
        0 => Data "data",
        /// `.so` scribble object (cache type 7, the `.so` loader's).
        1 => Object "object",
        /// `.sa` adjective (cache type 9, the `.sa` loader's).
        2 => Adjective "adjective",
        /// `.anim` mesh animation (cache type 5, `FUN_006ec460`).
        3 => Animation "animation",
        /// Texture (plain file; `.[texture]` in shipped files).
        4 => Texture "texture",
        /// `.vec` vector art (cache type 8).
        5 => Vector "vector",
        /// Effect (plain file; `.gec`, `.gps` in shipped files).
        6 => Effect "effect",
        /// Streamed audio (cache type 6, `FUN_0051f2f0`); unused by shipped files.
        7 => Audio "audio",
    }
);

/// One dependency.
///
/// ```text
/// u32 resource   (pmindex index; 0xFFFFFFFF = empty slot)
/// u8  kind       bits 5-7 DependencyKind, bits 0-4 unused (11 or 15 in shipped files)
/// ```
///
/// Bits 0-4 are discarded by every reader (`FUN_006bbc50`/`FUN_006bb9f0`/`FUN_00650600` keep
/// only `kind_byte >> 5`; `FUN_006e8e40` skips `.sao` lists whole) and the engine's own writer
/// `FUN_006b1910` writes `kind << 5` with them clear; shipped files carry the authoring tool's
/// 15 (objects) / 11 (everything else) in every entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dependency {
    /// The resource, `null` for an empty slot.
    pub resource: Option<ResRef>,
    pub kind: DependencyKind,
    /// Low 5 bits of the kind byte (never read, see above); omitted when it is the usual value
    /// (15 for objects, 11 otherwise). JSON `unused_bits`.
    #[serde(default, rename = "unused_bits", skip_serializing_if = "Option::is_none")]
    pub flags: Option<u8>,
}

impl Dependency {
    /// The value bits 0-4 have in every shipped dependency of this kind.
    pub fn usual_flags(kind: DependencyKind) -> u8 {
        if kind == DependencyKind::Object { 15 } else { 11 }
    }
}

/// `u32 count; count x Dependency`.
pub fn read(r: &mut Reader, ctx: &Context) -> Result<Vec<Dependency>> {
    let n = r.u32()?;
    let mut out = Vec::with_capacity(n.min(4096) as usize);
    for _ in 0..n {
        let idx = r.u32()?;
        let k = r.u8()?;
        let kind = DependencyKind::from_u8(k >> 5);
        let flags = k & 0x1f;
        out.push(Dependency {
            resource: crate::util::res32(idx, ctx),
            kind,
            flags: (flags != Dependency::usual_flags(kind)).then_some(flags),
        });
    }
    Ok(out)
}

pub fn write(list: &[Dependency], w: &mut Writer, ctx: &Context) -> Result<()> {
    w.u32(list.len() as u32);
    for d in list {
        let k = d.kind.to_u8();
        let flags = d.flags.unwrap_or(Dependency::usual_flags(d.kind));
        scribble_core::ensure!(k < 8 && flags < 0x20, "dependency kind/flags out of range");
        w.u32(crate::util::from_res32(&d.resource, ctx)?);
        w.u8(k << 5 | flags);
    }
    Ok(())
}
