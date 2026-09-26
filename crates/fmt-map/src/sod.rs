//! `.sod`: scene — everything placed in a level or event: objects with their scripts, merit
//! rules, object groups, liquids, rails, decorations, hints, lights, effects and doors.
//!
//! Loaded by `FUN_004bdab0` (level descriptor +0x04; event scenes are started from objects'
//! event records). A 32-bit flag word says which parts are present; they follow in a fixed
//! order, each read by its own function:
//!
//! ```text
//! u8  has_names          // object names, group names and merit markers are stored
//! u32 flags
//! bit0 : u16 budget_cost (skipped here; read by the level-table scan FUN_004e15e0)
//! bit1 : i16 event_id    (-> +0x148, or a save-slot lookup in type-6 levels)
//! bit2 : u16 persist_slot (-> +0x154, save slot of the persistent objects)
//! bit10: u16 offset of the liquids section (lets the engine re-read it alone)
//! bit3 : merit rules     (FUN_004bbe00)
//! bit15: unused markers  (FUN_004af310 -> FUN_00641b70, a bare `ret 4`)
//! bit4 : u8 n; n x object (see SceneObject)
//! bit5 : object groups   (FUN_004b6830)   bit6 : object links (FUN_004aea80)
//! bit7 : contents        (FUN_004aeb10)   bit8 : rope attachments (FUN_004aeba0)
//! bit9 : joints          (FUN_0066d4e0)   bit10: liquids (FUN_004b3220)
//! bit14: u8 r,g,b sky colour              bit11: rails (FUN_004aecf0)
//! bit12: camera bounds   (FUN_004af1e0)   bit13: decorations (FUN_006f4220)
//! bit16: hints           (FUN_004af400)   bit17: no-drop zones (FUN_004b3830)
//! bit19: survival waves  (FUN_004af6b0)   bit20: merit zones (FUN_004b39f0)
//! bit21: lights          (FUN_004b6a30)   bit23: atmospheres (FUN_004b7730)
//! bit24: u32 intro script                 bit25: effects (FUN_004bb8a0)
//! bit26: doors           (FUN_004b3ca0)
//! ```
//!
//! Bits 18 and 22 (never set in shipped files) are not supported; their loaders are
//! `FUN_004b3b50` (records parsed by `FUN_0047c310`) and `FUN_004b7180`.
//!
//! Placed objects get engine instance ids in load order starting with -1 for the stage entry
//! (`FUN_006997d0(obj, ..., loop_index - 1)`, resolved by `FUN_006a1610` through the instance
//! table `DAT_008a5bc0`): the object at `objects[n]` is instance `n - 1`. The object numbers of
//! links, contents, rope attachments and joints are such instance ids, like `{"object": n}`
//! entities in scripts and merit rules.
//!
//! A placed object (`FUN_004bdab0` loop):
//!
//! ```text
//! u16 object              // .so pmindex index; 0xFFFF = the stage (no body)
//! -- if object != 0xFFFF:
//!    u16 name_word        // .dtm word key, 0xFFFF = none (absent in the legacy layout)
//!    OBJECT_BODY          // position, flags, rotation, adjectives, attachments, event
//! -- if has_names: u8 len; char name[len]
//! u8 trigger_count
//! trigger_count x { -- if flags bit3: u16 merit; trigger; u8 n; n x action }
//! ```
//!
//! One unused event scene (`events\desert\e1_dunes.sod`) predates the `name_word` field and has
//! an older hint section; it is decoded with `legacy_objects` and its undecodable sections are
//! kept verbatim in `unparsed_sections`.

use crate::common::Rgb;
use crate::schema::{self, Record, F, F::*, P};
use crate::script::{read_triggers, write_triggers, SceneTrigger};
use fmt_object::refs::Filter;
use scribble_core::{bail, ensure, ns, Context, Format, Fx16 as Fixed16, Hex, NamedId, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    /// Byte 0: names of objects and groups and the merit markers are stored.
    pub has_names: bool,
    /// Flag bit 0: the object budget the placed objects use. `FUN_004bdab0` skips it
    /// (`if (flags & 1) local_280 = 7`), but the start-up scan of the level table
    /// (`FUN_004e15e0`: `*(uint *)(slot + 0x10) = *(ushort *)(sod + 5)`) stores it in the level
    /// descriptor, and entering the level trims carried objects until they fit in
    /// `0xfffffff - budget_cost` (`FUN_006831a0`/`FUN_006425b0` -> `FUN_006a7260`, which sums
    /// object costs obj+0x284 via `FUN_0069fdd0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_cost: Option<u16>,
    /// Flag bit 1: event id (-> +0x148), the save's completion bit for this scene (objects'
    /// `event.event_id` refer to it; `FUN_00454b90`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<i16>,
    /// Flag bit 2: save slot of this scene's persistent objects (mode+0x154). When the event
    /// completes (`FUN_004c1550` sets +0x17c) `FUN_00454510` writes every object with a persist
    /// id (obj+0x2a8 >= 0) into that slot (`FUN_00646fa0(slot + 5, ...)`, `FUN_006c90b0`). The
    /// three users (`e1_downtown` 4, `e1_museum` 18, `e2_oasis` 19) name the `event_id` slots of
    /// `s_downtown`, `s_museum` and `s_oasis`, whose groups hand out persist ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persist_slot: Option<u16>,
    /// Flag bit 3: rules linking merits (from the level's `.mdb`) to scene entities.
    /// When present, every trigger also names the merit it belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merits: Option<Vec<MeritRule>>,
    /// Flag bit 15: positions that `FUN_004af310` hands one by one to `FUN_00641b70`, which is
    /// a bare `ret 4` stub, so they are ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_markers: Option<Vec<[Fixed16; 2]>>,
    /// Flag bit 4: placed objects. Scripts refer to them by index (`{"object": n}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<Vec<SceneObject>>,
    /// Objects use the old layout without the `name_word` word.
    #[serde(default, skip_serializing_if = "crate::common::is_false")]
    pub legacy_objects: bool,
    /// Flag bit 5: named object groups (`$food$`, `$hat$`, ...): an object filter the scripts
    /// refer to as `{"group": n}` (entity id 0xFF0000nn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<ObjectGroup>>,
    /// Flag bit 6: objects attached to other objects (`FUN_004aea80` ->
    /// `FUN_006af730(inst(attached), inst(carrier), kind)`), e.g. hats worn by Maxwell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_links: Option<Vec<Record>>,
    /// Flag bit 7: objects put inside containers (`FUN_004aeb10`: `container.FUN_00672060(
    /// contents, 1, 1)`), e.g. a moneybag in a safe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<Vec<Record>>,
    /// Flag bit 8: objects tied to one end of a rope or chain (`FUN_004aeba0` ->
    /// `FUN_004d3980(end_segment, inst(object))`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rope_attachments: Option<Vec<Record>>,
    /// Flag bit 9: physics joints between two placed objects (`FUN_0066d4e0` -> `FUN_005cec50`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joints: Option<Vec<Record>>,
    /// Flag bit 10: liquid volumes (rectangles of water/lava with their textures and colours).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquids: Option<Vec<Record>>,
    /// Stored offset of the liquids section (flag bit 10, read right after the header words),
    /// kept only if it differs from the real one. `FUN_004bdab0` called with mode 1 jumps there
    /// to re-read only the liquids (`FUN_004b3220(param_2, &local_26c, 1)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquids_offset: Option<u16>,
    /// Flag bit 14: sky colour override; packed to RGB565 into level+0xb8 (the `.stp` sky colour
    /// slot) and applied by `FUN_004c5180`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sky_color: Option<Rgb>,
    /// Flag bit 11: rails/ropes between two end objects along a path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rails: Option<Rails>,
    /// Flag bit 12: camera bounds `x, y, width, height` (`FUN_004505c0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_bounds: Option<[i16; 4]>,
    /// Flag bit 13: background decorations (vector drawings and animations).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decorations: Option<Vec<Record>>,
    /// Flag bit 16: hint system data of an event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hints: Option<Record>,
    /// Flag bit 17: zones where objects cannot be dropped (`FUN_0053bb50`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_drop_zones: Option<Vec<Record>>,
    /// Flag bit 19: survival mode waves (`FUN_00702de0`); event scripts release them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub survival_waves: Option<Vec<Record>>,
    /// Flag bit 20: merit zones (`FUN_004b39f0`): each spawns a `zonefillbox` object at the
    /// rectangle's centre with +0x514 = its index, which merit rules and hints refer to as
    /// `{"zone": n}`. Never set in shipped files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merit_zones: Option<Vec<Record>>,
    /// Flag bit 21: lights.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lights: Option<Vec<Record>>,
    /// Flag bit 23: atmosphere overlays (`FUN_004b7730`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atmospheres: Option<Vec<Record>>,
    /// Flag bit 24: event script run when the scene starts (`<scene>_intro`): stored at
    /// mode+0xc0, run by `FUN_00455720`/`FUN_00462d30`/`FUN_00463350`
    /// (`if (mode[0x30] != -1) FUN_0045a060(mode[0x30])`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro_script: Option<ResRef>,
    /// Flag bit 25: particle effects placed in the level (`.gec`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effects: Option<Vec<Record>>,
    /// Flag bit 26: doors leading to other levels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doors: Option<Vec<Record>>,
    /// Sections the decoder could not interpret (legacy files only), kept verbatim with the
    /// flag bits that announce them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unparsed_sections: Option<UnparsedSections>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnparsedSections {
    pub flag_bits: Vec<u32>,
    pub data: Hex,
}

