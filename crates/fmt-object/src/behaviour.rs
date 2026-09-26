//! Behaviours (event triggers), actions and adjective modifiers: the scripting layer of
//! scribble objects — and of level scenes and event scripts, which use the very same engine
//! classes. This module is the one schema for all of them: `.so` behaviours, `.sa` modifiers,
//! `.sod` scene triggers (`fmt_map::script`) and `@` actions of event scripts
//! (`fmt_ui::event`) all decode through [`BEHAVIOURS`] and [`ACTIONS`], so the same trigger or
//! action has the same name and field names everywhere.
//!
//! An object body carries a list of **behaviours** (`FUN_006d36e0` builds the trigger class
//! for one of 65 type ids, 0x00-0x42 without 0x0e/0x0f, from a type byte; virtual `parse` =
//! vtable slot 8, check = slot 7, text table = slot 12 / `+0x30`), each followed by the **actions**
//! it runs (`FUN_0064e1f0` builds one of 70 action classes, virtual `parse` = vtable slot 9;
//! ids 0x22 and 0x29 do not exist). Adjectives (`.sa`) carry **modifiers** (`FUN_0068d970`,
//! virtual `parse` = slot 5), one of which adds a behaviour.
//!
//! ```text
//! behaviour entry:  u8 type_once   bits 0-6 type, bit7 set = fires only once
//!                   <type-specific fields>
//!                   u8 n; n x action
//! action:           u8 type; <type-specific fields>
//! modifier:         u8 type_flag   bits 0-6 type, bit7 = inheritable
//!                   <type-specific fields>
//! ```
//!
//! JSON: `{"type": "on_used", ["once": true], ...fields, "actions": [...]}` for behaviours,
//! `{"type": "spawn_object", ...fields}` for actions, `{"type": ..., ["inheritable": true],
//! ...fields}` for modifiers. Fields equal to their default (`null`, `false`, `[]`, `{}`, skipped
//! zero bytes) are left out.
//!
//! The per-type field lists are transcribed from each class's parse method (addresses in the
//! table) and named after what the class's other methods do with them. Type names come from the
//! game's own object-editor text where it has one: every trigger class returns the resource
//! index of its `static_trigger_*` text table from vtable slot `+0x30` (e.g.
//! `static_trigger_collide` "COLLIDES ... MINIMUM SPEED") and every action class returns its
//! `static_action_*` table from slot `+0x34`; the editor's pick lists (`static_triggerlist` via
//! the id table at `0x0083a9b0`, `static_actionlist` via `0x00840f28`) confirm the pairing.
//! Classes whose text is `static_*_default` ("UNKNOWN") are named after what they do, or from
//! the engine's debug strings (`add_portal_choice`, `wait_for_variable`).
//!
//! Common value encodings (see [`crate::record`]):
//! * `target` of an action (`FUN_0064f630`): `"self"`, `"trigger_target"`, `"carrier"` (root
//!   of the owner's attachment chain), `"none"`, or `{"entity": ...}`;
//! * scene entities: `{"object": n}` (instance id n = `.sod` `objects` index - 1), `{"group": n}`, `null`;
//! * adjective lists: `.sa` resources, or `{"adjective": ..., "word": key | "hidden"}` when
//!   shown under another word;
//! * objects and adjectives in filters and relations: taxonomy paths
//!   (`"mammal/large/hooved/cow"`, see [`crate::refs`]); merits and tags by name.

use crate::record::{read_fields, write_fields, Cond, Field, Field::*, Nested, Part};
use scribble_core::{anyhow, bail, Context, Reader, Result, Writer};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------------------------
// Shared field groups

/// Base trigger fields (`FUN_006d4640`): which objects can set the trigger off — an object
/// filter, or a specific scene entity (`subject`, which replaces the filter's clauses;
/// `FUN_00677df0`). `player_created_only` (+0x30, `FUN_006d4600`: `if (+0x30 == 0 ||
/// obj+0x24f != 0)` then the filter; used by `on_object_in_sight` and `object_count_in_area`)
/// limits it to objects the player wrote. Bit 1 is stored at +0x31 but only the writer
/// `FUN_006d46c0` reads it back: no vtable method of the 20 classes that parse through
/// `FUN_006d4640` touches +0x31 (hence `unused_bit1`; set on 69 shipped triggers).
const BASE: &[Field] = &[Flags("base_flags", &["player_created_only", "unused_bit1"]), Filter("filter"), Entity("subject")];

/// Action target (`FUN_0064f630`): who the action applies to; value 4 carries an instance id.
const TARGET: &[Field] = &[
    // Resolved by FUN_0064f570 (switch on +0x24): 0 the owner (text static_trigact_action_target
    // "SELF"), 1 the object the firing trigger recorded ("TRIGGER TARGET", owner+0x9f0 table
    // indexed by the trigger channel), 2 the root of the owner's attachment chain (FUN_006a17c0:
    // whoever holds, wears or rides it), 3 nothing (default case: no object; the editor's
    // "TERRAIN" / world coordinates), 4 the u32 entity read into +0x28 (FUN_006a1770).
    Target("target"),
];

// ---------------------------------------------------------------------------------------------
// Behaviours

/// Name and schema of each behaviour (trigger) type.
pub struct BehaviourDef {
    pub id: u8,
    pub name: &'static str,
    pub fields: &'static [Field],
    /// Parse method address in `Scribble.exe`.
    pub parser: u32,
}

macro_rules! defs {
    ($ty:ident: $($id:literal $name:literal $parser:literal => [$($f:expr),* $(,)?]),* $(,)?) => {
        &[$($ty { id: $id, name: $name, parser: $parser, fields: &[$($f),*] }),*]
    };
}

