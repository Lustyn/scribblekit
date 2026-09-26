//! References to other objects and adjectives by their *taxonomy ids*, and the small list
//! structures built from them (reference lists, filters, relations).
//!
//! Scribble objects and adjectives are not referenced by resource index here but by their
//! position in the game's word taxonomy: every `.so` starts with four u16 ids
//! (`category, subcategory, group, object`, e.g. mammal/large/hooved/cow = `[16, 1562, 1588,
//! 1598]`; the last is the dictionary object word id) and every `.sa` with three (`category,
//! group, adjective`). `0xFFFF` in any position is a wildcard ("any"). The JSON prints them as
//! paths of names from the [`Context`] (`"food/nutsgrains/*/*"`); see
//! `scribble_formats::context` for where the names come from.

use crate::util::count8;
use scribble_core::{ns, Context, NamedPath, Reader, Result, Writer};
use serde::{Deserialize, Serialize};

/// Four-level object taxonomy path (`category, subcategory, group, object`), printed through the
/// [`Context`]'s taxonomy names as `"mammal/large/hooved/cow"`; `*` = any (`0xFFFF`), numbers
/// for ids without a name, `name#id` where a name is ambiguous (see [`NamedPath`]).
///
/// ```text
/// u16 category, u16 subcategory, u16 group, u16 object    (0xFFFF = any)
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObjectPath(pub NamedPath);

impl Default for ObjectPath {
    fn default() -> Self {
        Self::any()
    }
}

impl ObjectPath {
    /// `*/*/*/*`: any object.
    pub fn any() -> Self {
        ObjectPath(NamedPath("*/*/*/*".into()))
    }
    pub fn from_ids(ids: [Option<u16>; 4], ctx: &Context) -> Self {
        ObjectPath(NamedPath::from_ids(&ids.map(|v| v.unwrap_or(u16::MAX) as u32), &ns::OBJECT_PATH, u16::MAX as u32, ctx))
    }
    /// The raw ids (`None` = any).
    pub fn ids(&self, ctx: &Context) -> Result<[Option<u16>; 4]> {
        let v = self.0.to_ids(&ns::OBJECT_PATH, u16::MAX as u32, ctx)?;
        let mut out = [None; 4];
        for (o, x) in out.iter_mut().zip(v) {
            *o = (x != u16::MAX as u32).then_some(u16::try_from(x)?);
        }
        Ok(out)
    }
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        Ok(Self::from_ids([r.u16()?, r.u16()?, r.u16()?, r.u16()?].map(opt), ctx))
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        for v in self.ids(ctx)? {
            w.u16(v.unwrap_or(u16::MAX));
        }
        Ok(())
    }
    pub fn is_any(&self) -> bool {
        self.0.is_any()
    }
}

fn opt(v: u16) -> Option<u16> {
    (v != u16::MAX).then_some(v)
}

/// Three-level adjective taxonomy path (`category, group, adjective`), printed like
/// [`ObjectPath`]: `"color/hue/red"`, `*` = any.
///
/// ```text
/// u16 category, u16 group, u16 adjective    (0xFFFF = any)
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AdjectivePath(pub NamedPath);

impl Default for AdjectivePath {
    fn default() -> Self {
        Self::any()
    }
}

impl AdjectivePath {
    /// `*/*/*`: any adjective.
    pub fn any() -> Self {
        AdjectivePath(NamedPath("*/*/*".into()))
    }
    pub fn from_ids(ids: [Option<u16>; 3], ctx: &Context) -> Self {
        AdjectivePath(NamedPath::from_ids(&ids.map(|v| v.unwrap_or(u16::MAX) as u32), &ns::ADJECTIVE_PATH, u16::MAX as u32, ctx))
    }
    /// The raw ids (`None` = any).
    pub fn ids(&self, ctx: &Context) -> Result<[Option<u16>; 3]> {
        let v = self.0.to_ids(&ns::ADJECTIVE_PATH, u16::MAX as u32, ctx)?;
        let mut out = [None; 3];
        for (o, x) in out.iter_mut().zip(v) {
            *o = (x != u16::MAX as u32).then_some(u16::try_from(x)?);
        }
        Ok(out)
    }
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        Ok(Self::from_ids([r.u16()?, r.u16()?, r.u16()?].map(opt), ctx))
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        for v in self.ids(ctx)? {
            w.u16(v.unwrap_or(u16::MAX));
        }
        Ok(())
    }
    pub fn is_any(&self) -> bool {
        self.0.is_any()
    }
}