/// A merit rule (`FUN_004bbe00`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeritRule {
    /// The merit (named from the merit databases; `null` = 0xFFFF).
    pub merit: Option<NamedId>,
    /// Conditions: `{"entity": ...}` adds a scene entity that counts for the merit
    /// (`FUN_004f79c0`, merit +0x2c list, matched by `FUN_004f7a70`); `{"zone": n}` adds a merit
    /// zone index (`FUN_004f7b60`, merit +0x38 list; `FUN_004f7b80` checks it when
    /// `FUN_004b0830` builds zone n of the flag-bit-20 section). No shipped file uses zones.
    pub conditions: Vec<Value>,
    /// With `has_names`: editor markers of the merit's entities, inserted into a map at
    /// rule+0x14 (`FUN_004bb630`) that nothing looks up (only the destructor `FUN_006d6ab0`
    /// touches it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markers: Option<Vec<MeritMarker>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeritMarker {
    pub entity: Value,
    /// Where the object editor shows the marker (never read at runtime, see
    /// [`MeritRule::markers`]).
    pub editor_position: [u16; 2],
}

/// A placed object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneObject {
    /// Object type (`.so`); `null` for the stage entry, which only carries scripts.
    pub object: Option<ResRef>,
    /// Display word of the object (a `.dtm` object word key, `FUN_0073cac0` -> obj+0x162),
    /// `null` = its own name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_word: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Position, flags, rotation, adjectives, attachments and event (see `OBJECT_BODY`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties: Option<Record>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<SceneTrigger>,
}

/// A named object group (`FUN_006cb850`): every object matching `filter` belongs to it, and
/// scripts can address the group as `{"group": n}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectGroup {
    /// Flag bits: `manual_only` (bit 0 -> +0x04: `FUN_006cba10` returns 0 unless the caller asks
    /// for this group explicitly, and its only automatic caller `FUN_004c5190` passes 0, so such
    /// groups only get objects spawned into them by index), `has_member_setup` (bit 1),
    /// `unique_words` (bit 2 -> +0x39: `FUN_006cba10` rejects an object whose name word
    /// obj+0x162 is already in the group's word list +0x40).
    pub flags: Vec<String>,
    pub filter: Filter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Scripts attached to members of the group: a u32-length blob holding a trigger list in
    /// the object layout (`FUN_006cb010`: count, u16 merit when the scene has merit rules,
    /// trigger and actions).
    pub triggers: Vec<SceneTrigger>,
    /// Setup applied to members (flag bit 1): see [`MEMBER_SETUP`]. Kept as hex only if a blob
    /// does not match that layout (none in shipped files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_setup: Option<MemberSetup>,
}

/// A group's member setup: decoded fields, or the raw blob when it does not fit the layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MemberSetup {
    Fields(Record),
    Raw(Hex),
}

const GROUP_FLAGS: &[&str] = &["manual_only", "has_member_setup", "unique_words"];

/// Member setup of an object group (a u32-length blob; `FUN_004b8cd0` reads byte 0,
/// `FUN_006cb310` applies the rest to each object joining the group, asm 0x6cb325-0x6cb49d).
/// It is a compact form of a placed object's load flags, object flags and AI fields.
const MEMBER_SETUP: &[F] = &[
    // Load flags passed to the object loader: bit0 -> 0x10, bit1 -> 4, bit2 -> 2, bit3 -> 1.
    Flags("load_flags", &["no_default_equipment", "load_relations", "spawn_contents", "load_behaviours"]),
    // bit0 FUN_006857f0/FUN_0069ee00, bit1 FUN_006c49b0, bit2 obj+0xb30, bit3 obj+0x24e,
    // bit4 FUN_006a7470(obj, 0, 0, 1) (out of the world), bit5 body+0x80 |= 3 (sleeps),
    // bit6 obj+0x20f / FUN_006c44d0, bit7 obj+0x68c bit6.
    Flags("object_flags", &["draggable", "drag_movable", "locked", "intangible", "starts_hidden", "asleep", "immovable", "no_wander"]),
    // bits 0-1 obj+0x24d; bits 2-3 never read; bit4 FUN_00653b30 (AI+0x14a); bit5 obj+0x1a4
    // bit4; bit6 persist ids follow; bit7 another flag byte follows.
    Split(&[
        P::Enum("grabbable", 0x03, &["default", "yes", "no"], 0),
        P::Num("unused_bits", 0x0c, 0),
        P::Bool("always_sees_target", 0x10),
        P::Bool("drop_contents_in_place", 0x20),
        P::Bool("has_persist_ids", 0x40),
        P::Bool("has_more_flags", 0x80),
    ]),
    // bit0 never read; bit1 FUN_006bf3d0 (mirrored); bit2 wander range follows.
    If("has_more_flags", 1, &[Split(&[P::Bool("unused_bit0", 0x01), P::Bool("mirrored", 0x02), P::Bool("has_wander_range", 0x04), P::Num("unused_bits_2", 0xf8, 0)])]),
    // AI target instance (0xFF none) and relation kind -> FUN_00653b70 (AI+0x80, AI+0xdc).
    U8D("ai_target", 0xff),
    U8D("ai_target_relation", 4),
    // The n-th member gets persist id `persist_ids[group+0x38++]` (obj+0x2a8; >= 0x80 = none).
    If("has_persist_ids", 1, &[ArrayU8("persist_ids", 10)]),
    // Wander box, like OBJECT_BODY's (the engine ORs the two bytes of each word, 0x6cb47f).
    If("has_more_flags", 1, &[If("has_wander_range", 1, &[U16("wander_left"), U16("wander_right")])]),
];

