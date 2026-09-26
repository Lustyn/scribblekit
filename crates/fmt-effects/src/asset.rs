//! Asset references used by the GIGL particle library (`gigl\prtcl`) that backs `.gps`, `.gec`
//! and `.trns`.
//!
//! ```text
//! u32 index   // pmindex resource index (what the engine actually loads)
//! u32 kind    // 1 = GPU program (<program>), 2 = texture (.[TEXTURE]), 3 = particle system (<system>)
//! u32 len
//! u8  path[len]   // authoring path, e.g. "[PLATFORM]/gpuprograms/determined.gp"
//! ```
//!
//! The reader is `FUN_00609450` (stream vtable slot `+0x60`); the kind names come from the table
//! at `0x008a34d0` used by `FUN_007a3130`. In every shipped file `path`, lowercased and with `/`
//! turned into `\`, is exactly the pmindex name of `index`, so the index is normally derived from
//! the path and only stored in JSON when it cannot be.

use scribble_core::{bail, ensure, Context, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

/// Kind tag stored in front of every asset reference; names from the table at `0x008a34d0`
/// (`"<undefined>"`, `"<program>"`, `".[TEXTURE]"`, `"<system>"`) used by `FUN_007a3130`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetKind {
    Program = 1,
    Texture = 2,
    System = 3,
}

/// A reference to another resource (GPU program, texture or particle system).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetRef {
    /// Path as authored (case and `/` vs `\` preserved; `FUN_00609450` reads it after the index
    /// and kind).
    pub path: String,
    /// The referenced resource, present only when it is not simply the resource named by `path`
    /// (or when no resource table was available while decoding).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResRef>,
}

/// The pmindex name a path refers to: lowercase, backslash-separated.
fn normalize(path: &str) -> String {
    path.to_lowercase().replace('/', "\\")
}

impl AssetRef {
    pub(crate) fn read(r: &mut Reader, kind: AssetKind, ctx: &Context) -> Result<Self> {
        let at = r.pos();
        let index = r.u32()?;
        let k = r.u32()?;
        ensure!(k == kind as u32, "asset reference at {at:#x}: expected kind {} ({kind:?}), found {k}", kind as u32);
        let path = r.str_u32()?;
        let derived = ctx.index(&normalize(&path)) == Some(index);
        Ok(AssetRef { resource: (!derived).then(|| ResRef::from_index(index, ctx)), path })
    }

    pub(crate) fn write(&self, w: &mut Writer, kind: AssetKind, ctx: &Context) -> Result<()> {
        let index = match &self.resource {
            Some(r) => r.to_index(ctx)?,
            None => match ctx.index(&normalize(&self.path)) {
                Some(i) => i,
                None => bail!("asset path {:?} is not a known resource; set \"resource\" explicitly", self.path),
            },
        };
        w.u32(index).u32(kind as u32);
        w.str_u32(&self.path)?;
        Ok(())
    }
}