/// A reference to a scene entity (u32): a placed object by instance id (`{"object": n}`, n =
/// the `.sod` `objects` index minus 1, see [`crate::record::entity_to_json`]), an object group of
/// the scene (`{"group": n}`, stored as `0xFF0000nn`) or another runtime handle
/// (`{"handle": n}`); `0xFFFFFFFF` = none (`null`). See [`crate::record::entity_to_json`].
/// `FUN_00675350` treats any non-zero high byte as a group: it clears the byte and sets the
/// list's group flag (`+0x28`), and `FUN_00674aa0` then matches group membership
/// (`FUN_0069e960`) instead of the object id (`+0xc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entity(pub u32);

impl Entity {
    pub const NONE: Entity = Entity(u32::MAX);
    pub fn is_none(&self) -> bool {
        self.0 == u32::MAX
    }
}

impl Default for Entity {
    fn default() -> Self {
        Entity::NONE
    }
}

impl Serialize for Entity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        crate::record::entity_to_json(self.0).serialize(s)
    }
}

impl<'de> Deserialize<'de> for Entity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        crate::record::json_to_entity(&v).map(Entity).map_err(serde::de::Error::custom)
    }
}

/// An object (optionally qualified by an adjective), e.g. "any mammal", "a *red* car".
///
/// ```text
/// ObjectPath object     (4 x u16)
/// AdjectivePath adjective (3 x u16)
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct ObjectRef {
    pub object: ObjectPath,
    /// Required adjective; omitted when "any".
    #[serde(default, skip_serializing_if = "AdjectivePath::is_any")]
    pub adjective: AdjectivePath,
}

impl ObjectRef {
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        Ok(ObjectRef { object: ObjectPath::read(r, ctx)?, adjective: AdjectivePath::read(r, ctx)? })
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        self.object.write(w, ctx)?;
        self.adjective.write(w, ctx)
    }
}

/// One entry of a [`RefList`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefListEntry {
    /// An exclusion: objects matching it are rejected (`FUN_00675350` reads `byte != 0` and
    /// counts exclusions at `+0x25`; `FUN_00674aa0` returns false on a match).
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub exclude: bool,
    #[serde(flatten)]
    pub target: ObjectRef,
}

/// A list of object references (`FUN_00675350`, matched by `FUN_00674aa0`): an object matches
/// if it matches an entry and no `exclude` entry, or is the `subject` entity. Used for the
/// objects a zone affects, what a spawner creates, the keys of a lock, ...
///
/// ```text
/// u8 count
/// count x { u8 exclude (0/1); ObjectRef target (7 x u16) }
/// u32 subject    scene entity (0xFFFFFFFF = none)
/// ```
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct RefList {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<RefListEntry>,
    /// A specific scene entity that also matches (`null` = none; `FUN_00674aa0` compares it
    /// first: with the object id `+0xc`, or group membership for group entities).
    #[serde(default, skip_serializing_if = "Entity::is_none")]
    pub subject: Entity,
}

impl RefList {
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let n = r.u8()?;
        let mut entries = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let exclude = r.bool()?;
            entries.push(RefListEntry { exclude, target: ObjectRef::read(r, ctx)? });
        }
        Ok(RefList { entries, subject: Entity(r.u32()?) })
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        count8(w, self.entries.len(), "reference list entries")?;
        for e in &self.entries {
            w.bool(e.exclude);
            e.target.write(w, ctx)?;
        }
        w.u32(self.subject.0);
        Ok(())
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.subject.is_none()
    }
}