/// Every behaviour type the factory (`FUN_006d36e0`) knows.
///
/// Each entry's comment gives the class's text table (vtable slot 12 = `+0x30`, a resource
/// index of `data\events\[region]\static_trigger_*`; its first string is the editor title), the
/// parse method (slot 8, the address in the table) and the check method (slot 7) that uses the
/// parsed fields. Classes shared by two ids pick their text from a constructor argument.
pub static BEHAVIOURS: &[BehaviourDef] = defs![BehaviourDef:
    // static_trigger_collide "COLLIDES" (param "MINIMUM SPEED"); check FUN_005a77a0. min_speed is
    // stored squared (<<12) and compared with the speed (FUN_004f09d0 * 0xe10). collide_flags
    // (FUN_005a75a0): bit0 +0x5c use the other object's speed; bit1 +0x5d count collisions even
    // while the owner is still spawning (else it needs obj+0x8b0 == -1, the spawn countdown
    // done, and obj+0x3f6 == 0); bits 2-4 pick the check at +0x58: objects (FUN_005a7360, the
    // collider matches the filter), terrain (FUN_005a74a0, obj+0x82e set by the terrain contact
    // FUN_005440f0), liquid (FUN_005a7520, obj+0x82c set when entering a non-lava liquid zone,
    // FUN_006a4540); objects+terrain = either, none = any of the three; bits 5-7 are not read.
    // thrown_only (+0x5e) needs obj+0x272 bit5 (thrown or fired).
    0x00 "on_collide" 0x5a75a0 => [
        Inline(BASE), U16("min_speed"), Flags("collide_flags", &["use_other_speed", "ignore_spawn_state", "objects", "terrain", "liquid"]),
        Bool("thrown_only"),
    ],
    // static_trigger_create "CREATE" (editor list "IS CREATED"); check FUN_005a8160: fires once
    // when the owner is in the world. Same class as 0x38 (type id kept at +0x30).
    0x01 "on_created" 0x6d1220 => [],
    // static_trigger_destroy "DESTROYED"; check FUN_005a82b0 (event bit 0x20, set by the death
    // state FUN_00662170 / FUN_006c5100).
    0x02 "on_destroyed" 0x6d1300 => [],
    // static_trigger_consumed "CONSUMED" ("GETS EATEN BY AN OBJECT"); check FUN_005a7f50, the
    // consumer is obj+0x9fc.
    0x03 "on_consumed" 0x6d4640 => [Inline(BASE)],
    // static_trigger_activated "ACTIVATED" / static_trigger_deactivated (one class, FUN_004ad9e0,
    // text chosen by +0x30); check FUN_005a5b00 watches obj+0x270 bit3 (active) and plays sound
    // slot 14/15 (activate/deactivate).
    0x04 "on_activated" 0x4ada50 => [],
    0x05 "on_deactivated" 0x4ada50 => [],
    // static_trigger_used "USED" ("IS USED WHILE NOT EQUIPPED"); check FUN_005adab0 (event bit
    // 2, user obj+0xa08 must match the filter). `icon`: the interaction the player picks, not
    // read by the check but by the use menu (editor FUN_005add50; ids from the table built by
    // FUN_00595830, text static_choice[DAT_00896fc0[id]]).
    0x06 "on_used" 0x5adb60 => [Inline(BASE), Enum("icon", ICONS)],
    // static_trigger_usedequipped "USED EQUIPPED" ("IS USED WHILE EQUIPPED"); check FUN_005ade50.
    0x07 "on_used_equipped" 0x5adf30 => [Inline(BASE), Enum("icon", ICONS)],
    // static_trigger_vehicleaction "VEHICLE ACTION" ("IS USED WHILE RIDDEN"); check FUN_005ae0f0.
    0x08 "on_used_ridden" 0x5ae1d0 => [Inline(BASE), Enum("icon", ICONS)],
    // static_trigger_objectswap "OBJECT SWAPPED"; check FUN_005aa7e0.
    0x09 "on_swapped" 0x6d1730 => [],
    // static_trigger_modifycharge "MODIFIED CHARGE" (param "MODIFICATION", values
    // static_chargemodification GAIN/LOOSE; "GAINS/LOSES ELECTRICITY"); check FUN_005a8fa0 watches
    // obj+0x1d8 bit1 (powered): change 0 fires when it turns on, 1 when it turns off.
    0x0a "on_charge_changed" 0x5a9070 => [Enum("change", &[(0, "gain"), (1, "lose")])],
    // One class (FUN_006d1500, text by +0x30): 0 static_trigger_tempchangesolid "BECOME COLD",
    // 1 static_trigger_tempchangefluid "BECOME WARM", 2 static_trigger_tempchangegas "BECOME HOT";
    // check FUN_005aade0.
    0x0b "on_become_cold" 0x6d1550 => [],
    0x0c "on_become_warm" 0x6d1550 => [],
    0x0d "on_become_hot" 0x6d1550 => [],
    // static_trigger_equipped / static_trigger_unequipped ("IS (UN)EQUIPPED"); check FUN_005a8bc0.
    0x10 "on_equipped" 0x6d4640 => [Inline(BASE)],
    0x11 "on_unequipped" 0x6d4640 => [Inline(BASE)],
    // static_trigger_attached / static_trigger_detached ("IS ATTACHED/DETACHED"); check
    // FUN_005a6420.
    0x12 "on_attached" 0x6d4640 => [Inline(BASE)],
    0x13 "on_detached" 0x6d4640 => [Inline(BASE)],
    // static_trigger_objectinsight "OBJECT IN SIGHT"; check FUN_005aa3a0 scans 6 objects per
    // frame within the owner's sight range (+0x3d1) and line of sight (FUN_00653720).
    0x14 "on_object_in_sight" 0x6d4640 => [Inline(BASE)],
    // One class (FUN_006d1130) for 0x15 and 0x42, text static_trigger_default ("UNKNOWN",
    // "TRIGGER FIRING CONDITIONS ARE UNKNOWN"): the parse FUN_005ad350 switches on the type id
    // kept at +0x30 to skip legacy layouts, but no case matches 0x15/0x42, so nothing is read;
    // the check FUN_005ad320 returns false, the writer (slot 10) writes nothing and the type-id
    // getter (slot 11, FUN_006d1190) returns 0x42, so 0x15 is re-saved as 0x42.
    0x15 "never_fires_15" 0x5ad350 => [],
    // static_trigger_catchfire "CAUGHT FIRE" / static_trigger_extinguishfire (one class, text by
    // +0x30); check FUN_005a6670 compares the burning state FUN_0069a460 with its last value.
    0x16 "on_catch_fire" 0x6d1bd0 => [],
    // One class (FUN_006d15b0, kind at +0x30): 0 static_trigger_condition_and "CONDITION AND",
    // 1 static_trigger_condition_or "CONDITION OR", 2 static_trigger_condition_oiand "CONDITION
    // OIAND ... (ORDER INDEPENDENT)" (text FUN_005a7dc0). Parse FUN_005a7e20: two nested
    // triggers with their actions ("TRIGGER 1", "TRIGGER 2"); check FUN_005a7b20.
    0x17 "condition_and" 0x5a7e20 => [Behaviour("first"), Behaviour("second")],
    0x18 "condition_or" 0x5a7e20 => [Behaviour("first"), Behaviour("second")],
    // static_trigger_checkvar "CHECK VARIABLE" (params REGISTER, VALUE, COMPARE); parse
    // FUN_005a6f00, check FUN_005a6d60 -> FUN_0045b800: `variable` compared with `value`
    // (`@name` = another variable; debug text FUN_005a7010 prints "variable OP value"); with
    // `on_change` (+0x78) only when the variable changed.
    0x19 "check_variable" 0x5a6f00 => [Bool("on_change"), Enum("compare", COMPARE), CStr("value"), CStr("variable")],
    // static_trigger_update "UPDATE" ("TRIGGER FIRES WHEN THIS OBJECT IS UPDATED"); check
    // FUN_005ad7d0 fires every frame while the owner is active (obj+0x270 bit3) and spawned.
    0x1a "while_active" 0x5e3e40 => [],
    // static_trigger_contained / static_trigger_uncontained ("ENTERS/EXITS A CONTAINER"); check
    // FUN_005a8070.
    0x1b "on_contained" 0x6d4640 => [Inline(BASE)],
    0x1c "on_uncontained" 0x6d4640 => [Inline(BASE)],
    // static_trigger_objectadded "OBJECT ADDED ... TO STAGE"; parse FUN_005a9400, check
    // FUN_005a94b0: unique_object_types (+0x54) counts each object type (+0x162) once;
    // `sources` (+0x58, 0 read as 3) is tested with &1 for player-written objects (obj+0x24f)
    // and &2 for the others, so 1 = player-created, 2 = other, 3 = both (the third bit of the
    // field is never tested); unique_adjectives (+0x55) counts each adjective once. Bits 5-7
    // are not read by the parser.
    0x1d "on_object_added" 0x5a9400 => [
        Inline(BASE),
        Split(&[Part::Bool("unique_object_types", 0x01), Part::Num("sources", 0x0e), Part::Bool("unique_adjectives", 0x10), Part::Rest("unused_bits", 0xe0)]),
    ],
    // static_trigger_aistate "MOOD CHANGED" (param "MOOD", values static_aimood); parse
    // FUN_005a6320; its check FUN_005a6310 returns false, so it never fires.
    0x1e "on_mood_changed" 0x5a6320 => [Enum("mood", MOODS)],
    // static_trigger_aiaction "AI ACTION" ("REACTS TO AN OBJECT"); check FUN_005a5c00: the event
    // mask for the action comes from FUN_00655830 (a relation kind), the other object is
    // obj+0xa6c.
    0x1f "on_ai_action" 0x5a5cf0 => [Inline(BASE), Enum("action", RELATION_KINDS)],
    // static_trigger_objectcountinarea "OBJECT COUNT IN AREA" (params TYPE OF OBJECT, ZONE OR
    // LOCATION, COMPARE, COUNT); parse FUN_005a9950, check FUN_005a9c90: `count` objects
    // (compare 2 = more, 3 = fewer) in a rectangle (whole units: each i32 is read as 24 bits
    // <<12, top byte skipped; y before x on disk), optionally relative to `anchor` (+0x6c).
    // area_flags: bit0 +0x6a count each object type (+0x162) once; bit1 anchor present; bit2
    // +0x6b mirror the rectangle with the anchor; bit3 stored inverted at +0x69 (clear = skip
    // objects held by the player, FUN_005a98a0); bit4 +0x78 skip attached objects
    // (FUN_005a9910); bits 5-7 not read. `extra_objects` (u8 list at +0x74) is compared with
    // obj+0xc when a `subject` is set but not found.
    0x20 "object_count_in_area" 0x5a9950 => [
        Inline(BASE), ListOf("extra_objects", &U8("")), Enum("compare", COMPARE), U8("count"),
        Flags("area_flags", &["unique_types", "has_anchor", "rotate_with_anchor", "include_player_held", "exclude_attached"]),
        I32("min_y"), I32("min_x"), I32("max_y"), I32("max_x"),
        If(Cond::Bit("area_flags", 2), &[Entity("anchor")]),
    ],
    // static_trigger_distance "DISTANCE" (params OBJECT, DISTANCE, COMPARE; "MOVES NEAR AN
    // OBJECT"); parse FUN_005a8450 (distance <<12), check FUN_005a8590 -> FUN_005a83b0: compare
    // 2 fires when the bounding boxes are farther apart than `distance` (text "FARTHER"), 3 when
    // within it; 0/1 never fire. Bit 7 stored inverted at +0x5c (clear = skip objects held by
    // the player, FUN_005a98a0).
    0x21 "distance" 0x5a8450 => [Inline(BASE), Split(&[Part::Enum("compare", 0x7f, COMPARE), Part::Bool("include_player_held", 0x80)]), U16("distance")],
    // static_trigger_velocity "VELOCITY" (params VELOCITY, COMPARE; "MOVES AT A SPEED"); parse
    // FUN_005aea80 (speed squared <<12 / 3600), check FUN_005ae920: compare 3 = slower than
    // `speed`, any other value = faster.
    0x22 "velocity" 0x5aea80 => [Enum("compare", COMPARE), U16("speed")],
    // static_trigger_split "SPLIT"; check FUN_005aab50.
    0x23 "on_split" 0x6d1800 => [],
    // static_trigger_pressed "PRESSED" / static_trigger_unpressed "UNPRESSED"; checks
    // FUN_005aa990 / FUN_005ad630.
    0x24 "on_pressed" 0x6d18d0 => [],
    0x25 "on_unpressed" 0x6d19a0 => [],
    // static_trigger_aiequip "AI EQUIP" ("EQUIPS AN OBJECT"; one class with 0x30, text by +0x54);
    // check FUN_005a61f0.
    0x26 "on_ai_equip" 0x6d4640 => [Inline(BASE)],
    // static_trigger_aiconsume "AI CONSUME"; check FUN_005a6140.
    0x27 "on_ai_consume" 0x6d4640 => [Inline(BASE)],
    // static_trigger_emptied "EMPTIED" / static_trigger_filled "FILLED" (one class, text by
    // +0x54); parse FUN_005a8950, check FUN_005a8b00 -> FUN_005a89c0: fill_flags bit0 (+0x55)
    // counts each object type (+0x160) once; bits 1-7 not read.
    0x28 "on_emptied" 0x5a8950 => [Inline(BASE), Flags("fill_flags", &["unique_object_types"])],
    0x29 "on_filled" 0x5a8950 => [Inline(BASE), Flags("fill_flags", &["unique_object_types"])],
    // static_trigger_modifyintegrity "MODIFIED INTEGRITY" ("LOSES OR GAINS HEALTH"); check
    // FUN_005a9120.
    0x2a "on_integrity_modified" 0x6d4640 => [Inline(BASE)],
    // static_trigger_hearsound "HEARD SOUND" (param "SOUND"); parse FUN_005a8d30 (+0x30), check
    // FUN_005a8cb0 fires on event bit 0x200000 only when `sound` is 0.
    0x2b "on_hear_sound" 0x5a8d30 => [U8("sound")],
    // static_trigger_mounted "MOUNTED" / static_trigger_unmounted (one class, text by +0x54);
    // parse FUN_005a9360, check FUN_005a91e0: mount_flags bit0 (+0x55) ignores the second
    // (passenger) event bit and fires on the driver-seat event only; bit1 (+0x56) needs the owner
    // active (obj+0x270 bit3); bits 2-7 not read.
    0x2c "on_mounted" 0x5a9360 => [Inline(BASE), Flags("mount_flags", &["driver_seat_only", "only_when_active"])],
    0x2d "on_unmounted" 0x5a9360 => [Inline(BASE), Flags("mount_flags", &["driver_seat_only", "only_when_active"])],
    // static_trigger_group "TRIGGER GROUP"; parse FUN_005ad2e0 = check_variable's parse + a
    // nested trigger list (FUN_005ad150); check FUN_005ad050.
    0x2e "trigger_group" 0x5ad2e0 => [Bool("on_change"), Enum("compare", COMPARE), CStr("value"), CStr("variable"), Behaviours("behaviours")],
    // Kind 2 of the condition class above: static_trigger_condition_oiand "CONDITION OIAND".
    0x2f "condition_and_any_order" 0x5a7e20 => [Behaviour("first"), Behaviour("second")],
    // static_trigger_aiunequip "AI UNEQUIP" ("UNEQUIPS AN OBJECT").
    0x30 "on_ai_unequip" 0x6d4640 => [Inline(BASE)],
    // static_trigger_applyadj "APPLY ADJECTIVE" / static_trigger_kickadj "REMOVE ADJECTIVE"
    // (one class, text by +0x54; "GAINS/LOSES AN ADJECTIVE"); check FUN_005a6370.
    0x31 "on_adjective_applied" 0x6d4640 => [Inline(BASE)],
    // static_trigger_extinguishfire "FIRE EXTINGUISHED" ("STOPS BEING ON FIRE"), see 0x16.
    0x32 "on_fire_extinguished" 0x6d1bd0 => [],
    0x33 "on_adjective_removed" 0x6d4640 => [Inline(BASE)],
    // static_trigger_terrainchanged "TERRAIN CHANGED"; parse FUN_005acd00 (bits 1-7 of the flag
    // byte not read), check FUN_005ac4f0: a rectangle of terrain cells; fires when all
    // initially solid cells are gone, or on any change with `fire_on_any_cell` (+0x4c).
    0x34 "terrain_changed" 0x5acd00 => [Flags("terrain_flags", &["fire_on_any_cell"]), U16("x"), U16("y"), U16("width"), U16("height")],
    // static_trigger_zonedepth "ZONE DEPTH"; parse FUN_005aecb0, check FUN_005aed90: the depth
    // (+0x7c - +0x74) of level liquid zone `zone` (DAT_008a8700+0x7b78 list) against `depth`
    // (<<12): 0 ==, 1 !=, 2 depth < value, 3 depth > value (the reverse of `COMPARE`'s order).
    0x35 "zone_depth" 0x5aecb0 => [
        Bool("on_change"), Enum("compare", &[(0, "equal"), (1, "not_equal"), (2, "less"), (3, "greater")]), U16("depth"), U8("zone"),
    ],
    // static_trigger_submerged "SUBMERGED" (params ZONE TYPE, ZONE NAME); parse FUN_005aabb0,
    // check FUN_005aac10: overlap with level liquid zone `zone` (0xFF = any zone of `zone_type`).
    0x36 "submerged" 0x5aabb0 => [Enum("zone_type", ZONE_TYPES), U8("zone")],
    // static_trigger_checksaveflag "CHECK SAVE FLAG"; parse FUN_005a6720, check FUN_005a66c0:
    // bit `flag_bit` of the save's flag bits (FUN_00646ff0()+0x3b) == `expected`, evaluated only
    // when `source` (+0x34) is 0 (other values never fire). Unused by shipped data.
    0x37 "check_save_flag" 0x5a6720 => [U8("source"), Bool("expected"), U16("flag_bit")],
    // static_trigger_create, same class as 0x01; fired synchronously after adjectives are
    // (re)applied and on restore (FUN_0069e000).
    0x38 "on_created_immediate" 0x6d1220 => [],
    // static_trigger_aienabled "AI ENABLED" ("BECOMES ANIMATE") / static_trigger_aidisabled
    // ("BECOMES INANIMATE") (one class FUN_005a5fb0, text by +0x30); check FUN_005a60c0 watches
    // obj+0x3ec / +0x3fc (brain).
    0x39 "on_ai_enabled" 0x5a6050 => [],
    0x3a "on_ai_disabled" 0x5a6050 => [],
    // static_trigger_pathused "PATH USED" (param TARGET PATH); parse FUN_005aa8d0, check
    // FUN_005aa840: `path` compared with obj+0xadc, 0xFF (or absent) = any.
    0x3b "path_used" 0x5aa8d0 => [Flags("path_flags", &["has_path"]), If(Cond::Bit("path_flags", 1), &[U8("path")])],
    // static_trigger_default "UNKNOWN" (FUN_006d20b0): parse FUN_006d2100 reads nothing and the
    // check FUN_005aced0 returns false.
    0x3c "never_fires_3c" 0x6d2100 => [],
    // static_trigger_merit "MERIT" (params MERIT ID, MERIT STATUS); parse FUN_005a8ed0, check
    // FUN_005a8df0 computes the merit's status: 4 if its bit in the save's merit bits
    // (DAT_008b3404+0x50, set when awarded by FUN_004f78c0 via FUN_006f9580) is set, else 2 if
    // merit+0x20 != 0 (set by enable_merit "disable", FUN_005422f0 -> FUN_004f77f0), else 1;
    // fires when it is in `status` (with `on_change` +0x48 only when it changed), then runs the
    // nested trigger list (FUN_005ad150).
    0x3d "merit" 0x5a8ed0 => [
        Flags("merit_flags", &["on_change"]), Merit("merit"), Flags("status", &["not_earned", "disabled", "earned"]), Behaviours("behaviours"),
    ],
    // static_trigger_default (no text of its own); parse FUN_005a8250, check FUN_005a81a0: event
    // bit 0x100 set by AI state handler 46 (FUN_00663760) when the AI uses an ability; the other
    // object is obj+0xae8. `icon` is not read by the check.
    0x3e "on_ai_ability" 0x5a8250 => [Inline(BASE), Enum("icon", ICONS)],
    // static_trigger_update text, editor list "WAITS A SET TIME"; parse FUN_005ad850, check
    // FUN_005ad820 fires every `frames` frames.
    0x3f "interval" 0x5ad850 => [U32("frames")],
    // static_trigger_submerged, editor list "GOES IN WATER"; same parse as 0x36, check
    // FUN_005aad60 polls FUN_005aac10 every 120 frames (ctor FUN_006d1de0 sets 0x78).
    0x40 "on_submerged" 0x5aabb0 => [Enum("zone_type", ZONE_TYPES), U8("zone")],
    // static_trigger_objectinsight, editor list "SEES AN OBJECT"; check FUN_005aa780 polls every
    // 60 frames.
    0x41 "on_sees_object" 0x6d4640 => [Inline(BASE)],
    // See 0x15.
    0x42 "never_fires_42" 0x5ad350 => [],
];

// ---------------------------------------------------------------------------------------------
// Actions

/// Name and schema of each action type.
pub struct ActionDef {
    pub id: u8,
    pub name: &'static str,
    pub fields: &'static [Field],
    /// Parse method address in `Scribble.exe`.
    pub parser: u32,
}