fn read_member_setup(blob: &[u8], ctx: &Context) -> MemberSetup {
    let mut r = Reader::new(blob);
    match schema::decode(MEMBER_SETUP, &mut r, ctx) {
        Ok(rec) if r.at_end() => {
            let mut w = Writer::new();
            match schema::encode(MEMBER_SETUP, &rec, &mut w, ctx) {
                Ok(()) if w.into_inner() == blob => MemberSetup::Fields(rec),
                _ => MemberSetup::Raw(Hex(blob.to_vec())),
            }
        }
        _ => MemberSetup::Raw(Hex(blob.to_vec())),
    }
}

fn read_groups(r: &mut Reader, ctx: &Context, has_names: bool, merit_ids: bool) -> Result<Vec<ObjectGroup>> {
    let n = r.u8()?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let f = r.u8()?;
        let flags = crate::stp::flag_names(f as u32, GROUP_FLAGS, 8);
        let filter = Filter::read(r, ctx)?;
        let name = if has_names { Some(r.str_u8()?) } else { None };
        let len = r.u32()? as usize;
        let blob = r.bytes(len)?;
        let mut br = Reader::new(blob);
        let triggers = read_triggers(&mut br, ctx, merit_ids)?;
        br.expect_end()?;
        let member_setup = if f & 2 != 0 {
            let len = r.u32()? as usize;
            Some(read_member_setup(r.bytes(len)?, ctx))
        } else {
            None
        };
        out.push(ObjectGroup { flags, filter, name, triggers, member_setup });
    }
    Ok(out)
}

fn write_groups(w: &mut Writer, groups: &[ObjectGroup], ctx: &Context, has_names: bool, merit_ids: bool) -> Result<()> {
    w.u8(u8::try_from(groups.len())?);
    for g in groups {
        let f = crate::stp::flag_value(&g.flags, GROUP_FLAGS)?;
        ensure!((f & 2 != 0) == g.member_setup.is_some(), "group flag has_member_setup must match the presence of member_setup");
        w.u8(u8::try_from(f)?);
        g.filter.write(w, ctx)?;
        match (&g.name, has_names) {
            (Some(n), true) => {
                w.str_u8(n)?;
            }
            (None, false) => {}
            _ => bail!("group names must be present exactly when has_names is set"),
        }
        let mut bw = Writer::new();
        write_triggers(&mut bw, &g.triggers, ctx, merit_ids)?;
        let blob = bw.into_inner();
        w.u32(blob.len() as u32).bytes(&blob);
        if let Some(e) = &g.member_setup {
            let blob = match e {
                MemberSetup::Fields(rec) => {
                    let mut mw = Writer::new();
                    schema::encode(MEMBER_SETUP, rec, &mut mw, ctx)?;
                    mw.into_inner()
                }
                MemberSetup::Raw(h) => h.0.clone(),
            };
            w.u32(blob.len() as u32).bytes(&blob);
        }
    }
    Ok(())
}

/// Rails (`FUN_004aecf0`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rails {
    /// Paths, each with an object at both ends (`FUN_005b81d0`).
    pub paths: Vec<Record>,
    /// Survival mode enemy paths (`FUN_007056e0`: survival-manager path slot `+0x118[i]`).
    pub survival_paths: Vec<Record>,
    /// Survival mode defender positions (`FUN_00705760`); present exactly when flag bit 19
    /// (survival waves) is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub survival_defenders: Option<Record>,
}

/// Event record of an object (`FUN_00672c40`): the object starts an event scene.
const EVENT: &[F] = &[
    Flags("flags", &["has_marker_kind", "has_preview", "has_text", "has_event_id", "has_margins", "has_camera"]),
    Res32("event"),
    // Save completion bit of the event (= the target scene's `event_id`; FUN_00673060).
    If("flags", 8, &[U16("event_id")]),
    // Texture of the `potalpreview.previews.PreviewSlot` panel (FUN_004e07c0; default 0x671b).
    If("flags", 2, &[Res32("preview")]),
    // Label/description strings (FUN_004e0540 -> FUN_00499a30; FUN_00673aa0 marker label).
    If("flags", 4, &[Res32("text_table"), U8("text_index")]),
    // Marker icon (FUN_00673aa0).
    // 0 event icon (sf_eventicon / sf_eventcompleted once the save bit is set); 1 and 3
    // multiplayicon (FUN_006734d0 switches to mode 7 / 8); 2 no icon (FUN_00673e80 passes
    // show = 0; starts mode 4); 4 completed icon with sparkle; 5 and 7 storefronticon; 6
    // storefrontinfo. Kinds 3 and 7 are named after the icon they share.
    If("flags", 1, &[Enum8("marker_kind", &["event", "multiplayer", "event_no_icon", "multiplayer_3", "completed", "storefront", "storefront_info", "storefront_7"])]),
    // Proximity box around the object, in tiles (default 1.0 each).
    If("flags", 0x10, &[Fx16("margin_top"), Fx16("margin_left"), Fx16("margin_right"), Fx16("margin_bottom")]),
    // Camera when the event starts (FUN_006734d0 reads +0x34/+0x38 in tiles, offsets them by
    // the anchor and calls FUN_0044ff40; FUN_00673480; zoom 0..1 between min and max).
    If("flags", 0x20, &[Enum8("camera_anchor", &["center", "top_left", "top_right", "bottom_left", "bottom_right"]), Fx16("camera_x"), Fx16("camera_y"), Fx12("camera_zoom")]),
];