/// One clause of a [`Filter`] (`FUN_00676330`): matches objects that are one of `objects`
/// (any; `_/self/self/me` = the owner, `_/self/self/myobject` = the owner's type), have the
/// `adjectives` and carry the `tags` (all of them, or any with the `*_match_any` flags).
///
/// ```text
/// u8 flags                     bit0 adjectives_match_any, bit1 tags_match_any
/// u8 n1; n1 x ObjectPath       (4 x u16)
/// u8 n2; n2 x AdjectivePath    (3 x u16)
/// u8 n3; n3 x u16 tag
/// ```
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct FilterEntry {
    /// Flag bit 0 (`FUN_00676330` -> `+1`): any of `adjectives` suffices (else all are
    /// required; `FUN_00676780`).
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub adjectives_match_any: bool,
    /// Flag bit 1 (`FUN_00676330` -> `+2`): any of `tags` suffices (else all are required;
    /// `FUN_00676930`).
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub tags_match_any: bool,
    /// Flag bits 2-7: never read (`FUN_00676330` keeps only `b & 1` and `b >> 1 & 1`); zero in
    /// shipped files. JSON `unused_flags`.
    #[serde(default, rename = "unused_flags", skip_serializing_if = "crate::util::is_zero_u8")]
    pub other_flags: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objects: Vec<ObjectPath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adjectives: Vec<AdjectivePath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<scribble_core::NamedId>,
}

impl FilterEntry {
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let flags = r.u8()?;
        let n = r.u8()?;
        let objects = (0..n).map(|_| ObjectPath::read(r, ctx)).collect::<Result<_>>()?;
        let n = r.u8()?;
        let adjectives = (0..n).map(|_| AdjectivePath::read(r, ctx)).collect::<Result<_>>()?;
        let tags = crate::util::read_tags(r, ctx)?;
        Ok(FilterEntry { adjectives_match_any: flags & 1 != 0, tags_match_any: flags & 2 != 0, other_flags: flags & !3, objects, adjectives, tags })
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        scribble_core::ensure!(self.other_flags & 3 == 0, "filter other_flags bits 0-1 are the *_match_any flags");
        w.u8(self.adjectives_match_any as u8 | (self.tags_match_any as u8) << 1 | self.other_flags);
        count8(w, self.objects.len(), "filter objects")?;
        for o in &self.objects {
            o.write(w, ctx)?;
        }
        count8(w, self.adjectives.len(), "filter adjectives")?;
        for a in &self.adjectives {
            a.write(w, ctx)?;
        }
        crate::util::write_tags(&self.tags, w, ctx)?;
        Ok(())
    }
}

/// Object filter used by triggers and scene groups (`FUN_006778d0`; written by
/// `FUN_00676e60`). An object matches if it matches any `include` clause (or `include` is
/// empty) and no `exclude` clause (`FUN_00677050`). A trigger's `subject` entity, when set,
/// replaces the clauses (`FUN_00677df0`).
///
/// ```text
/// u8 unused                          (never read: FUN_00676b80 starts at byte 2 and
///                                     FUN_006769e0 reads the count at byte 1)
/// u8 n; n x FilterEntry include
/// u8 m; m x FilterEntry exclude
/// ```
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Filter {
    /// Leading byte, never read (see above); zero in shipped files. JSON `unused`.
    #[serde(default, rename = "unused", skip_serializing_if = "crate::util::is_zero_u8")]
    pub reserved: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<FilterEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<FilterEntry>,
}

impl Filter {
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let reserved = r.u8()?;
        let n = r.u8()?;
        let include = (0..n).map(|_| FilterEntry::read(r, ctx)).collect::<Result<_>>()?;
        let n = r.u8()?;
        let exclude = (0..n).map(|_| FilterEntry::read(r, ctx)).collect::<Result<_>>()?;
        Ok(Filter { reserved, include, exclude })
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        w.u8(self.reserved);
        count8(w, self.include.len(), "filter clauses")?;
        for e in &self.include {
            e.write(w, ctx)?;
        }
        count8(w, self.exclude.len(), "filter clauses")?;
        for e in &self.exclude {
            e.write(w, ctx)?;
        }
        Ok(())
    }
    pub fn is_empty(&self) -> bool {
        self.reserved == 0 && self.include.is_empty() && self.exclude.is_empty()
    }
}