/// Every action type the factory (`FUN_0064e1f0`) knows.
pub static ACTIONS: &[ActionDef] = defs![ActionDef:
    // Text static_action_spawnobject. FUN_0054faa0; run by FUN_0054ff00: spawns `object`
    // (0x1E01 = the spawner's own type) with `adjectives` from hotspot `launch_point` and fires
    // it; at most `max_alive` live copies (`abort_when_full` skips the remaining actions when the
    // limit is reached). Flag bit 2 is neither read (FUN_0054faa0 tests only 1, 2, 8) nor
    // written (writer FUN_0054fbd0 emits bits 0, 1, 3).
    0x00 "spawn_object" 0x54faa0 => [
        Flags("flags", &["simple", "has_name", "unused_bit2", "has_variant"]), Res16("object"),
        If(Cond::NoBit("flags", 1), &[
            If(Cond::Bit("flags", 2), &[U16("name_key")]),
            Bool("inherit_adjectives"), Adjectives("adjectives"),
            If(Cond::Bit("flags", 8), &[U8("variant")]),
        ]),
        U8("launch_point"), Inline(SPAWN_LIMIT),
    ],
    // FUN_00542810: calls the indexed node's visibility setter (vtable+0x58).
    // Text static_action_enableshape; parse FUN_00542880, run FUN_00542810, start FUN_005427e0.
    0x01 "enable_shape" 0x542880 => [U8("shape"), Bool("enabled")],
    // Effect 0x1d for `poof`; `silent` sets target+0x275 bit1.
    // Text static_action_destroy; parse FUN_00540370, run FUN_00540c00, start FUN_00540b60.
    0x02 "destroy" 0x540370 => [Inline(TARGET), Flags("flags", &["poof", "destroy_connected", "immediate", "silent"])],
    // Text static_action_playsfx ("PLAY SFX": SFX, SOUND ATTRIBUTE). FUN_0054c0c0 stores the
    // sound at +0x24, the attribute at +0x28 and the list at +0x2c/+0x30; the run method
    // FUN_0054c040 plays only +0x24 (FUN_0069d670 / FUN_0051d120) and the other methods only
    // free / re-save the rest (writer FUN_0054c2c0), so the attribute and the list are never used.
    // Attribute names: static_soundattribute (never loaded by code). The list is empty in
    // every shipped file.
    0x03 "play_sound" 0x54c0c0 => [
        Res32("sound"), Enum("sound_attribute", &[(0, "none"), (1, "alarming"), (2, "entertaining"), (3, "scary"), (4, "soothing")]),
        List("unused_objects", &[U8("unused_flag"), ObjectPath("object")]),
    ],
    // One class; sets, clears or toggles target+0x270 bit3.
    // Text static_action_toggle; parse FUN_0064f630, run FUN_0053bd30, start FUN_0053bd00.
    0x04 "activate" 0x64f630 => [Inline(TARGET)],
    // Text static_action_toggle; parse FUN_0064f630, run FUN_0053bd30, start FUN_0053bd00.
    0x05 "deactivate" 0x64f630 => [Inline(TARGET)],
    // Text static_action_toggle; parse FUN_0064f630, run FUN_0053bd30, start FUN_0053bd00.
    0x06 "toggle_activation" 0x64f630 => [Inline(TARGET)],
    // FUN_0053e160: force (y down), spin in whole units (<<12); `relative` rotates the force
    // by the owner's angle.
    // Text static_action_applyforce; parse FUN_0053e160, run FUN_0053e440, start FUN_0053e130.
    0x07 "apply_force" 0x53e160 => [Inline(TARGET), Fx12("force_x"), Fx12("force_y"), I32("spin"), Bool("relative")],
    // FUN_00549880: only sleepy and sick have an effect.
    // Text static_action_modifyaistate; parse FUN_00549950, run FUN_00549880, start FUN_00549850.
    0x08 "change_mood" 0x549950 => [Inline(TARGET), Enum("mood", MOODS)],
    // Text static_action_modifyarlist ("MODIFY ARLIST": the attraction-repel list). Run
    // FUN_005499f0 adds relations (and exceptions) to the target (FUN_0043ecb0). The byte
    // between the lists is skipped by FUN_00549a70 and always written as 13 by FUN_00549c80
    // (like the `attitudes` modifier's separator).
    0x09 "modify_attitudes" 0x549a70 => [Inline(TARGET), Attitudes("attitudes"), Skip8("separator", 13), Attitudes("exceptions")],
    // FUN_0054b4f0: replaces the target with `object` (0x1E01 = its own type) carrying the
    // adjectives; `group` = scene object group of the new object.
    // Text static_action_objectswap; parse FUN_0054b100, run FUN_0054b4f0, start FUN_0054b0d0.
    0x0a "swap_object" 0x54b100 => [
        Inline(TARGET), Res16("object"),
        Flags("flags", &["revert", "inherit_adjectives", "not_counted_as_destroyed", "has_group", "has_name", "play_sound"]),
        If(Cond::Bit("flags", 0x10), &[U16("name_key")]), Adjectives("adjectives"), If(Cond::Bit("flags", 8), &[U32("group")]),
    ],
    // FUN_0054ad50/FUN_0054aa80: move the target to (x, y) (whole units, <<12), over `frames`
    // frames when >= 2.
    // Text static_action_moveto; parse FUN_0054a7e0, run FUN_0054ad50, start FUN_0054aa80.
    0x0b "move_to" 0x54a7e0 => [Inline(TARGET), I32("x"), I32("y"), U16("frames")],
    // FUN_0054a170: move the target by (x, y).
    // Text static_action_moveby; parse FUN_0054a3a0, start FUN_0054a170.
    0x0c "move_by" 0x54a3a0 => [Inline(TARGET), I32("x"), I32("y"), U16("frames")],
    // FUN_0053e890: attaches the second object (a new `object` with `create`, else an entity)
    // to the first, after `delay` frames.
    // Text static_action_attachto; parse FUN_0053ead0, run FUN_0053e890, start FUN_0053edc0.
    0x0d "attach_to" 0x53ead0 => [
        Entity("first_entity"), Flags("flags", &["create", "has_first_kind", "has_second_kind", "has_delay"]),
        If(Cond::Bit("flags", 2), &[Enum("first_kind", TARGET_KINDS)]), If(Cond::Bit("flags", 4), &[Enum("second_kind", TARGET_KINDS)]),
        IfElse(Cond::Bit("flags", 1), &[Res32("object")], &[Entity("second_entity")]),
        If(Cond::Bit("flags", 1), &[
            Flags("create_flags", &["inherit_adjectives", "has_name"]), If(Cond::Bit("create_flags", 2), &[U16("name_key")]), Adjectives("adjectives"),
        ]),
        Enum("attach_type", &[(0, "attach"), (1, "mount"), (2, "equip"), (3, "joint")]), If(Cond::Bit("flags", 8), &[U16("delay")]),
    ],
    // FUN_0066bd40: breaks all of the owner's attachments.
    // Text static_action_default ("UNKNOWN"; named from its methods); parse FUN_0064d750, run FUN_00540c80, start FUN_00540c50.
    0x0e "detach" 0x64d750 => [],
    // FUN_0053fbf0: `amount` damage (negative heals; multiplied by the attacker's +0x276
    // count); target `none` hits the terrain.
    // Text static_action_dealdamage; parse FUN_0053f740, run FUN_0053fbf0, start FUN_0053f8a0.
    0x0f "deal_damage" 0x53f740 => [Inline(TARGET), Flags("flags", &["knockback"]), I8("amount")],
    // Text static_action_split; parse FUN_0064f630, start FUN_00551940.
    0x10 "split" 0x64f630 => [Inline(TARGET)],
    // FUN_00558b80: speed in units per second (x4096/60); animation slot (throw, kick).
    // Text static_action_throwattarget; parse FUN_00558b80, run FUN_00559460, start FUN_00558740.
    0x11 "throw_at_target" 0x558b80 => [Inline(TARGET), U16("speed"), AnimSlot("animation")],
    // FUN_005449f0; run by FUN_00544460/FUN_00545270. `impact_damage`/`explosion` build the
    // projectile's on-impact actions (FUN_00544ef0); `preload` is regenerated by the writer.
    // Text static_action_fireprojectile; parse FUN_005449f0, start FUN_00544460.
    0x12 "fire_projectile" 0x5449f0 => [
        Flags("flags", &["simple", "has_name_key", "has_preload", "has_impact", "has_name_word", "has_adjective_words", "from_self"]),
        If(Cond::Bit("flags", 0x10), &[U16("name_word")]),
        If(Cond::Bit("flags", 8), &[I32("impact_damage"), Enum("explosion", EXPLOSIONS)]),
        Res32("object"),
        If(Cond::NoBit("flags", 1), &[
            If(Cond::Bit("flags", 2), &[U16("name_key")]), Adjectives("adjectives"),
            If(Cond::Bit("flags", 0x20), &[ListLike("adjective_words", "adjectives", &U16(""))]),
            If(Cond::Bit("flags", 4), &[List("preload", &[Split(&[Part::Num("load_flags", 0x1f), Part::Enum("kind", 0xe0, DEPENDENCY_KINDS)]), Res32("resource")])]),
        ]),
        U8("launch_point"), Flags("projectile_flags", &["ignore_gravity", "launch_point_is_id", "has_anim_speed"]), U16("speed"),
        If(Cond::Bit("projectile_flags", 4), &[Fx12("shoot_anim_speed")]),
    ],
    // FUN_0054e150: target temperature (+0x260) = value << 12.
    // Text static_action_settemperature; parse FUN_0054e190, run FUN_0054e150, start FUN_0054e120.
    // FUN_0054e190 reads the temperature and skips one byte (`*param_3 += 2`); the writer FUN_0054e1c0 writes 0 there.
    0x13 "set_temperature" 0x54e190 => [Inline(TARGET), U8("temperature"), Unused8("unused")],
    // FUN_0053d020; run by FUN_0053cdb0: plays animation `slot` on the target, `repeat_count`
    // + 1 times with `pause_frames` between; `use_ai` plays it through the AI.
    // Text static_action_animation; parse FUN_0053d020, run FUN_0053cdb0, start FUN_0053cd80.
    0x14 "play_animation" 0x53d020 => [
        // `unused_fx` (flag bit 2) is stored at +0x34 by FUN_0053d020 and only re-saved by FUN_0053d160;
        // the run method FUN_0053cdb0 uses +0x2c slot, +0x30 speed, +0x38 use_ai, +0x39 loop, +0x3a/+0x3c counts.
        Inline(TARGET), Flags("flags", &["loop_forever", "use_ai", "has_unused_fx", "has_speed", "has_repeat_count", "has_pause_frames"]), AnimSlot("slot"),
        If(Cond::Bit("flags", 4), &[Fx12("unused_fx")]), If(Cond::Bit("flags", 8), &[Fx12("speed")]),
        If(Cond::Bit("flags", 0x10), &[U16("repeat_count")]), If(Cond::Bit("flags", 0x20), &[U16("pause_frames")]),
    ],
    // FUN_0055b980: re-queues itself until `frames` frames have passed.
    // Text static_action_wait; parse FUN_0055ba30, start FUN_0055b980.
    0x15 "wait" 0x55ba30 => [U16("frames")],
    // FUN_00542000: empties (false) or refills the container.
    // Text static_action_emptyfill; parse FUN_00541df0, run FUN_00542000, start FUN_00541e50.
    0x16 "empty_fill" 0x541df0 => [Bool("fill")],
    // FUN_005515a0: spawns one (object, adjective) pair picked at random (0x1E01 = self).
    // Text static_action_spawnrandom; parse FUN_00551350, run FUN_005515a0, start FUN_00551520.
    0x17 "spawn_random" 0x551350 => [
        List("choices", &[Res16("object"), Res16("adjective")]), Bool("inherit_adjectives"), U8("launch_point"), Inline(SPAWN_LIMIT),
    ],
    // FUN_0054dca0: applies flaming.sa, or extinguished.sa when `ignite` is false.
    // Text static_action_setonfire; parse FUN_0054de00, run FUN_0054dca0, start FUN_0054dc70.
    0x18 "set_on_fire" 0x54de00 => [Inline(TARGET), Bool("ignite")],
    // FUN_0054e9a0: `variable` = `value` (`@name` = another variable's value). The engine
    // writes the operator form (see SET_VARIABLE_OP); this plain form is `=`.
    // Text static_action_setvariable; parse FUN_0054e9a0, run FUN_0054e340, start FUN_0054e220.
    0x19 "set_variable" 0x54e9a0 => [CStr("value"), CStr("variable")],
    // FUN_0053c030: variable = variable + amount.
    // Text static_action_add; parse FUN_0053c0e0, run FUN_0053c030, start FUN_0053be30.
    0x1a "add_to_variable" 0x53c0e0 => [I8("amount"), CStr("variable")],
    // Fails the level showing text box `message_index` of event file `message_script` (none =
    // "END LEVEL ACTION WAS TRIGGERED.", static_misc 5).
    // Text static_action_endlevel; parse FUN_005429a0, start FUN_00542ad0.
    0x1b "end_level" 0x5429a0 => [U8("message_index"), Res16Z("message_script")],
    // FUN_00550860; run by FUN_00550d20: spawns `object` at `offset` from the target (world
    // coordinates for target `none`; x mirrored when the target is flipped). Spawn flags 0 and
    // 2 are never read (bit 0 matches the editor's unused BOUNCE TOWARDS MAXWELL label).
    // Text static_action_spawnobjectat; parse FUN_00550860, run FUN_00550d20, start FUN_00550c10.
    0x1c "spawn_object_at" 0x550860 => [
        Inline(TARGET), Flags("flags", &["simple", "has_group", "has_name", "has_variant"]),
        If(Cond::Bit("flags", 2), &[U32("group")]), Res16("object"),
        If(Cond::NoBit("flags", 1), &[
            If(Cond::Bit("flags", 4), &[U16("name_key")]), Bool("inherit_adjectives"), Adjectives("adjectives"), If(Cond::Bit("flags", 8), &[U8("variant")]),
        ]),
        // FUN_00550860 stores spawn flag bits 0/1/2 at +0x4c/+0x4d/+0x50; only +0x4d is read (run FUN_00550d20:
        // FUN_006857f0/FUN_0069ee00, marks the spawn player-created); +0x4c/+0x50 are only re-saved by FUN_00550a80.
        Fx12("offset_x"), Fx12("offset_y"), Flags("spawn_flags", &["bounce_towards_maxwell", "mark_player_created", "unused_bit2"]), Inline(SPAWN_LIMIT),
    ],
    // FUN_00672820: emote icon (datavector\_emotes\*.vec) for `duration` frames.
    // Text static_action_showemote; parse FUN_0054f090, run FUN_0054f050, start FUN_0054f020.
    0x1d "show_emote" 0x54f090 => [
        Inline(TARGET), Flags("flags", &["loop_forever", "has_duration", "has_repeat_count", "has_interval", "no_fade"]), U8("emote"),
        If(Cond::Bit("flags", 2), &[U16("duration")]), If(Cond::Bit("flags", 4), &[U16("repeat_count")]), If(Cond::Bit("flags", 8), &[U16("interval")]),
    ],
    // `countdown` frames (0 = immediate).
    // Text static_action_explode; parse FUN_00542ec0, run FUN_005432d0, start FUN_00543090.
    0x1e "explode" 0x542ec0 => [Enum("size", EXPLOSIONS), U16("countdown")],
    // FUN_005494f0: swaps the target's image to `vector` (`image` for non-vector renderers).
    // Text static_action_imageswap; parse FUN_00549370, run FUN_005494f0, start FUN_005492f0.
    0x1f "image_swap" 0x549370 => [Inline(TARGET), Res16("image"), Res32("vector")],
    // Plays script number `script_index` of event file `script`.
    // Text static_action_cinematic; parse FUN_0053f340, start FUN_0053f3d0.
    0x20 "cinematic" 0x53f340 => [U8("script_index"), Res32("script")],
    // Text static_action_shownotepad; parse FUN_0054f9e0, start FUN_0054f990.
    0x21 "show_notepad" 0x54f9e0 => [Bool("show")],
    // FUN_0054e000: the target is attracted to (reacts to) `stage_object` with `behaviour`.
    // Text static_action_setstageobject; parse FUN_0054e000, run FUN_0054dee0, start FUN_0054deb0.
    0x23 "set_stage_object" 0x54e000 => [
        Inline(TARGET), Enum("behaviour", RELATION_KINDS),
        Split(&[Part::Bool("ignore_line_of_sight", 0x01), Part::Enum("emote_mode", 0xfe, &[(0, "default"), (1, "none"), (2, "explicit")])]),
        Entity("stage_object"), If(Cond::Eq("emote_mode", 2), &[U8("emote")]),
    ],
    // FUN_005421e0: bit0 -> FUN_006a7470 (enable), bit1 plays the smoke puff.
    // Text static_action_enableentity; parse FUN_00542250, run FUN_005421e0, start FUN_005421b0.
    0x24 "enable_entity" 0x542250 => [Inline(TARGET), Flags("flags", &["enabled", "poof"])],
    // FUN_00552dc0: runs one of the chains per call (other `order` values always run the first).
    // Text static_action_switch; parse FUN_00552e80, run FUN_00552dc0, start FUN_00552cb0.
    0x25 "switch" 0x552e80 => [Enum("order", &[(0, "shuffle"), (1, "random"), (2, "sequence")]), ActionChains("chains")],
    // FUN_00542660: enables/disables the indexed effect node (type 9).
    // Text static_action_enablesfanim; parse FUN_005426d0, run FUN_00542660, start FUN_00542630.
    0x26 "enable_sprite_anim" 0x5426d0 => [U8("node"), Bool("enabled")],
    // Slot 12 only resolves the target: no effect in this build.
    // Text static_action_rotateentity; parse FUN_0054d190, run FUN_0064f570, start FUN_0054d150.
    0x27 "rotate_object" 0x54d190 => [Inline(TARGET), U16("rotation_per_second")],
    // FUN_0053d590: applies `.sa` resources (849 = myadjectives.sa: the notebook's adjectives);
    // `word_ids` override the display word of each adjective (+0x38).
    // Text static_action_applyadjective; parse FUN_0053dc80, run FUN_0053d590, start FUN_0053d560.
    0x28 "apply_adjectives" 0x53dc80 => [
        // FUN_0053dc80: +0x3d = bit 1 (hide_names, tested by run FUN_0053d590), +0x3e = bit 0 | bit 1, which only the
        // writer FUN_0053dd50 reads back; bit 2 = word list present; bit 3 is never tested.
        Inline(TARGET), Flags("flags", &["unused_bit0", "hide_names", "has_word_ids", "unused_bit3"]), Adjectives("adjectives"),
        If(Cond::Bit("flags", 4), &[ListLike("word_ids", "adjectives", &U16(""))]),
    ],
    // FUN_0054f4a0: feedback icon; `success` adds 1 to `__ufsprogress` (else -1); the offset is
    // relative to the target unless `screen_space`.
    // Text static_action_showfeedback; parse FUN_0054f380, run FUN_0054f4a0, start FUN_0054f300.
    0x2a "show_feedback" 0x54f380 => [Inline(TARGET), Flags("flags", &["success", "screen_space"]), Fx12("offset_x"), Fx12("offset_y")],
    // FUN_00670b60: locks the container; `keys` open it.
    // Text static_action_lock; parse FUN_00549770, run FUN_00549740, start FUN_00549710.
    // FUN_00549770 copies the RefList (count*15+5 bytes) and skips one byte; the writer FUN_005497e0 writes 0.
    0x2b "lock" 0x549770 => [Inline(TARGET), RefList("keys"), Unused8("unused")],
    // FUN_0054ae30: turns off the AI of the owner and its flagged attachments.
    // Text static_action_default ("UNKNOWN"; named from its methods); parse FUN_0054af50, run FUN_0054ae30, start FUN_0054ae00.
    0x2c "disable_ai" 0x54af50 => [],
    // Text static_action_interruptflag ("SET INTERRUPT FLAG"). Start FUN_0054dac0 sets (or
    // clears) `event` (a trigger type) in the owner's per-channel event mask (+0x860 +
    // 8*channel), re-arming or blocking that trigger. FUN_0054dbc0 keeps `byte & 0x4f` as the
    // channel (bit 6 is part of it) and `byte >> 7` as `set`; bits 4-5 are dropped (the writer
    // FUN_0054dc10 re-emits only channel | set << 7).
    0x2d "set_interrupt_flag" 0x54dbc0 => [
        Split(&[Part::Num("channel", 0x4f), Part::Rest("unused_bits", 0x30), Part::Bool("set", 0x80)]),
        TriggerType("event"),
    ],
    // Text static_action_shock; parse FUN_0064f630, run FUN_0054eed0, start FUN_0054eea0.
    0x2e "shock" 0x64f630 => [Inline(TARGET)],
    // Sets +0xb30, which blocks adjectives written by the player.
    // Text static_action_untouchable; parse FUN_0055b350, run FUN_0055b310, start FUN_0055b2e0.
    0x2f "untouchable" 0x55b350 => [Inline(TARGET), Enum("action", SWITCH)],
    // "AddPortalChoice: Map TLE(%i)" (FUN_0053cc90); FUN_0053cc10 sends choice message 0x16.
    // Its text table is a copy of `static_action_add`. Unused by shipped data.
    // FUN_0053c4e0 builds a 0x90-byte level-transition record (the one doors build in
    // FUN_004b3ca0): the four resources go through FUN_004e1a50, the level-descriptor
    // constructor (`S_FARM` = c_farm.tle, s_farm.sod, c_farm.stp, s_farm.mdb), so they are the
    // level's tile map, scene, setup and merits in that order; flag bit 0 is the descriptor's
    // +0x14 byte (`unlocks_rewatch`, as in `.lvls`) and record+0x4d. The 8 bytes after them are
    // skipped (the door loader skips 8 bytes at the same place) and never written back by
    // FUN_0053c9c0. `script` is record+0x48, where FUN_004b3ca0 puts a door's script (tested
    // by the arrival code FUN_005f14a0); `value_a` is record+0x44 (no reader found). The two
    // bytes go to record+0x40 and +0x3c, which the map-edge exit FUN_00641eb0 fills with the
    // exit direction code; which one is the leaving and which the arriving side is not
    // established. The secondary block is a second level (record+0x50); its trailing 4 bytes
    // are skipped by the parser and written by FUN_0053c9c0 as a copy of `script`.
    0x30 "add_portal_choice" 0x53c4e0 => [
        Flags("flags", &["unlocks_rewatch", "has_value_a", "has_script", "has_secondary", "has_spawn"]), U8("transition_out"), U8("transition_in"),
        Res32("tile_map"), Res32("scene"), Res32("setup"), Res32("merits"), Bytes("unused", 8),
        If(Cond::Bit("flags", 2), &[U32("value_a")]), If(Cond::Bit("flags", 4), &[Res32("script")]),
        If(Cond::Bit("flags", 8), &[
            Flags("secondary_flags", &["unlocks_rewatch", "has_script_copy"]),
            Res32("secondary_tile_map"), Res32("secondary_scene"), Res32("secondary_setup"), Res32("secondary_merits"),
            If(Cond::Bit("secondary_flags", 2), &[Res32("unused_script_copy")]),
        ]),
        If(Cond::Bit("flags", 0x10), &[I32("spawn_x"), I32("spawn_y")]),
    ],
    // FUN_0054cc50 -> FUN_006531a0: asks the target's AI to do something, for up to `timeout`
    // frames; `x`/`y` for `move_to`.
    // Text static_action_airequest; parse FUN_0054cd60, run FUN_0054cc50, start FUN_0054cc20.
    0x31 "ai_request" 0x54cd60 => [
        // FUN_0054cd60 keeps `byte & 0x3f` (request) and tests bit 7; bit 6 is dropped.
        Inline(TARGET), Split(&[Part::Enum("request", 0x3f, AI_REQUESTS), Part::Rest("unused_bit6", 0x40), Part::Bool("has_timeout", 0x80)]),
        If(Cond::Eq("has_timeout", 1), &[U32("timeout")]),
        If(Cond::Eq("request", 4), &[Fx12("x"), Fx12("y")]),
    ],
    // FUN_00549050: awards `merit` (FUN_006f9580), then plays `script` (script number
    // `script_index` of an event file); `unlock_object` sets an unlock bit (FUN_004469d0).
    // Text static_action_getmerit; parse FUN_00548d10, start FUN_00549050.
    // FUN_00548d10 skips the byte after the cinematic fields; the writer FUN_00548d90 writes 0.
    0x32 "get_merit" 0x548d10 => [U8("script_index"), Res32("script"), Unused8("unused"), Merit("merit"), Res16Z("unlock_object")],
    // FUN_006f9750; `unlock_object` is read but unused.
    // Text static_action_showmerit; parse FUN_0054f8e0, run FUN_0054f8c0, start FUN_0054f890.
    // FUN_0054f8e0 skips the first byte (writer FUN_0054f950 writes 0); run FUN_0054f8c0 reads only +0x24 (merit).
    0x33 "show_merit" 0x54f8e0 => [Unused8("unused"), Merit("merit"), Res16Z("unlock_object")],
    // `path` 0xFF = every waypoint path.
    // Text static_action_enablepath; parse FUN_00542540, run FUN_00542460, start FUN_00542430.
    0x34 "enable_path" 0x542540 => [Enum("action", SWITCH), U8("path")],
    // FUN_005417d0: moves water zone `zone` (0xFF = the one that fired the trigger) by `amount`
    // pixels (up = positive) over `frames`, relative to its level or to the zone bottom.
    // Text static_action_editzone; parse FUN_00541680, run FUN_005417d0, start FUN_00541650.
    // FUN_00541680 skips the first byte; the writer FUN_00541720 writes 0.
    0x35 "edit_zone" 0x541680 => [Unused8("unused"), U8("zone"), U16("frames"), Bool("relative"), I16("amount")],
    // Text static_action_ignorewalls; parse FUN_00549200, run FUN_00549190, start FUN_00549160.
    0x36 "ignore_walls" 0x549200 => [Inline(TARGET), Enum("action", SWITCH)],
    // FUN_0054c460: removes the adjectives with these taxonomy ids (matched on the adjective id;
    // myadjectives = the notebook's adjectives).
    // Text static_action_removeadjectives; parse FUN_0054c650, run FUN_0054c8c0, start FUN_0054c430.
    // FUN_0054c650 skips the byte after the target; the writer FUN_0054c700 writes 0.
    0x37 "remove_adjectives" 0x54c650 => [Inline(TARGET), Unused8("unused"), ListOf("adjectives", &AdjectivePath(""))],
    // Only resolves its target.
    // Text static_action_default ("UNKNOWN"; named from its methods); parse FUN_0064f630, start FUN_0053f4c0.
    0x38 "no_op" 0x64f630 => [Inline(TARGET)],
    // FUN_0066d230: sticks the owner to the trigger target.
    // Text static_action_sticktoother; parse FUN_005523a0, run FUN_005522c0, start FUN_00552290.
    // FUN_005523a0 only skips one byte; the writer FUN_005523b0 writes 0.
    0x39 "stick_to_other" 0x5523a0 => [Unused8("unused")],
    // FUN_00549eb0: health (+0x1f8, capped at +0x1fc) op amount.
    // Text static_action_modifyintegrity; parse FUN_00549f80, run FUN_00549eb0, start FUN_00549e80.
    0x3a "modify_integrity" 0x549f80 => [Inline(TARGET), Enum("operator", OPS), U8("amount")],
    // Text static_action_usepath (TARGET ENTITY, TARGET PATH, ACTION TIMEOUT). Run
    // FUN_0055b410 walks waypoint path `path` from `start`. FUN_0055b520 keeps `byte & 7` and
    // tests bit 3; bits 4-7 are dropped.
    0x3b "use_path" 0x55b520 => [
        Inline(TARGET), Split(&[Part::Enum("start", 0x07, &[(0, "nearest_end"), (1, "first_waypoint"), (2, "last_waypoint")]), Part::Bool("has_timeout", 0x08), Part::Rest("unused_bits", 0xf0)]),
        If(Cond::Eq("has_timeout", 1), &[U16("timeout")]), U8("path"),
    ],
    // "WaitEx" (FUN_0055c380): waits until `variable` compares to `value` (`@name` = another
    // variable) or `timeout` frames pass, then optionally sets another variable.
    // Text static_action_wait; parse FUN_0055c080, start FUN_0055bde0.
    0x3c "wait_for_variable" 0x55c080 => [
        U16("timeout"), Flags("flags", &["has_set", "on_change", "continue_on_match"]), Enum("compare", COMPARE), CStr("variable"), CStr("value"),
        If(Cond::Bit("flags", 1), &[Enum("set_operator", OPS), CStr("set_variable"), CStr("set_value")]),
    ],
    // Text static_action_enableemotes; parse FUN_00542150, run FUN_005420f0, start FUN_005420c0.
    0x3d "enable_emotes" 0x542150 => [Inline(TARGET), Enum("action", SWITCH)],
    // Text static_action_enablemerit; parse FUN_00542390, run FUN_005422f0, start FUN_005422c0.
    0x3e "enable_merit" 0x542390 => [Enum("action", SWITCH), Merit("merit")],
    // FUN_0055a2b0: applies `action` to one of ten switchable things (`kind`) of `target`;
    // `argument` is the instance id (target 4), path index (-1 = all), merit id or list index.
    // Text static_action_toggletarget; parse FUN_0055a2b0, run FUN_00559bd0, start FUN_00559ba0.
    0x3f "toggle_target" 0x55a2b0 => [
        // `quiet`: FUN_0055a2b0 keeps the first byte (+0x30) only for kind `merit`; FUN_00559f00 passes it to
        // FUN_004f77f0, which on disabling with 1 sets merit state 2 instead of 1 and skips the listener
        // callbacks (FUN_004a4790).
        U8Z("quiet"), Enum("kind", TOGGLE_KINDS), Enum("target", TARGET_KINDS),
        Enum("action", SWITCH), I32("argument"),
    ],
    // FUN_0055acf0: one of 16 named commands (MoveCameraPrompt, SetObjectText, NextMap, ...).
    // Text static_action_default ("UNKNOWN"; named from its methods); parse FUN_0055acf0, start FUN_0055a520.
    // FUN_0055acf0 skips the byte after the target. The class writer is the bare target writer FUN_0064f6a0.
    0x40 "script_command" 0x55acf0 => [Inline(TARGET), Unused8("unused"), Str8("command"), ListOf("args", &Str8(""))],
    // Text static_action_spray. Start FUN_00551f70: particle effect `effect` (-1 = keep) from
    // launch point `launch_point` for `duration` frames; the filter is parsed but unused.
    // FUN_00551ef0 resolves the launch point exactly like fire_projectile's FUN_00543f10: with
    // `launch_point_is_id` it is a launcher id, found or created by FUN_006aa1a0 (launcher
    // +0x94, the launch_point hotspot / set_launcher `launcher_id`), else the index of a
    // launch_point hotspot node (FUN_00699960; node kind 7, hotspot kind 9). The byte after
    // `effect` is stored at +0x28 and read by no method (only the writer FUN_00551e50).
    0x41 "spray" 0x551d40 => [
        Flags("flags", &["launch_point_is_id", "has_filter"]), U8("launch_point"), Res32("effect"), Unused8("unused"), U16("duration"),
        If(Cond::Bit("flags", 2), &[Filter("filter")]),
    ],
    // FUN_0054d9c0; names from the objects that use each effect.
    // Text static_action_screeneffect; parse FUN_0054d950, run FUN_0054d9c0, start FUN_0054d2b0.
    // FUN_0054d950 skips the first byte (writer FUN_0054d980 writes 0); +0x24 = action, +0x28 = effect;
    // run FUN_0054d9c0 switches on the effect (FUN_0054d360 .. FUN_0054d810; 1 and 2 share FUN_0054d440).
    0x42 "screen_effect" 0x54d950 => [Unused8("unused"), Enum("action", SWITCH), Enum("effect", SCREEN_EFFECTS)],
    // Text static_action_stoneeffect. Run FUN_00552720 -> FUN_006c28e0(start/255, end/255,
    // duration, mask +0x34, ramp +0x38): turns the target to stone over `duration` seconds.
    // The third texture (+0x3c) and the optional word (+0x40) are only read by the writer
    // FUN_00552680.
    0x43 "stone_effect" 0x552500 => [
        Inline(TARGET), Flags("flags", &["has_unused_value"]), Res32("mask_texture"), Res32("ramp_texture"), Res32("unused_texture"),
        If(Cond::Bit("flags", 1), &[U32("unused_value")]), Fx12("duration"), U8("start_level"), U8("end_level"),
    ],
    // FUN_0053f000: camera follows the target (`release` = give it back).
    // Text static_action_default ("UNKNOWN"; named from its methods); parse FUN_0053f110, run FUN_0053f000, start FUN_0053ef10.
    0x44 "camera_follow" 0x53f110 => [
        Inline(TARGET), Flags("flags", &["has_offset"]), Enum("camera", &[(0, "follow"), (2, "release")]),
        If(Cond::Bit("flags", 1), &[Fx12("offset_x"), Fx12("offset_y")]),
    ],
    // FUN_0055b710: the merit is enabled only while any/all of the entities exist.
    // Text static_action_default ("UNKNOWN"; named from its methods); parse FUN_0055b7e0, start FUN_0055b950.
    0x45 "merit_requires_entities" 0x55b7e0 => [Flags("flags", &["any"]), Merit("merit"), ListOf("entities", &Entity(""))],
    // FUN_0054a030: sight range (+0x3d1) in units of 16 px.
    // Text static_action_modifylineofsight; parse FUN_0054a060, run FUN_0054a030, start FUN_0054a000.
    0x46 "modify_line_of_sight" 0x54a060 => [Inline(TARGET), U8("sight_range")],
    // FUN_00652240
    // Text static_action_removeadjectives; parse FUN_0064f630, run FUN_0054cbe0, start FUN_0054cbb0.
    0x47 "remove_all_adjectives" 0x64f630 => [Inline(TARGET)],
];