/// Body of a placed object (after `object` and `name_word`; `FUN_004bdab0`, flags go to the
/// object loader `FUN_006bbd70`).
const OBJECT_BODY: &[F] = &[
    Fx16("x"),
    Fx16("y"),
    // Load flag 1: 0 skips the .so behaviour block.
    U8D("load_behaviours", 1),
    Flags("load_flags", &["spawn_contents", "drop_contents_in_place"]),
    // Bit 0 = load flag 4 (keep the .so relations); bits 1-7 = animation slot + 1 to start in
    // (0 = idle; FUN_006b5440).
    Split(&[P::Num("load_relations", 0x01, 1), P::Num("initial_animation", 0xfe, 0)]),
    // Overrides the .so rope segment count.
    U8D("rope_segments", 0),
    Flags(
        "object_flags",
        &["immovable", "mirrored", "no_default_equipment", "no_wander", "draggable", "always_sees_target", "starts_inactive", "starts_hidden"],
    ),
    // bits 0-1 obj+0x24d; bit 2 never read (asm 0x4be11c-0x4be163 extracts bits 0-1, 3-7);
    // bit 3 physics body sleeps; bit 4 obj+0xb30 (not selectable or editable); bit 5
    // obj+0x252; bit 6 obj+0x24e.
    Split(&[
        P::Enum("grabbable", 0x03, &["default", "yes", "no"], 0),
        P::Bool("unused_bit2", 0x04),
        P::Bool("asleep", 0x08),
        P::Bool("locked", 0x10),
        P::Bool("drag_movable", 0x20),
        P::Bool("intangible", 0x40),
        P::Bool("has_event", 0x80),
    ]),
    // bit 2 -> obj+0x40e, only ever written or saved (FUN_006b1d00, FUN_006c84a0), never used.
    Flags("optional_fields", &["has_persist_id", "has_appearance_variant", "unused_40e", "has_wander_range"]),
    Fx12Z("rotation"),
    // Instance id the AI targets (0xFF = none; FUN_006a1770 -> FUN_00653b70, AI+0x80) and the
    // relation kind it uses (default protect; AI+0xdc).
    U8D("ai_target", 0xff),
    U8D("ai_target_relation", 4),
    // Kept across level exits (obj+0x2a8; FUN_00454510).
    If("optional_fields", 1, &[U8("persist_id")]),
    // 1-based appearance variant, 0xFF = random.
    If("optional_fields", 2, &[U8("appearance_variant")]),
    // Wander box (pixels left/right of the start; FUN_006639b0).
    If("optional_fields", 8, &[U16("wander_left"), U16("wander_right")]),
    Adjectives("adjectives"),
    ListRes16("attachments"),
    If("has_event", 1, &[Group("event", EVENT)]),
    // obj+0x247 (-1 = keep) and obj+0x248 sort order.
    I8D("draw_layer", -1),
    I8D("draw_order", 0),
];

/// `FUN_006af730`: instance `attached` rides / is held by / is worn by / is stuck to instance
/// `carrier` (a hat worn by Maxwell, a balloon stuck to a clown).
const OBJECT_LINK: &[F] = &[U8("attached"), U8("carrier"), Enum8("kind", &["ride", "held", "equipped", "stuck"])];
/// `FUN_004aeb10`: instance `contents` is put into instance `container` (`FUN_00672060`).
const CONTENTS: &[F] = &[U8("contents"), U8("container")];
/// `FUN_004aeba0` (asm 0x4aebd6): `end` 1 = the rope's first segment (rope+0x6cc), 0 = its last
/// (rope+0x6d0) (`FUN_005ebf50`); instance `object` is tied to it (`FUN_004d3980`).
const ROPE_ATTACHMENT: &[F] = &[Enum8("end", &["end", "start"]), U16("rope"), U16("object")];

/// `FUN_005cec50` between instances `object_a` and `object_b`; -1 = the player
/// (`DAT_008a8700+0x184`). Types 1, 4 and 5 hit the `default:` case and create no joint.
const JOINT: &[F] = &[
    Enum8("type", &["pin", "none_1", "weld", "elastic", "none_4", "none_5", "motor", "motor_limited"]),
    // Anchor in tiles (matches the objects' positions).
    Fx16("anchor_x"),
    Fx16("anchor_y"),
    // Percent; >= 100 = rigid.
    U8("stiffness"),
    I8("object_a"),
    I8("object_b"),
    IfMaskEq("type", 0xff, 6, &[Fx12("motor_speed")]),
    IfMaskEq("type", 0xff, 7, &[Fx12("motor_speed"), Fx12("min_angle"), Fx12("max_angle")]),
];

/// `FUN_004b3220` (defaults `FUN_005e2d30`), built by `FUN_005e2f70` / `FUN_00739820`; waves
/// simulated by `FUN_00739de0`.
const LIQUID: &[F] = &[
    // Low nibble: 0 water (internal type 2), 1 lava (type 3; FUN_00739820 `+0x4d = type == 3`),
    // others type 0. Bit 4 is never tested. `full_width`: x = 0, width = map width, height =
    // map height - y.
    Split(&[
        P::Enum("kind", 0x0f, &["water", "lava"], 0),
        P::Bool("unused_bit4", 0x10),
        P::Bool("styled", 0x20),
        P::Bool("has_flow", 0x40),
        P::Bool("full_width", 0x80),
    ]),
    Fx16("x"),
    Fx16("y"),
    Fx16("width"),
    Fx16("height"),
    // Texture scroll per frame (body+0xe4; FUN_00739de0 `+0x80 += speed`).
    If("has_flow", 1, &[U8("flow_speed")]),
    If(
        "styled",
        1,
        &[
            // Defaults (FUN_005e2d30): watercolor, waterdistortion, waterfresnel, watermeniscus.
            Res32("color_texture"),
            Res32("distortion_texture"),
            Res32("fresnel_texture"),
            Res32("meniscus_texture"),
            // Spring term (+0x60) and velocity damping (+0x64) of the wave simulation.
            Fx12("wave_speed"),
            Fx12("wave_damping"),
            // Stored at liquid+0x68 by FUN_00739820 and never read (default 0.5).
            Fx12("unused_wave_param"),
            // Each colour: one byte skipped, then r, g, b packed to 555 (FUN_004ae190).
            // Surface colour -> +0x58 (top-row vertices, FUN_00738f50), body colour -> +0x5a
            // (lower vertices), the third -> +0x5c, never read.
            U8D("unused_surface_color_byte", 0xff),
            Color3("surface_color"),
            U8D("unused_body_color_byte", 0xff),
            Color3("body_color"),
            U8D("unused_color_byte", 0xff),
            Color3("unused_color"),
            // *31/255 -> +0x5e; FUN_00737f00 caps the surface alpha at it (`min(alpha, 2*x)`).
            U8("alpha"),
            // Defaults water_center, water_fade01.
            Res32("center_texture"),
            Res32("fade_texture"),
        ],
    ),
];

