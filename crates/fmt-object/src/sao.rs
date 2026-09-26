//! `.sao` - simple (background) scribble object: just a node tree and an idle animation,
//! loaded by `FUN_006e8e40` (tree parser `FUN_006e8100`). Used for map decorations such as
//! the background cow, shrubs and windmills.
//!
//! ```text
//! u32 flags          bit0 dependencies, bit1 tree, bit2 idle animation (upper bits kept)
//! [bit0] 0x0F; u32 n; n x Dependency
//! [bit1] 0x08; Node root           (simple flavour: node types 0, 4, 5, 9)
//! [bit2] 0x09; u32 idle_animation  (played in slot 14 = idle)
//! ```
//!
//! `FUN_006e8e40` reads only the first byte of `flags` (`bVar1 = *buf`; bits 0-2), skips the
//! dependency list whole (`+ 8 + count * 5 + 1`) and the 0x08 / 0x09 markers, parses the tree
//! with `FUN_006e8100` and starts the idle animation with `FUN_00665c00(0xe, anim, ...)`
//! (`-1` = none). The `.sao` itself is preloaded by maps' `.dps` lists as kind 0 (plain file).

use crate::deps::{self, Dependency};
use crate::tree::{Flavor, Node};
use crate::util::marker;
use scribble_core::{ensure, Context, Format, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

/// A `.sao` file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimpleObject {
    /// Bits 3-31 of the flags word: never read (`FUN_006e8e40` tests bits 0-2 of the first
    /// byte only); zero in shipped files. JSON `unused_flags`.
    #[serde(default, rename = "unused_flags", skip_serializing_if = "is_zero")]
    pub extra_flags: u32,
    /// Flag bit 0; skipped whole by the loader `FUN_006e8e40`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<Dependency>>,
    /// Flag bit 1: the node tree (`FUN_006e8100`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<Node>,
    /// Animation looped in the idle slot (slot 14, `FUN_00665c00(0xe, ...)` then
    /// `FUN_006eda70`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_animation: Option<ResRef>,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

impl Format for SimpleObject {
    const NAME: &'static str = "sao";
    const DESCRIPTION: &'static str = "Simple background object: node tree and idle animation";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let flags = r.u32()?;
        let dependencies = if flags & 1 != 0 {
            marker(&mut r, 0x0f)?;
            Some(deps::read(&mut r, ctx)?)
        } else {
            None
        };
        let root = if flags & 2 != 0 {
            marker(&mut r, 0x08)?;
            Some(Node::read(&mut r, ctx, Flavor::Simple, None)?)
        } else {
            None
        };
        let idle_animation = if flags & 4 != 0 {
            marker(&mut r, 0x09)?;
            Some(ResRef::from_index(r.u32()?, ctx))
        } else {
            None
        };
        r.expect_end()?;
        Ok(SimpleObject { extra_flags: flags & !7, dependencies, root, idle_animation })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        ensure!(self.extra_flags & 7 == 0, "extra_flags bits 0-2 are derived");
        let mut w = Writer::new();
        w.u32(self.extra_flags | self.dependencies.is_some() as u32 | (self.root.is_some() as u32) << 1 | (self.idle_animation.is_some() as u32) << 2);
        if let Some(d) = &self.dependencies {
            w.u8(0x0f);
            deps::write(d, &mut w, ctx)?;
        }
        if let Some(n) = &self.root {
            w.u8(0x08);
            n.write(&mut w, ctx, Flavor::Simple, None)?;
        }
        if let Some(a) = &self.idle_animation {
            w.u8(0x09).u32(a.to_index(ctx)?);
        }
        Ok(w.into_inner())
    }
}