const SET_VARIABLE: u8 = 0x19;

/// The second form of `set_variable` (`FUN_0054e9a0`), chosen when the first byte is 0xFF:
/// `u8 0xFF; u8 skipped; cstr variable; u8 operator; cstr value`. The parser jumps over the
/// 0xFF and the next byte (`*param_3 = iVar3 + 2`). Operators (+0x24): its debug text prints
/// 2 "+", 3 "-", 4 "*", 5 "/", 6 ":=", anything else "="; run `FUN_0054e340` assigns for 1,
/// computes for 2-5 (divide by 0 gives 0) and for 6 assigns only while the variable's "set"
/// flag (+0x29) is clear (`init`).
const SET_VARIABLE_OP: &[Field] = &[
    Const(0xff),
    Unused8("unused"),
    CStr("variable"),
    Enum("operator", &[(1, "assign"), (2, "add"), (3, "subtract"), (4, "multiply"), (5, "divide"), (6, "init")]),
    CStr("value"),
];

// ---------------------------------------------------------------------------------------------
// Adjective modifiers

/// Object properties an adjective can change (modifier type 0, `set_property`). Each id is a case
/// of the apply switch `FUN_00621990` (which saves the old value at modifier `+0x20` and writes
/// the new one) and of the revert switch `FUN_00620e50`; the name is the object field it writes
/// (runtime offsets into the object) and matches the `.so` field loaded from the same offset.
/// Ids 3, 4, 5, 7, 0xb, 0xc, 0xe-0x12, 0x15, 0x17-0x19, 0x25, 0x27, 0x29, 0x2b, 0x2f and 0x3c
/// have no case in either switch (no effect); the named ones among them (`unused_*`) are used by
/// shipped adjectives and are named after those adjectives.
pub static PROPERTIES: &[(u8, &str)] = &[
    // +0x270 bit 0 (the `.so` general flag `stationary`); `_animatesbutdoesnotmove`.
    (0x01, "stationary"),
    // +0x270 bit 2, then FUN_006c4570 (gravity on the physics bodies) and FUN_00691310.
    (0x02, "no_gravity"),
    // No case in FUN_00621990/FUN_00620e50: no effect. Set by `useless`, `unlit`, `_disabled`,
    // `deactivated`.
    (0x03, "unused_disabled"),
    // +0x278 and +0x277 (shots per burst; `automatic` 9, `singlefire` 1).
    (0x06, "burst_count"),
    // +0x3fc (has an AI brain) -> FUN_00667a40 / FUN_0068eb90; re-enables equip slots 1, 2, 13.
    (0x08, "animate"),
    // +0x3d1, and +0x3d4 = value^2 * 256 (sight range squared).
    (0x09, "sight_range"),
    // +0x3d8 (visibility percent, 20.12 via FUN_00620020).
    (0x0a, "visibility"),
    // +0x3c0 (temperament, FUN_00620430).
    (0x0d, "aggression"),
    // +0x68c bit 4 (copied to +0x68e bit 4; tested by FUN_00691100 / FUN_006911e0).
    (0x13, "rider_flight"),
    // +0x68c bit 0. Every access to +0x68c in the binary (linear sweep) tests bits 0x10/0x40 or
    // copies bits 1-4 (editor save FUN_006b1d00); bit 0 is only written here and by the revert.
    (0x14, "unused_movement_bit0"),
    // +0x3d0 (attack damage).
    (0x16, "attack_damage"),
    // +0x1a2 via FUN_0066ead0(width, height) (container interior).
    (0x1a, "container_width"),
    // +0x1a3 via FUN_0066ead0(width, height).
    (0x1b, "container_height"),
    // +0x1d8 bit 0.
    (0x1c, "powered"),
    // +0x1d8 bit 2.
    (0x1d, "conductive"),
    // +0x1d8 bits 3-4 (FUN_00620610).
    (0x1e, "shock_mode"),
    // +0x1d8 bit 7.
    (0x1f, "ignores_shock"),
    // +0x1d8 bit 5.
    (0x20, "shocking"),
    // +0x244; only a change away from 2 (liquid/powder) is stored, and the case then falls through
    // into the `background` setter (no `break` in the engine, both in apply and revert).
    (0x21, "solidity"),
    // FUN_006a0760(value): sets +0x245 bit 2 (the `.so` layer flag set on the sky objects
    // environment_sky_*: stars, clouds, zodiac signs) with draw layer +0x246 = 2, makes the object
    // stationary without gravity and removes its seat/sit/equip/climb/fire hotspots; clearing it
    // restores the layer. `earthbound`, `wieldy`, `drivable` clear it (object editor: SKY OBJECT,
    // static_objectproperties[24]).
    (0x22, "sky_object"),
    // +0x245 bit 3 via FUN_004c35d0 (draw layer 3).
    (0x23, "background"),
    // +0x245 bit 1.
    (0x24, "reversible"),
    // No case in FUN_00621990/FUN_00620e50: no effect. Set by `boiled`, `dead`, `paralyzed`,
    // `nonliving`, `_disabled`.
    (0x25, "unused_inert"),
    // +0x24d (can be picked up), then FUN_0069c710 (collision group update).
    (0x26, "grabbable"),
    // +0x1e4 via FUN_006c33b0; material 8 (metal) calls FUN_006aa7b0(0).
    (0x28, "material"),
    // +0x1f8 and +0x1fc (health and maximum health).
    (0x2a, "health"),
    // +0x1f0 (FUN_006207f0).
    (0x2c, "combustion"),
    // +0x208 via FUN_006c3530.
    (0x2d, "buoyancy"),
    // +0x209.
    (0x2e, "waterproof"),
    // +0x25c: the temperature the `.so` loader stores next to the current one (+0x260) and the
    // editor saves back (FUN_006b1d00). Unused by shipped data.
    (0x30, "base_temperature"),
    // +0x264: the cold threshold compared with +0x260 by the become_cold/warm/hot triggers
    // (FUN_005aade0). Unused by shipped data.
    (0x31, "cold_temperature"),
    // +0x268: the hot threshold (FUN_005aade0). Unused by shipped data.
    (0x32, "hot_temperature"),
    // FUN_0069a460 (is burning) -> FUN_0069a740 (ignite) / FUN_0069a560(-100) (extinguish).
    (0x33, "burning"),
    // +0x6bc != 0; FUN_0069a950 / FUN_0069a8a0.
    (0x34, "flammable"),
    // FUN_006a41e0: adds (or re-enables, FUN_0069ab50) an attracting circular force zone of
    // radius max(w, h, 30) * 1.75 (zone class 0xb); clearing disables it.
    (0x35, "magnetic"),
    // Set: FUN_006aa350 (sticky blobs, unless in the static game mode); clear: FUN_006aa540
    // resets stick mode 2 (`sticky`, +0x1ec) to 0.
    (0x36, "sticky"),
    // FUN_0068f300 / FUN_0068f390 (jump strength).
    (0x37, "jump"),
    // +0x646 via FUN_0068f410 (movement speed).
    (0x38, "speed"),
    // +0x740, set only for visible adjectives (0xde when the adjective has flag 0x400,
    // FUN_0064fec0). The only accesses to +0x740 in the binary are this apply and the revert, so
    // the value is never used. Values group adjectives by topic (22 foods, 30 `-philic`,
    // 51 `-phobic`, 8 death).
    (0x39, "unused_topic"),
    // +0x68c bit 1.
    (0x3a, "equip_jump"),
    // +0x68c bit 2.
    (0x3b, "equip_speed"),
    // No case in FUN_00621990/FUN_00620e50: no effect. Set by `dry`, `sundried`.
    (0x3c, "unused_dry"),
    // +0x260: the current temperature (compared by the temperature triggers, FUN_005aade0).
    (0x3d, "temperature"),
    // +0x620 (animated; FUN_006209d0).
    (0x3e, "animated"),
    // +0x1d9 bit 0.
    (0x3f, "chargeable"),
    // +0x24e, then FUN_0069c710 (collision group update: no collisions).
    (0x40, "intangible"),
    // +0x5f0 via FUN_00690e30 on the movement component (+0x59c): clearing stops the current
    // movement controller; death (FUN_00662170) clears it and FUN_00641b80 treats a cleared
    // object as unable to move. Cleared by `dead`, `boiled`, `paralyzed`, `_animatesbutdoesnotmove`.
    (0x41, "can_move"),
    // +0x20b (tested by the extinguish/ignite code FUN_0069a560 / FUN_0069a740).
    (0x42, "inextinguishable"),
    // +0x20c via FUN_006c43a0 (physics body flags 0x40100, propagated to attached objects).
    (0x43, "bouncy"),
    // AI +0x144 (object +0x3f8) via FUN_00653e20 (AI state 0x38).
    (0x44, "asleep"),
    // AI +0x145/+0x144 (object +0x3f9) via FUN_00653ec0 (AI state 0x36).
    (0x45, "sick"),
    // Calls the object's vtable +0x18 (-1, 0x1d, 3): kills it.
    (0x46, "dead"),
    // +0x77c via FUN_00669720 (rope joint to the holder, component +0x774).
    (0x47, "tethered"),
    // +0x77d (tether component +9: FUN_00668d70 pushes the body up, FUN_00669430).
    (0x48, "lighter_than_air"),
    // Only sets +0x274 bit 6, whatever the value; no code tests that bit (every +0x274 access in
    // the binary checked). Set by `_survivalmodeattack` (1), `_survivalmodedefend` (2),
    // `_survivalmodetarget` (3).
    (0x49, "unused_survival_role"),
    // Clamped to -2..2 (FUN_00620ac0): < 0 sets +0x275 bit 1 (silent, as the `silent` destroy
    // flag); 0 clears it and stops the loop sound (FUN_00665fb0(obj, 1), +0x8c4 = 0x8000);
    // > 0 starts periodic noises from the sound table (FUN_00665fb0(obj, 0), +0x8c4 = 0x1000).
    // `silent` -2, `whispering` -1, `loud` 1, `deafening` 2.
    (0x4a, "loudness"),
    // +0x200 (i16): only loaded (FUN_006bbc50), saved (FUN_006b1d00) and written here.
    (0x4b, "unused_4b"),
    // +0x202.
    (0x4c, "blast_proof"),
    // Animation component (+0x814) speed via FUN_006661d0 / FUN_00666170.
    (0x4d, "animation_speed"),
    // +0x275 bit 0 (the AI steal planner FUN_00554850 picks held items with it); `_notstealable`.
    (0x4e, "stealable"),
    // +0x68f bit 1: FUN_00695650 picks a random jump velocity (FUN_00691340).
    (0x4f, "jumpy"),
];