/// A path between two end objects (`FUN_005b81d0`, `FUN_005b7d20`, `FUN_005b7910`).
const RAIL_PATH: &[F] = &[
    // FUN_005bbd00 at an end: 1 only travels forward (stops at the start), 2 only backward,
    // 3 both ways (default).
    Enum8("direction", &["", "forward_only", "backward_only", "both"]),
    // Bit 0 is never tested by the parser; bit 1 -> +5 (easing, FUN_005b7910).
    Flags("flags", &["unused_bit0", "has_easing", "has_start_box", "has_end_box", "has_ease_steps"]),
    // Percent, clamped to 1..100 (-> +0xc / +0x10).
    U8("speed_forward"),
    U8("speed_backward"),
    // Animation slots played along the path (FUN_00665570 -> FUN_00667e00; 33 = walk,
    // 3 = climb).
    U8("anim_forward"),
    U8("anim_backward"),
    // Number of points over which the speed ramps up / down at the ends (+6 / +7).
    If("flags", 0x10, &[U8("ease_in"), U8("ease_out")]),
    If("flags", 4, &[I16("start_x"), I16("start_y"), I16("start_width"), I16("start_height")]),
    If("flags", 8, &[I16("end_x"), I16("end_y"), I16("end_width"), I16("end_height")]),
    List("points", &[Fx16("x"), Fx16("y")]),
];

/// Survival mode enemy paths (`FUN_007056e0`, into survival-manager slot `+0x118[i]`): world
/// pixel positions. The leading byte is skipped (`*param_3 = *param_3 + 1` in `FUN_004aecf0`).
const SURVIVAL_PATH: &[F] = &[U8D("unused", 0), List("points", &[Fx12("x"), Fx12("y")])];

/// Survival mode defender positions (`FUN_00705760`): every point of every list is appended
/// to the one path at +0x684, and `FUN_00704a80` puts defender i at point i (i < 6). Each
/// point's optional byte is stored at `mgr+0x5e8+i` and never read.
const SURVIVAL_DEFENDERS: &[F] = &[
    Flags("flags", &["has_defender_positions"]),
    If(
        "flags",
        1,
        &[List(
            "defender_slots",
            &[Flags("flags", &["has_unused_bytes"]), List("defender_positions", &[Fx12("x"), Fx12("y"), If("flags", 1, &[U8("unused_byte")])])],
        )],
    ),
];

/// Background decorations (`FUN_006f4220`: `if (cVar1 < 1) FUN_006f3810 else FUN_006f3f20`):
/// kind 0 places a `.sao`, kind 1 a texture / vector / animation. Sort key
/// `depth & 0xfff | layer << 12` (FUN_006e9010, FUN_006f3f20).
const DECORATION: &[F] = &[
    Enum8("kind", &["object", "vector"]),
    IfMaskEq(
        "kind",
        0xff,
        0,
        &[
            // Bit 1: horizontal flip (FUN_006e9010 param_5: `+0x14 = -scale`).
            Flags("flags", &["has_position", "mirrored", "has_rotation", "has_scale", "has_object", "has_layer", "has_depth"]),
            If("flags", 1, &[Fx16("x"), Fx16("y")]),
            If("flags", 4, &[Fx12("rotation")]),
            If("flags", 8, &[Fx12("scale")]),
            If("flags", 0x10, &[Res32("simple_object")]),
            // Draw layer (default 1; 0 = background) and depth (default 100).
            If("flags", 0x20, &[I8("layer")]),
            If("flags", 0x40, &[U16("depth")]),
        ],
    ),
    IfMaskEq(
        "kind",
        0xff,
        1,
        &[
            Flags("flags", &["has_position", "has_texture", "has_vector", "has_animation", "has_layer", "has_depth", "has_scale"]),
            If("flags", 1, &[Fx16("x"), Fx16("y")]),
            If("flags", 2, &[Res32("texture")]),
            If("flags", 4, &[Res32("vector")]),
            If("flags", 8, &[Res32("animation")]),
            If("flags", 0x10, &[U8("layer")]),
            If("flags", 0x20, &[U16("depth")]),
            If("flags", 0x40, &[Fx12("scale")]),
        ],
    ),
];

/// Hints of an event (`FUN_004af400`, `FUN_004d2180`). The leading byte is skipped
/// (`*param_2 = *param_2 + 1`).
const HINTS: &[F] = &[
    U8D("unused", 0),
    // Sub-pieces of each `__ufsprogress` bar segment (FUN_00454010 -> FUN_0047f020).
    ListU8("progress_segments"),
    Res32("text_table"),
    // Steps are consumed in groups of these sizes (FUN_004d2260 -> +0x2c).
    ListU8("group_sizes"),
    List(
        "steps",
        &[
            // Seconds before the hint unlocks (`* 1000`, FUN_004d2180).
            U16("unlock_delay"),
            // Unlocked by touching `entity` (FUN_0047e650) or entering merit zone `zone`
            // (FUN_0047e690, compares zone object +0x514).
            List("conditions", &[Enum8("kind", &["entity", "zone"]), IfMaskEq("kind", 0xff, 0, &[Entity("entity")]), IfMaskEq("kind", 0xff, 1, &[U8("zone")])]),
        ],
    ),
];

/// Rectangles (`FUN_004b3830` no-drop zones -> `FUN_0053bb50`, `FUN_005e74a0`; `FUN_004b39f0`
/// merit zones). The leading byte is skipped (`*param_3 = *param_3 + 1`).
const AREA: &[F] = &[U8D("unused", 0), Fx16("x"), Fx16("y"), Fx16("width"), Fx16("height")];

/// A survival mode wave (`FUN_00702de0` / `FUN_00707430`): `min_count`..`max_count` of the
/// `objects`, with weapons and random adjectives by chance, entering along `spawn_paths`.
const SURVIVAL_WAVE: &[F] = &[
    Flags("flags", &["has_weapons", "has_objects", "has_adjectives", "has_spawn_paths", "has_unused_58", "has_stats"]),
    U8("min_count"),
    U8("max_count"),
    // Copied into each spawn (FUN_00707430); hit points via enemy vtable +0x84 (0x700090),
    // damage via +0x90 (0x700100). Defaults 25 / 10.
    If("flags", 0x20, &[U16("health"), U16("damage")]),
    // Stored at wave+0x58 (0x702ea5), never read.
    If("flags", 0x10, &[U8("unused_58")]),
    If("flags", 2, &[ListRes32("objects")]),
    If("flags", 1, &[U8("weapon_chance"), ListRes32("weapons")]),
    If(
        "flags",
        4,
        &[
            U8("adjective_chance_1"),
            ListRes32("adjectives_1"),
            U8("adjective_chance_2"),
            ListRes32("adjectives_2"),
            U8("adjective_chance_3"),
            ListRes32("adjectives_3"),
            U8("adjective_chance_4"),
            ListRes32("adjectives_4"),
        ],
    ),
    If("flags", 8, &[ListU8("spawn_paths")]),
];