crate::util::enum8!(
    /// What an AI does about the objects a relation names (low 5 bits of the relation flags).
    /// Names are the object editor's `static_atrrepmode` list, in order (DESTROY, CONSUME,
    /// INVESTIGATE, FOLLOW, PROTECT, USE, MOUNT, STEAL, FLEE, SPLIT, GUARD, SPLIT (TOOL), DEAL
    /// DAMAGE, FIRE PROJECTILE, USE TOOL, STAGE OBJECT, USE VEHICLE; the first nine also in
    /// `static_atrrep_mode`, 0x22e8, the editor's pick list). The AI evaluator (`FUN_00656100`)
    /// returns the kind as the AI action and `FUN_00655830` maps it to the `on_ai_action` event;
    /// `set_stage_object` and `on_ai_action` use the same numbering. Shipped relations use
    /// 0-8 and 14.
    RelationKind {
        /// DESTROY (attack); added by e.g. `angry`, `acidic`, `_teama`.
        0 => Destroy "destroy",
        /// CONSUME (eat); added by `-vorous`/`-phagous` adjectives.
        1 => Consume "consume",
        /// INVESTIGATE; the most common kind.
        2 => Investigate "investigate",
        /// FOLLOW; `_dogwhistlefollower`, `beelike`.
        3 => Follow "follow",
        /// PROTECT; `-philic` adjectives; eggs, babies, puppies.
        4 => Protect "protect",
        /// USE (spades, jukeboxes, computers).
        5 => Use "use",
        /// MOUNT (horses, vehicles, skateboards).
        6 => Mount "mount",
        /// STEAL; `criminal`, `greedy`, `jealous`.
        7 => Steal "steal",
        /// FLEE; every `-phobic` adjective.
        8 => Flee "flee",
        /// SPLIT (text only; no shipped relation).
        9 => Split "split",
        /// GUARD (text only).
        10 => Guard "guard",
        /// SPLIT (TOOL) (text only).
        11 => SplitTool "split_tool",
        /// DEAL DAMAGE (text only).
        12 => DealDamage "deal_damage",
        /// FIRE PROJECTILE (text only).
        13 => FireProjectile "fire_projectile",
        /// USE TOOL: only when the held tool matches `tools` (gasoline, vomit, puddles); the
        /// only kind with a trailing `RefList` (`FUN_00658830`, `FUN_0043f190`).
        14 => UseTool "use_tool",
        /// STAGE OBJECT (text only; the `set_stage_object` action's reaction).
        15 => StageObject "stage_object",
        /// USE VEHICLE (text only).
        16 => UseVehicle "use_vehicle",
    }
);

/// A relationship record as stored in object bodies (`FUN_00658830`): how this object's AI
/// reacts to objects matching `object`/`adjective` (or the tag list `tags`).
///
/// ```text
/// u8 flags            bits 0-4 kind, bit5 has tag list, bit6 has priority, bit7 via_holder
/// [bit5] u8 tags_match_any (0/1); u8 n; n x u16 tag
/// [bit6] u8 priority    (engine default 28)
/// ObjectRef target      (7 x u16)
/// [kind == 14] RefList tools
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub kind: RelationKind,
    /// Bit 7 (`FUN_00658830`: `local_4 = ... | bVar1 >> 7`): only applies when the target is
    /// held, equipped or ridden by another object, and the AI then acts on the holder
    /// (`FUN_0066ad10`).
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub via_holder: bool,
    /// Tags to match instead of the object reference (bit 5: `u8 match_any` -> flag bit 3,
    /// then the tags via `FUN_0043e5b0`/`FUN_0043e620`; matched by `FUN_0043e640`; unused by
    /// shipped data).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<RelationTags>,
    /// AI request priority (bit 6; `FUN_00658830` uses `0x1c` = 28 when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    #[serde(flatten)]
    pub target: ObjectRef,
    /// The tools a `use_tool` relation needs (kind 14 only: `FUN_00658830` reads a `RefList`
    /// when `(flags & 0x1f) == 0xe`; the AI checks the held tool against it in `FUN_00656100`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<RefList>,
}

/// Tag list of a [`Relation`] (flag bit 5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelationTags {
    /// Any tag suffices (else all are required).
    pub match_any: bool,
    pub tags: Vec<scribble_core::NamedId>,
}