/// Movement abilities: the `.so` `MovementFlags` bits (`movement` modifier, `FUN_005387b0`, masks
/// the equip +0x5f4/+0x600, ability +0x5fc/+0x608 and AI +0x5f8/+0x604 words). Bit 3 makes the
/// body airborne (FUN_00698a30); FUN_00641b80 treats an AI without walk/fly/hover (0x29) on land,
/// or without swim (2) in liquid, as unable to move; object editor MOVEMENT PROPERTIES: FLY, WALK,
/// SWIM, CLIMB, DIVE (static_objectproperties[27-31]).
const MOVEMENT: &[&str] = &["walk", "swim", "dive", "fly", "glide", "hover", "climb"];

/// Value encodings of `set_property` (`FUN_0061fb30` reads 1 byte for 0-2, an i16 for 3 and 12,
/// a u16 for 4, 4 bytes for 5, 6 and 13; nothing else reads the type). The operator helpers
/// (`FUN_0061fd50`, `FUN_0061fe40`, `FUN_00620020`, ...) treat the operand of `multiply`/`divide`
/// as 20.12 fixed point whatever the type. The distinctions the engine does not make come from
/// the data: 0 only with on/off properties, 12 (`i16_enum`) only with `set` on enumerated
/// properties (material, combustion, ...), 13 (`fixed`) only with `multiply`/`divide`.
const VALUE_TYPES: &[(u8, &str)] =
    &[(0, "bool"), (1, "u8"), (2, "i8"), (3, "i16"), (4, "u16"), (5, "i32"), (6, "u32"), (12, "i16_enum"), (13, "fixed")];