/// Light shafts (`FUN_004b6a30`; vertex attributes of lightshaft.gp: `aPosition` = x, y,
/// `aDirection`, `aParam` = (angle, start_distance, end_distance, brightness), `aRate` =
/// the two shimmer scroll rates of the aperture texture, `aColor`). The first 4 bytes are skipped
/// (`*param_3 = *param_3 + 4`); layer is clamped to <= 3, sort key `depth & 0xfff | layer << 12`;
/// colour bytes are `a r g b` (/255).
const LIGHT: &[F] = &[
    Bytes("unused", 4),
    Res32("aperture_texture"),
    Res32("attenuation_texture"),
    U8("layer"),
    U16("depth"),
    Fx12("x"),
    Fx12("y"),
    Fx12("direction_x"),
    Fx12("direction_y"),
    Fx12("angle"),
    U16("start_distance"),
    U16("end_distance"),
    Fx12("brightness"),
    Color4("color"),
    Fx12("shimmer_rate_1"),
    Fx12("shimmer_rate_2"),
];

/// One detail layer of an atmosphere: a texture scrolling in three directions; the values
/// are the shader uniforms `detail<N>velocity<M>` / `detail<N>period<M>` of atmosphere.gp.
/// The leading byte is skipped (`FUN_004b7730` reads the texture at +1).
const ATMOSPHERE_DETAIL: &[F] = &[
    U8D("unused", 0),
    Res32("texture"),
    I32("velocity_x_0"),
    I32("velocity_y_0"),
    I32("period_0"),
    I32("velocity_x_1"),
    I32("velocity_y_1"),
    I32("period_1"),
    I32("velocity_x_2"),
    I32("velocity_y_2"),
    I32("period_2"),
];

/// Atmosphere overlays (`FUN_004b7730`; parameters of atmosphere.gp). The first 4 bytes are
/// skipped (`*param_3 = *param_3 + 4`).
const ATMOSPHERE: &[F] = &[
    Bytes("unused", 4),
    Res32("mask_texture"),
    U8("layer"),
    U16("depth"),
    Fx12("x"),
    Fx12("y"),
    Fx12("width"),
    Fx12("height"),
    Fx12("brightness"),
    Color4("color"),
    Group("detail_0", ATMOSPHERE_DETAIL),
    Group("detail_1", ATMOSPHERE_DETAIL),
];

/// Particle effects placed in the level (`FUN_004bb8a0`). `paused`: `FUN_00600150(effect,
/// ~flags & 1, 0)` sets the handle's playing state; bits 1-7 are never tested.
const EFFECT: &[F] = &[
    Flags("flags", &["paused"]),
    U8("layer"),
    U16("depth"),
    Fx16("x"),
    Fx16("y"),
    Fx12("rotation"),
    Res32("effect"),
];

/// Doors to other levels (`FUN_004b3ca0`; state machine `FUN_004de5f0`, decoration
/// `FUN_004de700`).
const DOOR: &[F] = &[
    Flags("flags", &["has_level", "has_script", "has_decoration", "has_unused_offset"]),
    Fx16("x"),
    Fx16("y"),
    Fx16("width"),
    Fx16("height"),
    If(
        "flags",
        1,
        &[
            // FUN_004e1a50(tile_map, scene, setup, merits, 0) takes these four.
            Res32("scene"),
            Res32("setup"),
            Res32("tile_map"),
            Res32("merits"),
            // Record bytes 0x21-0x28 are never read (the loader continues at +0x29).
            Res32("unused_dependencies"),
            Res32("unused_preload"),
            // -> door trigger +0x38.
            Res32("preview"),
            // Event script (-> +0x48, -1 without has_script).
            If("flags", 2, &[Res32("script")]),
        ],
    ),
    // Index into `decorations` (shows `close_sequence` at once); its .sfb sequences played on
    // touch (state 3 -> 0) and on leaving (state 1 -> 2).
    If("flags", 4, &[U8("decoration"), U8("open_sequence"), U8("close_sequence")]),
    // Skipped (`*param_2 = *param_2 + 8`); in the data an (x, y) 20.12 pair like (0, -25).
    If("flags", 8, &[Bytes("unused_offset", 8)]),
    // Sound ids (-1 = none): on touch, while in the doorway (FUN_004de2f0), on leaving.
    I32("open_sound"),
    I32("doorway_sound"),
    I32("close_sound"),
];

fn read_list(fields: &[F], r: &mut Reader, ctx: &Context) -> Result<Vec<Record>> {
    let n = r.u8()?;
    (0..n).map(|_| schema::decode(fields, r, ctx)).collect()
}

fn write_list(fields: &[F], items: &[Record], w: &mut Writer, ctx: &Context) -> Result<()> {
    w.u8(u8::try_from(items.len())?);
    for it in items {
        schema::encode(fields, it, w, ctx)?;
    }
    Ok(())
}

fn read_merits(r: &mut Reader, ctx: &Context, has_names: bool) -> Result<Vec<MeritRule>> {
    let n = r.u8()?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let merit = r.u16()?;
        let merit = (merit != u16::MAX).then(|| NamedId::from_id(merit as u32, ns::MERIT, None, ctx));
        let k = r.u8()?;
        let mut conditions = Vec::with_capacity(k as usize);
        for _ in 0..k {
            let mut m = serde_json::Map::new();
            match r.u8()? {
                0 => m.insert("entity".into(), schema::entity_to_json(r.u32()?)),
                1 => m.insert("zone".into(), Value::from(r.u8()?)),
                v => bail!("unknown merit condition kind {v}"),
            };
            conditions.push(Value::Object(m));
        }
        let markers = if has_names {
            let m = r.u8()?;
            let mut v = Vec::with_capacity(m as usize);
            for _ in 0..m {
                let entity = schema::entity_to_json(r.u32()?);
                v.push(MeritMarker { entity, editor_position: [r.u16()?, r.u16()?] });
            }
            Some(v)
        } else {
            None
        };
        out.push(MeritRule { merit, conditions, markers });
    }
    Ok(out)
}

fn write_merits(w: &mut Writer, merits: &[MeritRule], ctx: &Context, has_names: bool) -> Result<()> {
    w.u8(u8::try_from(merits.len())?);
    for m in merits {
        let merit = match &m.merit {
            Some(id) => u16::try_from(id.to_id(ns::MERIT, None, ctx)?)?,
            None => u16::MAX,
        };
        w.u16(merit).u8(u8::try_from(m.conditions.len())?);
        for c in &m.conditions {
            let o = c.as_object().filter(|o| o.len() == 1).ok_or_else(|| anyhow::anyhow!("bad merit condition {c}"))?;
            if let Some(e) = o.get("entity") {
                w.u8(0).u32(schema::json_to_entity(e)?);
            } else if let Some(v) = o.get("zone") {
                w.u8(1).u8(u8::try_from(v.as_u64().ok_or_else(|| anyhow::anyhow!("bad value {v}"))?)?);
            } else {
                bail!("bad merit condition {c}");
            }
        }
        match (&m.markers, has_names) {
            (Some(ms), true) => {
                w.u8(u8::try_from(ms.len())?);
                for mk in ms {
                    w.u32(schema::json_to_entity(&mk.entity)?).u16(mk.editor_position[0]).u16(mk.editor_position[1]);
                }
            }
            (None, false) => {}
            _ => bail!("merit markers must be present exactly when has_names is set"),
        }
    }
    Ok(())
}