impl Relation {
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let f = r.u8()?;
        let tags = if f & 0x20 != 0 { Some(RelationTags { match_any: r.bool()?, tags: crate::util::read_tags(r, ctx)? }) } else { None };
        let priority = if f & 0x40 != 0 { Some(r.u8()?) } else { None };
        let target = ObjectRef::read(r, ctx)?;
        let kind = f & 0x1f;
        let tools = if kind == 0xe { Some(RefList::read(r, ctx)?) } else { None };
        Ok(Relation { kind: RelationKind::from_u8(kind), via_holder: f & 0x80 != 0, tags, priority, target, tools })
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        let kind = self.kind.to_u8();
        scribble_core::ensure!(kind < 0x20, "relation kind {kind} does not fit in 5 bits");
        scribble_core::ensure!((kind == 0xe) == self.tools.is_some(), "relation kind use_tool needs `tools` (and only it)");
        let f = kind | (self.tags.is_some() as u8) << 5 | (self.priority.is_some() as u8) << 6 | (self.via_holder as u8) << 7;
        w.u8(f);
        if let Some(t) = &self.tags {
            w.bool(t.match_any);
            crate::util::write_tags(&t.tags, w, ctx)?;
        }
        if let Some(p) = self.priority {
            w.u8(p);
        }
        self.target.write(w, ctx)?;
        if let Some(t) = &self.tools {
            t.write(w, ctx)?;
        }
        Ok(())
    }
}

/// A relationship record as added by actions and adjectives (`FUN_0043f190`): like
/// [`Relation`] but with an insertion position and no tag list.
///
/// ```text
/// u8 insert_at
/// u8 flags            bits 0-4 kind, bit6 has priority, bit7 via_holder (bit5 ignored)
/// [bit6] u8 priority
/// ObjectRef target
/// [kind == 14] RefList tools
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attitude {
    /// Where the relation is inserted into the object's list (`FUN_00658180`'s switch): 0 =
    /// front, 1-3 = after the `_adjective_adjective_adjectiveN` marker relation (object ids
    /// 0x1574-0x1576), 4 = after the leading runtime-added entries (flag bit 1), 5 = end.
    /// Shipped data uses 0-3.
    pub insert_at: u8,
    pub kind: RelationKind,
    /// See [`Relation::via_holder`].
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub via_holder: bool,
    /// Bit 5 of the flags byte: never read (`FUN_0043f190` takes bits 0-4, 6 and 7 only; the
    /// priority test is `(b >> 5) & 2`, i.e. bit 6); unset in shipped files. JSON `unused_bit5`.
    #[serde(default, rename = "unused_bit5", skip_serializing_if = "crate::util::is_false")]
    pub ignored_bit5: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<u8>,
    #[serde(flatten)]
    pub target: ObjectRef,
    /// The tools a `use_tool` relation needs (kind 14 only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<RefList>,
}

impl Attitude {
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let insert_at = r.u8()?;
        let f = r.u8()?;
        let priority = if f & 0x40 != 0 { Some(r.u8()?) } else { None };
        let target = ObjectRef::read(r, ctx)?;
        let kind = f & 0x1f;
        let tools = if kind == 0xe { Some(RefList::read(r, ctx)?) } else { None };
        Ok(Attitude {
            insert_at,
            kind: RelationKind::from_u8(kind),
            via_holder: f & 0x80 != 0,
            ignored_bit5: f & 0x20 != 0,
            priority,
            target,
            tools,
        })
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        let kind = self.kind.to_u8();
        scribble_core::ensure!(kind < 0x20, "relation kind {kind} does not fit in 5 bits");
        scribble_core::ensure!((kind == 0xe) == self.tools.is_some(), "relation kind use_tool needs `tools` (and only it)");
        w.u8(self.insert_at);
        w.u8(kind | (self.ignored_bit5 as u8) << 5 | (self.priority.is_some() as u8) << 6 | (self.via_holder as u8) << 7);
        if let Some(p) = self.priority {
            w.u8(p);
        }
        self.target.write(w, ctx)?;
        if let Some(t) = &self.tools {
            t.write(w, ctx)?;
        }
        Ok(())
    }
}