/// Explosion sizes: `static_action_explosiontype` in order (SMALL, NORMAL, BIG, AUTO, SUPER,
/// HARMLESS); 7 = none: `fire_projectile`'s impact builder `FUN_00544ef0` adds no explosion
/// when the size is 7.
const EXPLOSIONS: &[(u8, &str)] = &[(0, "small"), (1, "normal"), (2, "big"), (3, "auto"), (4, "super"), (5, "harmless"), (7, "none")];
/// AI moods: the object editor's `static_aimood` list in order (FRIENDLY, NEUTRAL, HOSTILE,
/// SLEEPY, SICK, FRENZY, DEFEND). `change_mood`'s parse (`FUN_00549950`) turns 2 into 5 and its
/// run (`FUN_00549880`) acts only on 3 and 4. Value 7 (one `.sod` use) has no text entry and no
/// code path (a no-op for `change_mood`), so it stays numeric.
const MOODS: &[(u8, &str)] = &[(0, "friendly"), (1, "neutral"), (2, "hostile"), (3, "sleepy"), (4, "sick"), (5, "frenzy"), (6, "defend")];
/// Relation kinds / AI behaviours: the object editor's `static_atrrepmode` list in order
/// (DESTROY, CONSUME, ..., USE VEHICLE; see [`crate::refs::RelationKind`]). `on_ai_action` maps
/// the kind to its event bit with `FUN_00655830`.
const RELATION_KINDS: &[(u8, &str)] = &[
    (0, "destroy"), (1, "consume"), (2, "investigate"), (3, "follow"), (4, "protect"), (5, "use"), (6, "mount"), (7, "steal"), (8, "flee"),
    (9, "split"), (10, "guard"), (11, "split_tool"), (12, "deal_damage"), (13, "fire_projectile"), (14, "use_tool"), (15, "stage_object"),
    (16, "use_vehicle"),
];
/// Target kinds of `attach_to`: `FUN_0053ead0` stores them in the base target slots
/// (+0x24/+0x28), resolved like every action target by `FUN_0064f570` (0 owner, 1 the firing
/// trigger's target from owner+0x9f0, 2 `FUN_006a17c0` carrier, 3 nothing, 4 an entity; as in
/// [`crate::record::TARGETS`]; editor text `static_trigact_action_target`: SELF, TRIGGER TARGET,
/// TERRAIN).
const TARGET_KINDS: &[(u8, &str)] = &[(0, "self"), (1, "trigger_target"), (2, "carrier"), (3, "none"), (4, "entity")];
/// Zone types of the submerged triggers (`FUN_005aac10`): 0 any liquid zone, 1 zones of kind 2
/// (zone+0xda), 2 zones of kind 3. Kind 3 is lava: entering one plays
/// `env_water_splash_lava.gec` (0x278d, `FUN_0046b930`) and sets AIs on fire with `flaming.sa`
/// (0x1ea, `FUN_006a4540`, `FUN_005e2f20` = kind == 3); kind 2 is water (`env_water_splash` /
/// `_swamp` splash, sets the object's wet/liquid contact +0x82d). Object box zones create kind
/// `3 - (liquid != 1)` (`FUN_006b6c00`), level zones `2 + (type & 0xf)` (`FUN_004b3220`). Data:
/// kind 3 only in `e*_lavaland.sod`, `nonflammable.sa`, `golden.sa`; kind 2 in sponges, `dry.sa`.
const ZONE_TYPES: &[(u8, &str)] = &[(0, "any"), (1, "water"), (2, "lava")];
/// Resource kinds (as in `.dps` and dependency lists; see `fmt_common::DependencyKind`): the
/// preload loader `FUN_00494b60` maps them to file-cache types; 7 = audio (cache type 6, the
/// streaming-audio loader `FUN_0051f2f0`).
const DEPENDENCY_KINDS: &[(u8, &str)] =
    &[(0, "data"), (1, "object"), (2, "adjective"), (3, "animation"), (4, "texture"), (5, "vector"), (6, "effect"), (7, "audio")];
/// The last byte of the spawning actions: at most `max_alive` live spawned objects (0 = no
/// limit); `abort_when_full` skips the remaining actions once the limit is reached
/// (`FUN_0054faa0`: +0x2f = byte & 0x7f, +0x3c = byte >> 7; `spawn_object_at`'s start
/// `FUN_00550c10` compares the live count +0x5c with +0x36 and, when full, continues the chain
/// only if +0x37 is clear; `FUN_00550c90` drops dead objects from the live list).
const SPAWN_LIMIT: &[Field] = &[Split(&[Part::Rest("max_alive", 0x7f), Part::Bool("abort_when_full", 0x80)])];
/// Interaction icons the player picks to use an object: the label of id `n` is
/// `static_choice[DAT_00896fc0[n]]` (e.g. 0 and 28 are both "USE", 15 and 51 "RIDE", 1 and 63
/// "USE ANIMAL", so the repeats carry their id); the editor offers the 56 ids listed by
/// `FUN_00595830` (`FUN_005add50`).
const ICONS: &[(u8, &str)] = &[
    (0, "use"), (1, "use_animal"), (2, "attach"), (3, "climb"), (4, "consume"), (5, "dig"),
    (6, "empty_item"), (7, "empty"), (8, "pick_up"), (9, "fill_item"), (10, "fill"), (11, "follow"),
    (12, "guard"), (13, "burn"), (14, "knock"), (15, "ride"), (16, "move_to"), (17, "pet"),
    (18, "play_music"), (19, "protect"), (20, "shoot"), (21, "split"), (22, "spray"), (23, "steal"),
    (24, "attack"), (25, "throw"), (26, "turn_off"), (27, "turn_on"), (28, "use_28"),
    (29, "interact"), (30, "use_vehicle"), (31, "apply"), (32, "clean"), (33, "paint"),
    (34, "play_sound"), (35, "add_adjective"), (36, "breathe_fire"), (37, "cast_magic"),
    (38, "combine"), (39, "cook"), (40, "drive"), (41, "flatten"), (42, "freeze"), (43, "grow"),
    (44, "heal"), (45, "heat"), (46, "hit"), (47, "investigate"), (48, "make_wish"), (49, "play"),
    (50, "raise"), (51, "ride_51"), (52, "scratch"), (53, "shock"), (54, "shoot_turret"),
    (55, "shrink"), (56, "spit"), (57, "spray_water"), (58, "teleport"), (59, "time_travel"),
    (60, "turn_on_off"), (61, "create_object"), (62, "drop"), (63, "use_animal_63"),
];