fn read_objects(r: &mut Reader, ctx: &Context, has_names: bool, merit_ids: bool, legacy: bool) -> Result<Vec<SceneObject>> {
    let n = r.u8()?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let id = r.u16()?;
        let object = (id != u16::MAX).then(|| ResRef::from_index(id as u32, ctx));
        let mut name_word = None;
        let mut properties = None;
        if id != u16::MAX {
            if !legacy {
                let v = r.u16()?;
                name_word = (v != u16::MAX).then_some(v);
            }
            properties = Some(schema::decode(OBJECT_BODY, r, ctx)?);
        }
        let name = if has_names { Some(r.str_u8()?) } else { None };
        let triggers = read_triggers(r, ctx, merit_ids)?;
        out.push(SceneObject { object, name_word, name, properties, triggers });
    }
    Ok(out)
}

fn write_objects(w: &mut Writer, objects: &[SceneObject], ctx: &Context, has_names: bool, merit_ids: bool, legacy: bool) -> Result<()> {
    w.u8(u8::try_from(objects.len())?);
    for o in objects {
        match &o.object {
            None => {
                w.u16(u16::MAX);
                ensure!(o.properties.is_none(), "the stage entry (object null) has no properties");
            }
            Some(r) => {
                let i = r.to_index(ctx)?;
                w.u16(u16::try_from(i).map_err(|_| anyhow::anyhow!("object index {i} does not fit in 16 bits"))?);
                if !legacy {
                    w.u16(o.name_word.unwrap_or(u16::MAX));
                }
                let p = o.properties.as_ref().ok_or_else(|| anyhow::anyhow!("object needs properties"))?;
                schema::encode(OBJECT_BODY, p, w, ctx)?;
            }
        }
        match (&o.name, has_names) {
            (Some(n), true) => {
                w.str_u8(n)?;
            }
            (None, false) => {}
            _ => bail!("object names must be present exactly when has_names is set"),
        }
        write_triggers(w, &o.triggers, ctx, merit_ids)?;
    }
    Ok(())
}

/// Order in which the optional sections after the objects are stored.
const SECTION_ORDER: [u32; 19] = [5, 6, 7, 8, 9, 10, 14, 11, 12, 13, 16, 17, 19, 20, 21, 23, 24, 25, 26];
const KNOWN_FLAGS: u32 = 0x07ff_ffff & !(1 << 18 | 1 << 22);

struct Decoder<'a> {
    ctx: &'a Context,
    has_names: bool,
    flags: u32,
}

impl Decoder<'_> {
    fn section(&self, bit: u32, r: &mut Reader, s: &mut Scene) -> Result<()> {
        let ctx = self.ctx;
        match bit {
            5 => s.groups = Some(read_groups(r, ctx, self.has_names, self.flags & 8 != 0)?),
            6 => s.object_links = Some(read_list(OBJECT_LINK, r, ctx)?),
            7 => s.contents = Some(read_list(CONTENTS, r, ctx)?),
            8 => s.rope_attachments = Some(read_list(ROPE_ATTACHMENT, r, ctx)?),
            9 => s.joints = Some(read_list(JOINT, r, ctx)?),
            10 => s.liquids = Some(read_list(LIQUID, r, ctx)?),
            14 => s.sky_color = Some(Rgb(r.array()?)),
            11 => {
                let paths = read_list(RAIL_PATH, r, ctx)?;
                let survival_paths = read_list(SURVIVAL_PATH, r, ctx)?;
                let survival_defenders = if self.flags & 1 << 19 != 0 { Some(schema::decode(SURVIVAL_DEFENDERS, r, ctx)?) } else { None };
                s.rails = Some(Rails { paths, survival_paths, survival_defenders });
            }
            12 => s.camera_bounds = Some([r.i16()?, r.i16()?, r.i16()?, r.i16()?]),
            13 => s.decorations = Some(read_list(DECORATION, r, ctx)?),
            16 => s.hints = Some(schema::decode(HINTS, r, ctx)?),
            17 => s.no_drop_zones = Some(read_list(AREA, r, ctx)?),
            19 => s.survival_waves = Some(read_list(SURVIVAL_WAVE, r, ctx)?),
            20 => s.merit_zones = Some(read_list(AREA, r, ctx)?),
            21 => s.lights = Some(read_list(LIGHT, r, ctx)?),
            23 => s.atmospheres = Some(read_list(ATMOSPHERE, r, ctx)?),
            24 => s.intro_script = Some(ResRef::from_index(r.u32()?, ctx)),
            25 => s.effects = Some(read_list(EFFECT, r, ctx)?),
            26 => s.doors = Some(read_list(DOOR, r, ctx)?),
            _ => unreachable!(),
        }
        Ok(())
    }
}

fn decode_scene(data: &[u8], ctx: &Context, legacy: bool) -> Result<Scene> {
    let mut r = Reader::new(data);
    let has_names = match r.u8()? {
        0 => false,
        1 => true,
        v => bail!("unexpected has_names byte {v}"),
    };
    let flags = r.u32()?;
    ensure!(flags & !KNOWN_FLAGS == 0, "unsupported scene flags {:#x}", flags & !KNOWN_FLAGS);
    let has = |b: u32| flags >> b & 1 != 0;
    let mut s = Scene {
        has_names,
        budget_cost: None,
        event_id: None,
        persist_slot: None,
        merits: None,
        unused_markers: None,
        objects: None,
        legacy_objects: legacy,
        groups: None,
        object_links: None,
        contents: None,
        rope_attachments: None,
        joints: None,
        liquids: None,
        liquids_offset: None,
        sky_color: None,
        rails: None,
        camera_bounds: None,
        decorations: None,
        hints: None,
        no_drop_zones: None,
        survival_waves: None,
        merit_zones: None,
        lights: None,
        atmospheres: None,
        intro_script: None,
        effects: None,
        doors: None,
        unparsed_sections: None,
    };
    if has(0) {
        s.budget_cost = Some(r.u16()?);
    }
    if has(1) {
        s.event_id = Some(r.i16()?);
    }
    if has(2) {
        s.persist_slot = Some(r.u16()?);
    }
    let stored_offset = if has(10) { Some(r.u16()?) } else { None };
    if has(3) {
        s.merits = Some(read_merits(&mut r, ctx, has_names)?);
    }
    if has(15) {
        let n = r.u8()?;
        s.unused_markers = Some((0..n).map(|_| Ok([Fixed16(r.i32()?), Fixed16(r.i32()?)])).collect::<Result<_>>()?);
    }
    if has(4) {
        s.objects = Some(read_objects(&mut r, ctx, has_names, has(3), legacy)?);
    }
    let dec = Decoder { ctx, has_names, flags };
    for (i, &bit) in SECTION_ORDER.iter().enumerate() {
        if !has(bit) {
            continue;
        }
        let start = r.pos();
        if bit == 10 {
            let off = stored_offset.unwrap();
            if off as usize != start {
                s.liquids_offset = Some(off);
            }
        }
        let res = dec.section(bit, &mut r, &mut s);
        if let Err(e) = res {
            if !legacy {
                return Err(e.context(format!("section bit {bit} at {start:#x}")));
            }
            let flag_bits = SECTION_ORDER[i..].iter().copied().filter(|&b| has(b)).collect();
            r.seek(start)?;
            if has(10) && start <= stored_offset.unwrap() as usize {
                s.liquids_offset = stored_offset;
            }
            s.unparsed_sections = Some(UnparsedSections { flag_bits, data: Hex(r.rest().to_vec()) });
            break;
        }
    }
    r.expect_end()?;
    Ok(s)
}

