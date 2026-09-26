//! `.sa` - adjective definition (loaded by `FUN_006507c0`).
//!
//! ```text
//! u32 deps_offset          absolute offset of the dependency list (read, FUN_00650600)
//! u16 category, u16 group, u16 adjective   taxonomy id (+0x8, +0xa, +0xc)
//! u16 budget_cost          (+0x38)
//! u8  flags                bit0 inheritable (+0x32), bit1 transient (+0x33), bit2 hidden (+0x34)
//! u8  appeal               (+0x3c)
//! u8  apply_order          (+0x37)
//! u32 tags_offset          absolute offset of the 0x0E marker (skipped: the loader jumps from
//!                          byte 0xe to 0x13)
//! RefList cancels          (a lone 0 byte when empty; +0x40)
//! RefList blocks           (a lone 0 byte when empty; +0x44)
//! 0x0C; u8 n; n x u32 effect_offset; n x Effect      (marker and offsets skipped: `+= n * 4`)
//! 0x0E; u8 n; n x u16 tag                            (+0x60 count, +0x64 list)
//! u32 n; n x Dependency
//! ```
//!
//! All offsets and counts are recomputed on encode.
//!
//! An **effect** (`FUN_004358e0`, parsed by `FUN_00435950`) is a group of modifiers, optionally
//! conditional on the object being one of a list of objects. Effects are alternatives: the
//! engine keeps only the first effect whose condition matches (`FUN_00650090`):
//!
//! ```text
//! u8 type                         0 = always, 1 = conditional
//! [type 1] u8 n; n x (u8 exclude; ObjectRef)   (FUN_00435220)
//! u8 n; n x Modifier
//! ```

use crate::behaviour::Modifier;
use crate::deps::{self, Dependency};
use crate::refs::{AdjectivePath, ObjectRef, RefList};
use crate::util::{count8, flags8, marker};
use scribble_core::{bail, ensure, Context, Format, Reader, Result, Writer};
use serde::{Deserialize, Serialize};

/// A `.sa` file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Adjective {
    /// This adjective's taxonomy id.
    pub id: AdjectivePath,
    /// Budget points the adjective adds to the object: `FUN_00650090` adds `+0x38` (plus 0x40
    /// when the adjective makes an inanimate object animate) to the object's budget `+0x284`
    /// (phobias 10-11, colours 12).
    pub budget_cost: u16,
    pub flags: AdjectiveFlags,
    /// How much AIs want the object, 0-10: `FUN_00650090` copies `+0x3c` to the object's
    /// `+0x286`, and the AI evaluator `FUN_00656100` ranks reaction targets by
    /// `10 - target+0x286` (lower = preferred). `hated` 1, 5 default, `tasty` 9, `invaluable` 10.
    pub appeal: u8,
    /// Adjectives are applied in ascending order: `FUN_00652420` sorts the object's adjectives
    /// by `+0x37` (materials 150-200, colours 250, `_dead` 253).
    pub apply_order: u8,
    /// Conflicting adjectives: when either of two adjectives lists the other, one is removed
    /// (`+0x40`, a `RefList` read by `FUN_00675350`, tested by `FUN_00651200`; `aboveground` /
    /// `underground`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancels: Option<RefList>,
    /// Adjectives this one keeps from being added while it is applied (`+0x44`, a `RefList`
    /// read by `FUN_00675350`; `immune` blocks `sick`, `inextinguishable` blocks
    /// `extinguished`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocks: Option<RefList>,
    /// Alternatives (`+0x48`, count `+0x4c`): `FUN_00650090` keeps the first effect whose
    /// condition matches (index `+0x4d`) and frees the rest.
    pub effects: Vec<Effect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<scribble_core::NamedId>,
    pub dependencies: Vec<Dependency>,
}

flags8!(
    /// Adjective flags (`FUN_006507c0`: bit 0 -> `+0x32`, bit 1 -> `+0x33`, bit 2 -> `+0x34`).
    AdjectiveFlags {
        /// Copied onto objects this object creates or equips (the loader clears `+0x32` again
        /// when no modifier is `inheritable`, `FUN_0068d070`).
        0 => "inheritable",
        /// Removed by a refresh when the player added it (`FUN_00652420` tests `+0x33 == 1`
        /// with the player-added flag 0x200; `FUN_00651bb0`; set by no shipped adjective).
        1 => "transient",
        /// Left out of the object's name and not inherited (`_male`, `_dead`, `_invincible`):
        /// the name getter `FUN_0064ff60` returns nothing and `FUN_0064ff80` shows word 0xFFFE
        /// (hidden) when `+0x34` is set.
        2 => "hidden",
    }
);

/// A condition entry of a conditional effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectCondition {
    /// A match on this entry rejects the object (`FUN_00435220` passes `byte != 0` as the
    /// entry's exclude flag to `FUN_006756f0`; matched by `FUN_00674cf0`).
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub exclude: bool,
    #[serde(flatten)]
    pub target: ObjectRef,
}