/// Arithmetic operators: `modify_integrity`'s run `FUN_00549eb0` sets health for 1, adds (2) or
/// subtracts (3) through `FUN_006c3490`, multiplies (4) or divides (5); the same numbering as
/// `set_variable` (`FUN_0054e340`).
const OPS: &[(u8, &str)] = &[(1, "set"), (2, "add"), (3, "subtract"), (4, "multiply"), (5, "divide")];
/// Enable/disable/toggle argument shared by many actions: e.g. `toggle_target`'s handlers
/// (`FUN_00559ca0`: 0 clears, 1 sets, 2 flips obj+0x270 bit 3) and `screen_effect`
/// (`FUN_0054d440`: 0 `FUN_005f3cb0` off, 2 toggle, else on); text `static_action_activatetype`
/// (ACTIVE, INACTIVE, TOGGLE) for the activation actions.
const SWITCH: &[(u8, &str)] = &[(0, "disable"), (1, "enable"), (2, "toggle")];
/// Variable comparisons (`FUN_0045b800`: 0 equal, 1 not equal, 2 `variable > value`, 3
/// `variable < value`; debug text `FUN_005a7010` " == ", " != ", " > ", " < "). The editor's
/// `static_trigger_comparison` list reads EQUAL, NOT EQUAL, FARTHER, LESS.
const COMPARE: &[(u8, &str)] = &[(0, "equal"), (1, "not_equal"), (2, "greater"), (3, "less")];
/// What `toggle_target` switches: `FUN_0055a2b0` stores one handler per kind (+0x38) that the
/// run method `FUN_00559bd0` calls: 0 `FUN_00559ca0` obj+0x270 bit 3 (activation), 1
/// `FUN_00559d20` (`FUN_006a7df0`, the ignore_walls action's call), 2 `FUN_00559d90` (emotes),
/// 3 `FUN_00559df0` waypoint path (+8 of `FUN_004b0660(argument)`, -1 = all, as `enable_path`'s
/// `FUN_00542460`), 4 `FUN_00559ec0` obj+0xb30 (untouchable), 5 `FUN_00559f00` merit
/// (`FUN_004fa310` + `FUN_004f77f0`), 6 `FUN_00559fa0` (flag of `FUN_006f2390(argument)`) and 7
/// `FUN_0055a020` (`FUN_006f23c0(argument)` -> `FUN_005fcd70`): scene-level switchable lists
/// (names data-only), 8 `FUN_0055a070` (`FUN_006bf3d0`, flip), 9
/// `FUN_0055a0d0` AI on/off (`FUN_006857f0`/`FUN_0069ee00` on the object and its attachments).
const TOGGLE_KINDS: &[(u8, &str)] = &[
    (0, "activation"), (1, "ignore_walls"), (2, "emotes"), (3, "path"), (4, "untouchable"), (5, "merit"), (6, "scene_list"),
    (7, "scene_effect"), (8, "facing"), (9, "ai"),
];
/// AI requests: the `REQUEST_TYPE_*` debug names of `FUN_006546f0` (jump table 0x654c60); 26
/// has no debug name but is DROP in the editor list (`FUN_0054d020`: ids at 0x833ea0 ->
/// `static_ai_request` ATTACK, EQUIP, MOUNT, USE EQUIPMENT, USE ON GROUND, FILL, EMPTY, USE AS
/// VEHICLE, RUN (= 33 flee), DISMOUNT, GIVE, STEAL, EAT, FOLLOW, INVESTIGATE, DROP). 15/16 set /
/// clear brain+0x190 (`FUN_006531a0`); 29 is sent as a `move_to` (4) aimed at the target
/// entity by `ai_request`'s run `FUN_0054cc50`. 30/31 (used by `.sod` scripts) clear / set
/// obj+0x68c bit 6 in `FUN_006531a0` (the bit `FUN_004a0940` sets while a player controls the
/// creature and the AI idle code `FUN_00659970` tests); no name found, so they stay numeric.
const AI_REQUESTS: &[(u8, &str)] = &[
    (0, "invalid"), (1, "attack"), (2, "equip"), (3, "mount"), (4, "move_to"), (5, "idle"), (6, "use_equipped"), (7, "use"), (8, "climb"),
    (9, "fill"), (10, "empty"), (11, "fill_equipped"), (12, "empty_equipped"), (13, "use_vehicle"), (14, "panic"), (15, "ai_flag_on"),
    (16, "ai_flag_off"), (17, "burn"), (18, "dismount"), (19, "dig"), (20, "scribble"), (21, "give"), (22, "steal"), (23, "consume"),
    (24, "follow"), (25, "investigate"), (26, "drop"), (29, "move_to_target"), (33, "flee"), (34, "attach_equipped"), (35, "use_self"),
    (36, "creature_command"),
];
/// Screen effects of `screen_effect` (`FUN_0054d9c0` switch; no text or debug names): named after
/// the only shipped users (time machine, arcade, video game / handheld, night-vision, x-ray,
/// thermal goggles, polarised sunglasses, x-ray glasses).
const SCREEN_EFFECTS: &[(u8, &str)] = &[
    (0, "time_warp"), (1, "arcade"), (2, "video_game"), (3, "night_vision"), (4, "xray"), (5, "thermal"), (6, "polarised"), (7, "xray_glasses"),
];

/// Name and schema of each adjective modifier type.
pub struct ModifierDef {
    pub id: u8,
    pub name: &'static str,
    pub fields: &'static [Field],
    /// Parse method address in `Scribble.exe`.
    pub parser: u32,
}

/// Every modifier type the factory (`FUN_0068d970`) knows. Each class's vtable is
/// `[dtor, can_apply, _, apply (+0xc), revert (+0x10), parse (+0x14)]`; the comments name the
/// apply method the fields were traced into. No class exists for type 0x0e.
pub static MODIFIERS: &[ModifierDef] = defs![ModifierDef:
    // FUN_0061fb30 reads property (+0x14), value type (+0x15), operator (+0x16) and the value
    // (+0x18; size by value type, see VALUE_TYPES); apply FUN_00621990 / revert FUN_00620e50
    // switch on the property (PROPERTIES) and combine the old value with `value` through the
    // operator (FUN_0061fe40 & co.: 1 set, 2 add, 3 old - value, 4 multiply, 5 divide by a 20.12
    // factor). `fixed` values print as 20.12 numbers.
    0x00 "set_property" 0x61fb30 => [
        Enum("property", PROPERTIES), Enum("value_type", VALUE_TYPES), Enum("operator", OPS),
        If(Cond::Eq("value_type", 0), &[U8("value")]),
        If(Cond::Eq("value_type", 1), &[U8("value")]),
        If(Cond::Eq("value_type", 2), &[I8("value")]),
        If(Cond::Eq("value_type", 3), &[I16("value")]),
        If(Cond::Eq("value_type", 12), &[I16("value")]),
        If(Cond::Eq("value_type", 4), &[U16("value")]),
        If(Cond::Eq("value_type", 5), &[I32("value")]),
        If(Cond::Eq("value_type", 13), &[Fx12("value")]),
        If(Cond::Eq("value_type", 6), &[U32("value")]),
    ],
    // FUN_0045ff30 / apply FUN_0045fec0: ARGB `color`; alpha != 0xFF only changes the opacity
    // (FUN_006a9fa0, +0xb4a), else FUN_006a9dc0 sets the base colour (+0xb3c, renderer vtable
    // +0x40) or with `shade` the shade colour (+0xb40, vtable +0x4c); revert FUN_0045ffd0 restores
    // 0x00FFFFFF / 0xFF808080.
    0x01 "tint" 0x45ff30 => [U32("color"), Bool("shade")],
    // FUN_0043f380 / apply FUN_0043eec0: relations this object gets and exceptions to them, both
    // inserted by FUN_00658180 (FUN_0043f190 reads each). The byte between the lists is stepped
    // over without being read (`*param_3 += 1`; 13 in every file).
    0x02 "attitudes" 0x43f380 => [Attitudes("attitudes"), Skip8("unused", 13), Attitudes("exceptions")],
    // FUN_004efc40 / apply FUN_004eeea0 -> FUN_004737e0: material overlay `texture` (0x664B =
    // invisible.[texture] also sets flags 0x18) with shader parameters `material_override`
    // (default 1.0, uMaterialOverride), `reflectivity` (default 0, uReflectivity) and
    // `sky_reflection` (default 0.5, uSkyReflectionT). `overlay_key` caches the overlay
    // (FUN_004ef530); it is adjective id | modifier index << 16 | effect index << 24 in all 334
    // shipped modifiers.
    0x03 "material_overlay" 0x4efc40 => [
        Flags("flags", &["has_texture", "has_override", "has_reflectivity", "has_sky_reflection"]), U32("overlay_key"),
        If(Cond::Bit("flags", 1), &[Res32("texture")]), If(Cond::Bit("flags", 2), &[Fx12("material_override")]),
        If(Cond::Bit("flags", 4), &[Fx12("reflectivity")]), If(Cond::Bit("flags", 8), &[Fx12("sky_reflection")]),
    ],
    // FUN_00628e00 / apply FUN_00628d30: deletes the object's behaviours whose trigger type
    // (vtable +0x2c) is listed.
    0x04 "remove_behaviours" 0x628e00 => [ListOf("triggers", &TriggerType(""))],
    // FUN_00434f70 / apply FUN_00434bf0: builds the trigger (FUN_006d36e0, repeatable = !bit 7)
    // and its actions (FUN_0064e1f0) and adds them to the object.
    0x05 "add_behaviour" 0x434f70 => [Behaviour("behaviour")],
    // FUN_006e94a0 / apply FUN_006e9960. The flags byte: bits 0-3 `slot` (>= 8 = the whole
    // object, stored as -1), bit 6 `local_only` (stored inverted: when clear, connected objects
    // are resized too, FUN_0068de80), bit 7 `along_long_axis` (the larger percentage goes to the
    // object's longer side); bits 4-5 are never read. The whole object grows by
    // `grow_x_pct`/`grow_y_pct` percent (FUN_006a4b80); for an equip slot the two values instead
    // offset that slot's equip_slot hotspots (FUN_0069f5f0).
    0x06 "resize" 0x6e94a0 => [
        Split(&[Part::Num("slot", 0x0f), Part::Rest("unused_bits", 0x30), Part::Bool("local_only", 0x40), Part::Bool("along_long_axis", 0x80)]),
        I32("grow_x_pct"), I32("grow_y_pct"),
    ],
    // FUN_00474900 -> FUN_00474420 / apply FUN_00474810 -> FUN_00474710: (re)configures the
    // launcher with id `launcher_id` (FUN_006aa1a0), the same fields as the launch_point hotspot:
    // `spread` (degrees -> radians, launcher +0x70), `unused_flag` (+0x7c: only copied and saved
    // by the hotspot writer FUN_004d4b40, never read by the launcher), `gravity` (+0x7d), `speed`
    // (+0x74), `interval` (+0x84/+0x88), `burst` (+0x7e), `aim_x`/`aim_y` (+0x68/+0x6c),
    // `particles` (+0x90: fire a particle stream instead of an object) and the object or stream
    // (+0x80): `stream_type` is the stream id FUN_00474f60 switches on (2879 fire, 5053 snow
    // particles).
    0x07 "set_launcher" 0x474900 => [
        Flags("flags", &["effect_only"]),
        IfElse(Cond::Bit("flags", 1), &[Res32("effect")], &[
            Fx12("spread"), Bool("unused_flag"), Bool("gravity"), Fx12("speed"), U32("interval"), U8("burst"),
            Fx12("aim_x"), Fx12("aim_y"), Bool("particles"), IfElse(Cond::Eq("particles", 1), &[U16("stream_type")], &[Res16("projectile")]),
        ]),
        U8("launcher_id"),
    ],
    // FUN_00465530 / apply FUN_00465810 -> FUN_00465680 (FUN_00465560): rewrites the amount of
    // every deal_damage action with the operator.
    0x08 "modify_damage" 0x465530 => [U8("amount"), Enum("operator", OPS)],
    // FUN_00712d70 / apply FUN_00712bf0: enables (or with `enabled` false disables, +0xdb) the
    // object's thermal zone (zone class 0x11), creating one of `temperature` (<< 12) if needed.
    0x09 "heat_aura" 0x712d70 => [Bool("enabled"), I32("temperature")],
    // FUN_00434850 / apply FUN_00434970: adds a runtime effect node (like tree node type 9) at
    // (`x_pct`, `y_pct`) percent of the half size: `vector`, flipbook `animation`, draw `layer`,
    // playback `speed` (0 when `stopped`) and `flipbook` texture.
    0x0a "add_effect" 0x434850 => [
        I8("x_pct"), I8("y_pct"), Res16("vector"), U8("animation"), I8("layer"), Fx12("speed"), Bool("stopped"), Res16("flipbook"),
    ],
    // FUN_00433f40 / apply FUN_00434260: with `equip`, equips `object` (display word
    // `name_word`, `adjectives` via FUN_0053d8d0) in equip slot `slot` (FUN_004341d0), or seats it
    // (0x10 = sit point FUN_00433b40, 0x90 = seat FUN_00433bc0, the codes toggle_hotspot uses);
    // without, empties the slot (FUN_00433df0).
    0x0b "equip" 0x433f40 => [Bool("equip"), If(Cond::NonZero("equip"), &[Res32("object"), Id16("name_word"), Adjectives("adjectives")]), U8("slot")],
    // FUN_004ed960 / apply FUN_004ed830: `set` the weight (+0x203, FUN_006c4520) or `adjust` it by
    // `delta` (FUN_006c3d00).
    0x0c "weight" 0x4ed960 => [Enum("change", &[(0, "set"), (1, "adjust")]), If(Cond::Eq("change", 0), &[U8("weight")]), If(Cond::Eq("change", 1), &[I8("delta")])],
    // FUN_00538710 / apply FUN_005387b0: (movement | add) & !remove for the equip (+0x5f4/+0x600),
    // ability (+0x5fc/+0x608) and AI (+0x5f8/+0x604) movement words.
    0x0d "movement" 0x538710 => [
        Flags("add_equip", MOVEMENT), Flags("remove_equip", MOVEMENT), Flags("add_abilities", MOVEMENT),
        Flags("remove_abilities", MOVEMENT), Flags("add_ai", MOVEMENT), Flags("remove_ai", MOVEMENT),
    ],
    // FUN_004772e0 / apply FUN_00477ec0 -> FUN_00477790: enables or disables the hotspots of a kind
    // (bits 0-6; bit 7 `enable`); seat and sit_point are addressed as slots 0x90 / 0x10, equip
    // slots by the extra `slot` byte.
    0x0f "toggle_hotspot" 0x4772e0 => [
        Split(&[Part::Enum("hotspot_kind", 0x7f, crate::tree::HOTSPOT_KINDS), Part::Bool("enable", 0x80)]),
        If(Cond::Eq("hotspot_kind", 5), &[U8("slot")]),
    ],
    // FUN_00622c10 / apply FUN_00622b20: deletes the object's actions of this type (and
    // behaviours left empty).
    0x10 "remove_actions" 0x622c10 => [ActionType("action")],
    // FUN_006ee1c0 / apply FUN_006ee150: per-slot sound overrides (+0x90c + slot*12); the byte after
    // each sound is stepped over without being read (1 in every file).
    0x11 "sounds" 0x6ee1c0 => [List("sounds", &[Enum("slot", crate::so::SOUND_SLOTS), Res32("sound"), Skip8("unused", 1)])],
    // FUN_00436150 / apply FUN_00439280: replaces the object with `replace_with`, or when null
    // with its counterpart of that gender (object +0x798 for male/female objects, else its own
    // type +0x16c).
    0x12 "set_gender" 0x436150 => [Enum("gender", &[(1, "male"), (2, "female")]), Res32("replace_with")],
];