impl Format for Scene {
    const NAME: &'static str = "sod";
    const DESCRIPTION: &'static str = "Scene: placed objects, scripts, merit rules and level features";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        match decode_scene(data, ctx, false) {
            Ok(s) => Ok(s),
            Err(e) => decode_scene(data, ctx, true).map_err(|_| e),
        }
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let s = self;
        let bit = |b: bool, n: u32| (b as u32) << n;
        let mut flags = bit(s.budget_cost.is_some(), 0)
            | bit(s.event_id.is_some(), 1)
            | bit(s.persist_slot.is_some(), 2)
            | bit(s.merits.is_some(), 3)
            | bit(s.objects.is_some(), 4)
            | bit(s.groups.is_some(), 5)
            | bit(s.object_links.is_some(), 6)
            | bit(s.contents.is_some(), 7)
            | bit(s.rope_attachments.is_some(), 8)
            | bit(s.joints.is_some(), 9)
            | bit(s.liquids.is_some(), 10)
            | bit(s.rails.is_some(), 11)
            | bit(s.camera_bounds.is_some(), 12)
            | bit(s.decorations.is_some(), 13)
            | bit(s.sky_color.is_some(), 14)
            | bit(s.unused_markers.is_some(), 15)
            | bit(s.hints.is_some(), 16)
            | bit(s.no_drop_zones.is_some(), 17)
            | bit(s.survival_waves.is_some(), 19)
            | bit(s.merit_zones.is_some(), 20)
            | bit(s.lights.is_some(), 21)
            | bit(s.atmospheres.is_some(), 23)
            | bit(s.intro_script.is_some(), 24)
            | bit(s.effects.is_some(), 25)
            | bit(s.doors.is_some(), 26);
        if let Some(u) = &s.unparsed_sections {
            for &b in &u.flag_bits {
                ensure!(SECTION_ORDER.contains(&b), "unparsed section bit {b} is not a section");
                flags |= 1 << b;
            }
        }
        if let Some(rails) = &s.rails {
            ensure!(rails.survival_defenders.is_some() == (flags & 1 << 19 != 0), "rails.survival_defenders must be present exactly when survival_waves is");
        }
        let mut w = Writer::new();
        w.u8(s.has_names as u8).u32(flags);
        if let Some(v) = s.budget_cost {
            w.u16(v);
        }
        if let Some(v) = s.event_id {
            w.i16(v);
        }
        if let Some(v) = s.persist_slot {
            w.u16(v);
        }
        let offset_at = if flags & 1 << 10 != 0 {
            let at = w.pos();
            w.u16(0);
            Some(at)
        } else {
            None
        };
        if let Some(m) = &s.merits {
            write_merits(&mut w, m, ctx, s.has_names)?;
        }
        if let Some(m) = &s.unused_markers {
            w.u8(u8::try_from(m.len())?);
            for [x, y] in m {
                w.i32(x.0).i32(y.0);
            }
        }
        if let Some(o) = &s.objects {
            write_objects(&mut w, o, ctx, s.has_names, s.merits.is_some(), s.legacy_objects)?;
        }
        let unparsed_from = s.unparsed_sections.as_ref().and_then(|u| u.flag_bits.first().copied());
        for &b in &SECTION_ORDER {
            if Some(b) == unparsed_from {
                break;
            }
            if flags & 1 << b == 0 {
                continue;
            }
            match b {
                5 => write_groups(&mut w, s.groups.as_ref().unwrap(), ctx, s.has_names, s.merits.is_some())?,
                6 => write_list(OBJECT_LINK, s.object_links.as_ref().unwrap(), &mut w, ctx)?,
                7 => write_list(CONTENTS, s.contents.as_ref().unwrap(), &mut w, ctx)?,
                8 => write_list(ROPE_ATTACHMENT, s.rope_attachments.as_ref().unwrap(), &mut w, ctx)?,
                9 => write_list(JOINT, s.joints.as_ref().unwrap(), &mut w, ctx)?,
                10 => {
                    let at = offset_at.unwrap();
                    let off = match s.liquids_offset {
                        Some(o) => o,
                        None => u16::try_from(w.pos()).map_err(|_| anyhow::anyhow!("liquids section beyond 64 KiB"))?,
                    };
                    w.patch_u16(at, off);
                    write_list(LIQUID, s.liquids.as_ref().unwrap(), &mut w, ctx)?;
                }
                14 => {
                    w.bytes(&s.sky_color.unwrap().0);
                }
                11 => {
                    let rails = s.rails.as_ref().unwrap();
                    write_list(RAIL_PATH, &rails.paths, &mut w, ctx)?;
                    write_list(SURVIVAL_PATH, &rails.survival_paths, &mut w, ctx)?;
                    if let Some(e) = &rails.survival_defenders {
                        schema::encode(SURVIVAL_DEFENDERS, e, &mut w, ctx)?;
                    }
                }
                12 => {
                    for v in s.camera_bounds.unwrap() {
                        w.i16(v);
                    }
                }
                13 => write_list(DECORATION, s.decorations.as_ref().unwrap(), &mut w, ctx)?,
                16 => schema::encode(HINTS, s.hints.as_ref().unwrap(), &mut w, ctx)?,
                17 => write_list(AREA, s.no_drop_zones.as_ref().unwrap(), &mut w, ctx)?,
                19 => write_list(SURVIVAL_WAVE, s.survival_waves.as_ref().unwrap(), &mut w, ctx)?,
                20 => write_list(AREA, s.merit_zones.as_ref().unwrap(), &mut w, ctx)?,
                21 => write_list(LIGHT, s.lights.as_ref().unwrap(), &mut w, ctx)?,
                23 => write_list(ATMOSPHERE, s.atmospheres.as_ref().unwrap(), &mut w, ctx)?,
                24 => {
                    w.u32(s.intro_script.as_ref().unwrap().to_index(ctx)?);
                }
                25 => write_list(EFFECT, s.effects.as_ref().unwrap(), &mut w, ctx)?,
                26 => write_list(DOOR, s.doors.as_ref().unwrap(), &mut w, ctx)?,
                _ => unreachable!(),
            }
        }
        if let Some(u) = &s.unparsed_sections {
            if u.flag_bits.contains(&10) {
                let off = s.liquids_offset.ok_or_else(|| anyhow::anyhow!("liquids_offset required when the liquids section is unparsed"))?;
                w.patch_u16(offset_at.unwrap(), off);
            }
            w.bytes(&u.data.0);
        }
        Ok(w.into_inner())
    }
}