/// A group of modifiers applied together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    /// Present for conditional effects (type 1, `FUN_004358e0` builds the conditional class
    /// `FUN_004357a0`): the objects the effect applies to (`FUN_00435220`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_for: Option<Vec<EffectCondition>>,
    pub modifiers: Vec<Modifier>,
}

fn read_optional_reflist(r: &mut Reader, ctx: &Context) -> Result<Option<RefList>> {
    if r.peek_u8()? == 0 {
        r.u8()?;
        Ok(None)
    } else {
        Ok(Some(RefList::read(r, ctx)?))
    }
}

fn write_optional_reflist(l: &Option<RefList>, w: &mut Writer, ctx: &Context) -> Result<()> {
    match l {
        Some(l) => {
            ensure!(!l.entries.is_empty(), "an adjective reference list must have entries (use null instead)");
            l.write(w, ctx)
        }
        None => {
            w.u8(0);
            Ok(())
        }
    }
}

impl Effect {
    fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let t = r.u8()?;
        let only_for = match t {
            0 => None,
            1 => {
                let n = r.u8()?;
                Some((0..n).map(|_| Ok(EffectCondition { exclude: r.bool()?, target: ObjectRef::read(r, ctx)? })).collect::<Result<Vec<_>>>()?)
            }
            t => bail!("unknown effect type {t}"),
        };
        let n = r.u8()?;
        let modifiers = (0..n).map(|_| Modifier::read(r, ctx)).collect::<Result<_>>()?;
        Ok(Effect { only_for, modifiers })
    }
    fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        match &self.only_for {
            None => {
                w.u8(0);
            }
            Some(list) => {
                w.u8(1);
                count8(w, list.len(), "effect conditions")?;
                for c in list {
                    w.bool(c.exclude);
                    c.target.write(w, ctx)?;
                }
            }
        }
        count8(w, self.modifiers.len(), "modifiers")?;
        for m in &self.modifiers {
            m.write(w, ctx)?;
        }
        Ok(())
    }
}

impl Format for Adjective {
    const NAME: &'static str = "sa";
    const DESCRIPTION: &'static str = "Adjective definition: property changes, added behaviours, conflicts";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let deps_offset = r.u32()? as usize;
        let id = AdjectivePath::read(&mut r, ctx)?;
        let budget_cost = r.u16()?;
        let flags = AdjectiveFlags(r.u8()?);
        let appeal = r.u8()?;
        let apply_order = r.u8()?;
        let tags_offset = r.u32()? as usize;
        let cancels = read_optional_reflist(&mut r, ctx)?;
        let blocks = read_optional_reflist(&mut r, ctx)?;
        marker(&mut r, 0x0c)?;
        let n = r.u8()?;
        let offsets = (0..n).map(|_| r.u32()).collect::<Result<Vec<_>>>()?;
        let mut effects = Vec::with_capacity(n as usize);
        for o in offsets {
            ensure!(r.pos() == o as usize, "effect offset {o:#x} does not match position {:#x}", r.pos());
            effects.push(Effect::read(&mut r, ctx)?);
        }
        ensure!(r.pos() == tags_offset, "tags offset {tags_offset:#x} does not match position {:#x}", r.pos());
        marker(&mut r, 0x0e)?;
        let tags = crate::util::read_tags(&mut r, ctx)?;
        ensure!(r.pos() == deps_offset, "dependency offset {deps_offset:#x} does not match position {:#x}", r.pos());
        let dependencies = deps::read(&mut r, ctx)?;
        r.expect_end()?;
        Ok(Adjective { id, budget_cost, flags, appeal, apply_order, cancels, blocks, effects, tags, dependencies })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u32(0);
        self.id.write(&mut w, ctx)?;
        w.u16(self.budget_cost).u8(self.flags.0).u8(self.appeal).u8(self.apply_order);
        let tags_at = w.pos();
        w.u32(0);
        write_optional_reflist(&self.cancels, &mut w, ctx)?;
        write_optional_reflist(&self.blocks, &mut w, ctx)?;
        w.u8(0x0c);
        count8(&mut w, self.effects.len(), "effects")?;
        let table = w.pos();
        for _ in &self.effects {
            w.u32(0);
        }
        for (i, e) in self.effects.iter().enumerate() {
            let at = w.pos() as u32;
            w.patch_u32(table + 4 * i, at);
            e.write(&mut w, ctx)?;
        }
        let at = w.pos() as u32;
        w.patch_u32(tags_at, at);
        w.u8(0x0e);
        crate::util::write_tags(&self.tags, &mut w, ctx)?;
        let at = w.pos() as u32;
        w.patch_u32(0, at);
        deps::write(&self.dependencies, &mut w, ctx)?;
        Ok(w.into_inner())
    }
}