// ---------------------------------------------------------------------------------------------
// Lookup

fn behaviour_def(id: u8) -> Result<&'static BehaviourDef> {
    BEHAVIOURS.iter().find(|d| d.id == id).ok_or_else(|| anyhow!("unknown behaviour type {id:#04x}"))
}
fn action_def(id: u8) -> Result<&'static ActionDef> {
    ACTIONS.iter().find(|d| d.id == id).ok_or_else(|| anyhow!("unknown action type {id:#04x}"))
}
fn modifier_def(id: u8) -> Result<&'static ModifierDef> {
    MODIFIERS.iter().find(|d| d.id == id).ok_or_else(|| anyhow!("unknown modifier type {id:#04x}"))
}

fn id_from_name<'a>(name: &str, mut table: impl Iterator<Item = (u8, &'a str)>, prefix: &str) -> Result<u8> {
    if let Some((id, _)) = table.find(|(_, n)| *n == name) {
        return Ok(id);
    }
    if let Some(hex) = name.strip_prefix(prefix)
        && let Ok(v) = u8::from_str_radix(hex, 16)
    {
        return Ok(v);
    }
    bail!("unknown {prefix}type {name:?}")
}

// ---------------------------------------------------------------------------------------------
// Types

/// A behaviour: an event trigger plus the actions it runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Behaviour {
    /// Type id (see [`BEHAVIOURS`]).
    pub kind: u8,
    /// Bit 7 of the type byte: the trigger fires only once (the engine keeps `repeatable =
    /// !bit7` at +0x29, the editor's REPEATABLE box; `on_created` always fires once).
    pub once: bool,
    /// Type-specific fields, in file order.
    pub fields: Map<String, Value>,
    /// Actions run when the trigger fires.
    pub actions: Vec<Action>,
}

/// An action run by a behaviour.
#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    /// Type id (see [`ACTIONS`]).
    pub kind: u8,
    pub fields: Map<String, Value>,
}

/// An adjective modifier.
#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    /// Type id (see [`MODIFIERS`]).
    pub kind: u8,
    /// Bit 7 of the type byte, passed by the factory `FUN_0068d970` to modifier `+0x12`: the
    /// modifier is also applied when the adjective is inherited by objects this one creates or
    /// equips. `FUN_0068de30` runs apply (vtable `+0xc`) on an inherited adjective instance
    /// (`+0x54 & 0x800`) only when `+0x12` is set and the adjective is `inheritable` (`+0x32`).
    /// JSON key `inheritable`.
    pub inheritable: bool,
    pub fields: Map<String, Value>,
}

pub(crate) struct Hooks;

impl Nested for Hooks {
    fn read_behaviour(r: &mut Reader, ctx: &Context) -> Result<Value> {
        Ok(serde_json::to_value(Behaviour::read(r, ctx)?)?)
    }
    fn write_behaviour(v: &Value, w: &mut Writer, ctx: &Context) -> Result<()> {
        let b: Behaviour = serde_json::from_value(v.clone())?;
        b.write(w, ctx)
    }
    fn read_action(r: &mut Reader, ctx: &Context) -> Result<Value> {
        Ok(serde_json::to_value(Action::read(r, ctx)?)?)
    }
    fn write_action(v: &Value, w: &mut Writer, ctx: &Context) -> Result<()> {
        let a: Action = serde_json::from_value(v.clone())?;
        a.write(w, ctx)
    }
}

impl Behaviour {
    /// Read a behaviour entry (type byte, fields, action list).
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let at = r.pos();
        let tf = r.u8()?;
        let kind = tf & 0x7f;
        let def = behaviour_def(kind).map_err(|e| anyhow!("{e} at {at:#x}"))?;
        let fields = read_fields::<Hooks>(def.fields, r, ctx).map_err(|e| anyhow!("behaviour {} at {at:#x}: {e}", def.name))?;
        let n = r.u8()?;
        let actions = (0..n).map(|_| Action::read(r, ctx)).collect::<Result<_>>()?;
        Ok(Behaviour { kind, once: tf & 0x80 != 0, fields, actions })
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        let def = behaviour_def(self.kind)?;
        w.u8(self.kind | if self.once { 0x80 } else { 0 });
        write_fields::<Hooks>(def.fields, &self.fields, w, ctx).map_err(|e| anyhow!("behaviour {}: {e}", def.name))?;
        crate::util::count8(w, self.actions.len(), "actions")?;
        for a in &self.actions {
            a.write(w, ctx)?;
        }
        Ok(())
    }
    /// A new behaviour of the given type with default field values.
    pub fn new(kind: u8) -> Result<Self> {
        Ok(Behaviour { kind, once: false, fields: crate::record::default_fields(behaviour_def(kind)?.fields), actions: vec![] })
    }
    pub fn name(&self) -> String {
        BEHAVIOURS.iter().find(|d| d.id == self.kind).map_or_else(|| format!("behaviour_{:02x}", self.kind), |d| d.name.to_string())
    }
}

impl Action {
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let at = r.pos();
        let kind = r.u8()?;
        let def = action_def(kind).map_err(|e| anyhow!("{e} at {at:#x}"))?;
        let schema = if kind == SET_VARIABLE && r.peek_u8()? == 0xff { SET_VARIABLE_OP } else { def.fields };
        let fields = read_fields::<Hooks>(schema, r, ctx).map_err(|e| anyhow!("action {} at {at:#x}: {e}", def.name))?;
        Ok(Action { kind, fields })
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        let def = action_def(self.kind)?;
        w.u8(self.kind);
        let schema = if self.kind == SET_VARIABLE && self.fields.contains_key("operator") { SET_VARIABLE_OP } else { def.fields };
        write_fields::<Hooks>(schema, &self.fields, w, ctx).map_err(|e| anyhow!("action {}: {e}", def.name))
    }
    pub fn new(kind: u8) -> Result<Self> {
        Ok(Action { kind, fields: crate::record::default_fields(action_def(kind)?.fields) })
    }
    pub fn name(&self) -> String {
        ACTIONS.iter().find(|d| d.id == self.kind).map_or_else(|| format!("action_{:02x}", self.kind), |d| d.name.to_string())
    }
}

impl Modifier {
    pub fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let at = r.pos();
        let tf = r.u8()?;
        let kind = tf & 0x7f;
        let def = modifier_def(kind).map_err(|e| anyhow!("{e} at {at:#x}"))?;
        let fields = read_fields::<Hooks>(def.fields, r, ctx).map_err(|e| anyhow!("modifier {} at {at:#x}: {e}", def.name))?;
        Ok(Modifier { kind, inheritable: tf & 0x80 != 0, fields })
    }
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        let def = modifier_def(self.kind)?;
        w.u8(self.kind | if self.inheritable { 0x80 } else { 0 });
        write_fields::<Hooks>(def.fields, &self.fields, w, ctx).map_err(|e| anyhow!("modifier {}: {e}", def.name))
    }
    pub fn new(kind: u8) -> Result<Self> {
        Ok(Modifier { kind, inheritable: false, fields: crate::record::default_fields(modifier_def(kind)?.fields) })
    }
    pub fn name(&self) -> String {
        MODIFIERS.iter().find(|d| d.id == self.kind).map_or_else(|| format!("modifier_{:02x}", self.kind), |d| d.name.to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// JSON: {"type": name, [flag], ...fields, ["actions": [...]]}

impl Serialize for Behaviour {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut m = Map::new();
        m.insert("type".into(), self.name().into());
        if self.once {
            m.insert("once".into(), true.into());
        }
        for (k, v) in &self.fields {
            m.insert(k.clone(), v.clone());
        }
        if !self.actions.is_empty() {
            m.insert("actions".into(), serde_json::to_value(&self.actions).map_err(serde::ser::Error::custom)?);
        }
        m.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Behaviour {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut m = Map::deserialize(d)?;
        let name = m.remove("type").and_then(|v| v.as_str().map(str::to_string)).ok_or_else(|| serde::de::Error::custom("behaviour needs \"type\""))?;
        let kind = id_from_name(&name, BEHAVIOURS.iter().map(|d| (d.id, d.name)), "behaviour_").map_err(serde::de::Error::custom)?;
        let once = m.remove("once").and_then(|v| v.as_bool()).unwrap_or(false);
        let actions = match m.remove("actions") {
            Some(v) => serde_json::from_value(v).map_err(serde::de::Error::custom)?,
            None => vec![],
        };
        Ok(Behaviour { kind, once, fields: m, actions })
    }
}

impl Serialize for Action {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut m = Map::new();
        m.insert("type".into(), self.name().into());
        for (k, v) in &self.fields {
            m.insert(k.clone(), v.clone());
        }
        m.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut m = Map::deserialize(d)?;
        let name = m.remove("type").and_then(|v| v.as_str().map(str::to_string)).ok_or_else(|| serde::de::Error::custom("action needs \"type\""))?;
        let kind = id_from_name(&name, ACTIONS.iter().map(|d| (d.id, d.name)), "action_").map_err(serde::de::Error::custom)?;
        Ok(Action { kind, fields: m })
    }
}

impl Serialize for Modifier {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut m = Map::new();
        m.insert("type".into(), self.name().into());
        if self.inheritable {
            m.insert("inheritable".into(), true.into());
        }
        for (k, v) in &self.fields {
            m.insert(k.clone(), v.clone());
        }
        m.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Modifier {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let mut m = Map::deserialize(d)?;
        let name = m.remove("type").and_then(|v| v.as_str().map(str::to_string)).ok_or_else(|| serde::de::Error::custom("modifier needs \"type\""))?;
        let kind = id_from_name(&name, MODIFIERS.iter().map(|d| (d.id, d.name)), "modifier_").map_err(serde::de::Error::custom)?;
        let inheritable = m.remove("inheritable").and_then(|v| v.as_bool()).unwrap_or(false);
        Ok(Modifier { kind, inheritable, fields: m })
    }
}

/// Read a u8-counted behaviour list.
pub fn read_behaviours(r: &mut Reader, ctx: &Context) -> Result<Vec<Behaviour>> {
    let n = r.u8()?;
    (0..n).map(|_| Behaviour::read(r, ctx)).collect()
}

/// Write a u8-counted behaviour list.
pub fn write_behaviours(list: &[Behaviour], w: &mut Writer, ctx: &Context) -> Result<()> {
    crate::util::count8(w, list.len(), "behaviours")?;
    for b in list {
        b.write(w, ctx)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Field;

    fn names(fields: &[Field], out: &mut Vec<&'static str>) {
        for f in fields {
            match *f {
                Field::If(_, fs) | Field::Inline(fs) => names(fs, out),
                Field::IfElse(_, a, b) => {
                    names(a, out);
                    names(b, out);
                }
                Field::Split(parts) => out.extend(parts.iter().map(|p| match *p {
                    Part::Num(n, _) | Part::Bool(n, _) | Part::Enum(n, _, _) | Part::Rest(n, _) => n,
                })),
                Field::Const(_) => {}
                ref f => out.extend(crate::record::field_name_of(f)),
            }
        }
    }

    /// Keys the record wrappers use themselves: a trigger's `type`, `once`, `actions`, a scene
    /// trigger's `for_merit`, an event action's `op` and `trailing`.
    const RESERVED: &[&str] = &["type", "once", "inheritable", "actions", "for_merit", "op", "trailing"];

    fn splits(fields: &[Field], out: &mut Vec<&'static [Part]>) {
        for f in fields {
            match *f {
                Field::If(_, fs) | Field::Inline(fs) | Field::List(_, fs) => splits(fs, out),
                Field::IfElse(_, a, b) => {
                    splits(a, out);
                    splits(b, out);
                }
                Field::Split(parts) => out.push(parts),
                _ => {}
            }
        }
    }

    /// Split bytes must be lossless: the parts' masks cover every bit exactly once.
    #[test]
    fn split_masks_cover_the_byte() {
        let mut all = Vec::new();
        for fields in BEHAVIOURS.iter().map(|d| d.fields).chain(ACTIONS.iter().map(|d| d.fields)).chain(MODIFIERS.iter().map(|d| d.fields)) {
            splits(fields, &mut all);
        }
        for h in crate::tree::HOTSPOTS {
            splits(h.fields, &mut all);
        }
        assert!(!all.is_empty());
        for parts in all {
            let mut seen = 0u8;
            for p in parts {
                let m = match *p {
                    Part::Num(_, m) | Part::Bool(_, m) | Part::Enum(_, m, _) | Part::Rest(_, m) => m,
                };
                assert_eq!(seen & m, 0, "overlapping split masks");
                seen |= m;
            }
            assert_eq!(seen, 0xff, "split masks must cover all bits");
        }
    }

    #[test]
    fn field_names_not_reserved() {
        let all = BEHAVIOURS.iter().map(|d| (d.name, d.fields)).chain(ACTIONS.iter().map(|d| (d.name, d.fields))).chain(MODIFIERS.iter().map(|d| (d.name, d.fields)));
        for (name, fields) in all.chain([("set_variable_op", SET_VARIABLE_OP)]) {
            let mut v = Vec::new();
            names(fields, &mut v);
            for n in &v {
                assert!(!RESERVED.contains(n), "{name}: field {n:?} is reserved");
            }
        }
        let mut ids: Vec<_> = BEHAVIOURS.iter().map(|d| d.name).collect();
        ids.extend(ACTIONS.iter().map(|d| d.name));
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "trigger/action names must be unique");
    }
}
