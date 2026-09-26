//! `.so` - scribble object definition: everything a spawnable noun is (loaded by
//! `FUN_006bbd70`, "SCRIB OBJECT LOAD").
//!
//! ```text
//! u16 category, u16 subcategory, u16 group, u16 object   taxonomy id of this object
//! u8  gender          low nibble Gender (+0x790), high nibble DefaultGender (FUN_006c05f0)
//! u32 link            Male/Female: the counterpart object; Both: offset of the female body
//! Body body           at offset 13
//! [Both] Body female  at `link`
//! ```
//!
//! A **body** is a sequence of sections, most introduced by a constant marker byte:
//!
//! ```text
//! u32 deps_offset      absolute offset of the dependency list (end of this body)
//! u32 tags_offset      absolute offset of the 0x0E tags marker
//! Budget budget        spawn-budget cost (40 bytes + 3 optional sub-budgets)
//! u16 width, u16 height
//! 0x0A General     0x0D Movement    0x01 Stats + 2 relation lists
//! 0x02 Container   0x03 Electrical  0x04 Layer     0x05 Physical
//! 0x06 Thermal     0x07 behaviours (u16 byte length, u8 count, entries)
//! 0x08 u8 character_body, u8 physics_bodies, node tree
//! 0x0B sound table [0x09 animation table]  0x0E tags   dependency list
//! ```
//!
//! Field names come from the engine code that consumes each value (runtime offsets are given
//! as `+0x...` into the object instance) and from the adjective property table
//! (`FUN_00621990`), which maps adjective names onto the same runtime fields. The engine's own
//! `.so` writer (`FUN_006b1d00`, used for custom objects) writes every section back from the
//! same runtime fields, which confirms each mapping. Evidence per field:
//! `docs/evidence/object.md`.

use crate::behaviour::{read_behaviours, write_behaviours, Behaviour};
use crate::deps::{self, Dependency};
use crate::refs::{ObjectPath, RefList, Relation};
use crate::tree::{Flavor, Node};
use crate::util::{count8, enum8, flags8, marker};
use scribble_core::{bail, ensure, Context, Format, Fx12, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

/// A `.so` file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScribbleObject {
    /// This object's taxonomy id.
    pub id: ObjectPath,
    pub gender: Gender,
    /// High nibble of the gender byte: which body a `both` object spawns with when no gender
    /// adjective decides (`FUN_006c05f0`: 2 sets spawn flag 0x40 = female body, 1 sets 0x80 =
    /// male, 0 draws at random). Named characters use it (Leonardo da Vinci `male`, Emily Cox
    /// `female`). Omitted when `random`.
    #[serde(default, skip_serializing_if = "DefaultGender::is_random")]
    pub default_gender: DefaultGender,
    /// Male/female objects: the object of the other gender (e.g. rooster <-> chicken;
    /// `FUN_006c05f0` swaps to it when the requested gender differs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterpart: Option<ResRef>,
    /// The body (the male body when there are two).
    pub body: Body,
    /// The female body (spawn flag 0x40 selects it). Always present for gender `both`; five
    /// genderless files also carry one (the loader uses it whenever the link is non-zero and
    /// the gender is not `female`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub female_body: Option<Body>,
}

enum8!(
    /// Gender of the object (low nibble of byte 8, `+0x790`; `FUN_006c05f0` / `FUN_006bbd70`).
    Gender {
        0 => None "none",
        1 => Male "male",
        2 => Female "female",
        /// Two bodies: male first, female second (spawn flag 0x40 selects the female one).
        3 => Both "both",
    }
);

enum8!(
    /// Body a `both` object spawns with by default (`FUN_006c05f0`).
    DefaultGender {
        /// Picked with the engine's random generator.
        0 => Random "random",
        /// Spawn flag 0x80.
        1 => Male "male",
        /// Spawn flag 0x40.
        2 => Female "female",
    }
);

impl DefaultGender {
    pub fn is_random(&self) -> bool {
        *self == DefaultGender::Random
    }
}

#[allow(clippy::derivable_impls)]
impl Default for DefaultGender {
    fn default() -> Self {
        DefaultGender::Random
    }
}

/// One body of an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub budget: Budget,
    /// Object size in world pixels (`+0x7d8`/`+0x7da`, originals kept at `+0x7dc`/`+0x7de`,
    /// which the writer saves; e.g. `FUN_0058cd80` sizes an auto explosion from max(w, h)).
    pub width: u16,
    pub height: u16,
    pub general: General,
    pub movement: Movement,
    pub stats: Stats,
    /// How this object's AI reacts to other objects (`FUN_00658830`, first list).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
    /// Exceptions: a same-kind match here cancels the reaction (`FUN_00656100`). JSON
    /// `relation_exceptions`.
    #[serde(default, rename = "relation_exceptions", skip_serializing_if = "Vec::is_empty")]
    pub reverse_relations: Vec<Relation>,
    pub container: Container,
    pub electrical: Electrical,
    pub layer: Layer,
    pub physical: Physical,
    pub thermal: Thermal,
    /// Event triggers and their actions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub behaviours: Vec<Behaviour>,
    /// Section 0x08 byte 1: non-zero (or `stats.flags` `animate`) makes `FUN_006bbd70` build
    /// a character controller (`FUN_005be970`, `FUN_005bca20`) instead of a plain rigid body
    /// (0x006bcb92 onwards). Matches `animate` in 99% of shipped bodies.
    pub character_body: u8,
    /// Section 0x08 byte 2: values > 1 make `FUN_006bbd70` build a composite body for the
    /// shapes (`FUN_005c85e0`, or `FUN_005c6f10` with `body_flags` `articulated`); 0/1 = a
    /// single body.
    pub physics_bodies: u8,
    /// The node tree (visual parts, physics shapes, hotspots).
    pub root: Node,
    /// Per-slot sound entries (section 0x0B; stored at `+0x90c + slot*12` by
    /// `FUN_006bbd70`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sounds: Vec<SoundEntry>,
    /// Animation table (section 0x09), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animations: Option<AnimationTable>,
    /// Tag ids (section 0x0E; `FUN_006bbd70` keeps them at `+0xb00`/`+0xb04` only when the
    /// object's vtable slot 9 says so; read by the object suggestor through `FUN_006b66c0`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<scribble_core::NamedId>,
    /// Resources to preload.
    pub dependencies: Vec<Dependency>,
}

/// Counters shared by the main budget and the sub-budgets. `FUN_0066a050` (main) and
/// `FUN_00669860` (sub-budgets) load them into a budget record (offsets below are into that
/// record); `FUN_00669c70` adds a record to the level total, `FUN_00669b30` subtracts it,
/// `FUN_006699a0` checks the total against the limits, `FUN_006b12c0` recomputes a record when
/// the object editor saves, and `FUN_00669d10`/`FUN_00669ec0` write it back.
///
/// ```text
/// u16 node_count (loaders read only the low byte); u8 jointed_part_count; u32 vector_memory;
/// u32 unused_memory; u8 relation_count; 4 x u8[6] trigger_stats
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BudgetCounters {
    /// Tree nodes below the root (record `+8`; `FUN_006b12c0` adds 1 per node, matching the
    /// low byte in 93% of files). Only the low byte is loaded; the high byte (no reader) is
    /// roughly the number of vector nodes in shipped data. Not added to the level total for
    /// brainless `scenery` (`FUN_00669c70`).
    pub node_count: u16,
    /// Hotspots of kind `jointed_part` (record `+0xc`; `FUN_006b12c0` counts type-7 nodes whose
    /// kind is 0xc). JSON `jointed_part_count` (formerly `attached_part_count`).
    pub jointed_part_count: u8,
    /// Size of the vector art (record `+0x10`; `FUN_006b12c0` sums the resource-table size
    /// (`DAT_008a8290`, 14-byte entries, u32 at +10) of every node's `.vec` except mesh parts).
    pub vector_memory: u32,
    /// Record `+0x14`: loaded, summed into the level total and written back, but nothing
    /// compares it and `FUN_006b12c0` never computes it (about 0.36 x `vector_memory` in
    /// shipped files).
    pub unused_memory: u32,
    /// Relations + relation exceptions (record `+0x1a`; `FUN_006b12c0` adds `+0x368` and
    /// `+0x378`, the counts of the two relation lists).
    pub relation_count: u8,
    pub trigger_stats: TriggerStats,
}

/// Behaviour statistics written by the offline object compiler: per trigger group six
/// counters `[triggers, projectile_actions, spawn_actions, presentation_actions,
/// other_actions, unused]`. Data-only names: they match the shipped behaviours statistically
/// (`collision[0]` = `on_collide` behaviours in 93% of bodies, `proximity[0]` =
/// `on_object_in_sight`/`distance`/`on_sees_object`/`object_count_in_area` behaviours in 99.9%,
/// counter 2 = `spawn_*` actions, counter 1 = `fire_projectile`/`throw_at_target`). The engine
/// loads only counter 0 of each group (record `+0x1c`, `+0x1e`, `+0x20`, `+0x22`); only
/// `other[0]` is used: for custom objects it is a cost capped at 2000 per level
/// (`FUN_006699a0`; `FUN_006b12c0` fills it from type-11 nodes), for game objects the loader
/// replaces it with 1. The engine's writer stores zero in every other counter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TriggerStats {
    pub collision: [u8; 6],
    pub proximity: [u8; 6],
    /// Zero in every file (counter 0 loads into record `+0x20`, which nothing reads).
    pub unused: [u8; 6],
    pub other: [u8; 6],
}

/// Spawn budget of an object (`FUN_0066a050`).
///
/// ```text
/// u8 budget_cost; u8 object_count; u16 node_count; u8 scenery; u8 jointed_part_count;
/// u32 vector_memory; u32 unused_memory; u8 has_brain; u8 relation_count;
/// u8 trigger_stats[4][6]                               (40 bytes, main counters)
/// 3 x { u8 count; [count] 36-byte BudgetCounters }      attached, equipment, contents
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    /// Budget points the object costs (record `+4`, summed into the level total by
    /// `FUN_00669c70`).
    pub budget_cost: u8,
    /// Objects spawned (record `+6`; 1 in every file, `FUN_0066a050` adds each sub-budget's
    /// `count`). `FUN_006699a0` refuses the spawn when it exceeds the free entity slots
    /// (`FUN_005cc1b0`) or free player slots (`FUN_0047a270`).
    pub object_count: u8,
    /// Scenery object (record `+0xa`; `FUN_006b12c0` sets it for `solidity` 1 or the
    /// `sky_object` layer). Brainless scenery does not add its nodes to the level total.
    pub scenery: u8,
    /// Has an AI brain (record `+0x18`; `FUN_006b12c0` copies `stats.flags` `animate`,
    /// `+0x3fc`); the level total counts brains (`FUN_00669c70`).
    pub has_brain: u8,
    #[serde(flatten)]
    pub counters: BudgetCounters,
    /// Budget of objects attached to this one (first sub-budget; `FUN_0066a050` skips it
    /// with spawn flag 0x20).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached: Option<SubBudget>,
    /// Budget of equipment (second sub-budget; skipped with spawn flag 0x10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equipment: Option<SubBudget>,
    /// Budget of container contents (third sub-budget; only counted with spawn flag 0x02,
    /// the flag the loader uses to spawn `container.contents`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contents: Option<SubBudget>,
}

/// A sub-budget: `count` instances costing `counters` each (`FUN_00669860`, which adds the
/// counters once and `count` to the object count; it skips `trigger_stats` except to add 1 to
/// record `+0x22`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SubBudget {
    /// Number of instances (added to the main record `+6` by `FUN_0066a050`).
    pub count: u8,
    #[serde(flatten)]
    pub counters: BudgetCounters,
}

flags8!(
    /// Section 0x0A flags byte (`FUN_006bbd70`; written back by `FUN_006b1d00`). Bit 4 = spawn
    /// list present and bits 5-7 = [`SizeFit`] are kept separately.
    GeneralFlags {
        /// `+0x270` bit 0; adjective property 0x01 (`FUN_00621990`) `stationary` ("animates
        /// but does not move").
        0 => "stationary",
        /// `+0x270` bit 2; adjective property 0x02 toggles gravity through `FUN_006c4570`
        /// (sky objects, balloons).
        1 => "no_gravity",
        /// Starts inactive (stored inverted at `+0x270`/`+0x274` bit 3 = active, the bit the
        /// activate/deactivate actions switch; switches, electronics).
        2 => "starts_inactive",
        /// `+0x270` bit 1: picked by its bounding box instead of per pixel (`FUN_0069d0b0`;
        /// cages, ice blocks).
        3 => "bounds_hit_test",
    }
);

flags8!(
    /// Section 0x0A second flags byte, `body_flags`. Bits 0-3 are [`VehicleControl`] and bit 5
    /// (reflection present) is derived; both are kept separately. Bits 6-7 have no reader.
    GeneralFlags2 {
        /// `+0x272` bit 7: a composite body (`physics_bodies` > 1) uses the articulated
        /// variant (`FUN_005c6f10` instead of `FUN_005c85e0` in `FUN_006bbd70`).
        4 => "articulated",
    }
);

enum8!(
    /// Built-in vehicle handling (`body_flags` bits 0-3). A non-zero value makes the loader add
    /// a hidden trigger of class 0x50 (`FUN_005ae390`, vtable `0x0083ab78`) holding the value
    /// at `+0x30`; the engine's writer reads it back from that trigger. Its update
    /// (`FUN_005ae440`) reacts to the drive controls (event bits 0x40000/0x80000/0x100000).
    VehicleControl {
        0 => None "none",
        /// Mode 1: rocks the body while driving and levels it in the air (ground vehicles:
        /// cars, excavators, motorcycles).
        1 => Ground "ground",
        /// Mode 2 (3 behaves the same): stops the spin and steers the body level (aircraft,
        /// jet packs, magic carpet).
        2 => Air "air",
    }
);

impl VehicleControl {
    pub fn is_none(&self) -> bool {
        *self == VehicleControl::None
    }
}

#[allow(clippy::derivable_impls)]
impl Default for VehicleControl {
    fn default() -> Self {
        VehicleControl::None
    }
}

enum8!(
    /// How this object's size must compare with another's for one to equip or ride the other
    /// (`+0x28c`, checked by `FUN_006aa9a0` from `FUN_006ab310`). "Size" is the growth the
    /// objects' `resize` adjective modifiers add per equip slot (`FUN_006aa560`).
    SizeFit {
        /// Growth must be identical.
        0 => MustMatch "must_match",
        /// This object may not be grown more than the other (fails when other < this; bra,
        /// bib). Formerly `at_least`.
        1 => NotLarger "not_larger",
        /// This object may not be grown less than the other (fails when other > this;
        /// minecart, bathtub, bunker). Formerly `at_most`.
        2 => NotSmaller "not_smaller",
        /// No check.
        3 => Any "any",
    }
);

/// Section 0x0A.
///
/// ```text
/// 0x0A; u8 flags (bits 5-7 size_fit); u8 body_flags (bits 0-3 vehicle_control);
/// u8 rope_segments; u8 burst_count
/// [flags bit4]  RefList spawn_list
/// [body_flags bit5] i32 reflectivity, i32 sky_reflection   (20.12)
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct General {
    pub flags: GeneralFlags,
    /// Flags bits 5-7 (`+0x28c`; `any` on almost every object, `must_match` on clothing).
    pub size_fit: SizeFit,
    /// `body_flags` bits 0-3.
    #[serde(default, skip_serializing_if = "VehicleControl::is_none")]
    pub vehicle_control: VehicleControl,
    pub body_flags: GeneralFlags2,
    /// Number of rope segments (`+0x1f4`; the engine's writer stores the rope's segment list
    /// length - 1; non-zero only on ropes, chains, cables).
    #[serde(default, skip_serializing_if = "crate::util::is_zero_u8")]
    pub rope_segments: u8,
    /// Shots per burst (`+0x278`/`+0x277`; adjective property 0x06 `burst_count`:
    /// `automatic` = 9, `singlefire` = 1; 3 on most objects).
    pub burst_count: u8,
    /// Objects spawned with this one (trees, buildings; `FUN_00675350` into `+0x654`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_list: Option<RefList>,
    /// Default material `[reflectivity, sky_reflection]` (`+0x2ac`/`+0x2b0`, copied to the
    /// renderer at `+0x68`/`+0x6c` as floats; shader uniforms `uReflectivity`/
    /// `uSkyReflectionT`, `FUN_004737e0`). Unused by shipped files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reflection: Option<[Fx12; 2]>,
}

flags8!(
    /// Movement abilities. `walk`, `swim`, `dive`, `fly` and `climb` are the object editor's
    /// MOVEMENT PROPERTIES (`static_objectproperties` 27-31: FLY, WALK, SWIM, CLIMB, DIVE) and
    /// what the `movement` adjective modifier sets (`walking` adds walk, `aquatic` swim+dive,
    /// `flying` fly, `climbing` climb); bit 3 is tested as "can fly" (`+0x5fc & 8`,
    /// `FUN_006986d0` area). `glide` (bit 4: umbrellas, hang glider, flying squirrel) and
    /// `hover` (bit 5: 548 held weapons and tools) are named from the objects that carry them.
    MovementFlags {
        0 => "walk",
        1 => "swim",
        2 => "dive",
        3 => "fly",
        4 => "glide",
        5 => "hover",
        6 => "climb",
    }
);

flags8!(
    /// Section 0x0D flags byte (bits 4-5 = [`Locomotion`], kept separately). Mapping from
    /// `FUN_006bbd70`, confirmed by the writer `FUN_006b1d00`.
    MovementBits {
        /// `+0x68c` bit 1 (adjective property 0x3a `equip_jump`): gives the wearer its jump
        /// (`FUN_00690700`; springshoes, pogostick, `bouncy`).
        0 => "equip_jump",
        /// `+0x68c` bit 4 (adjective property 0x13 `rider_flight`): as a mount it can fly when
        /// its rider can (`FUN_00693a00`; most animals).
        1 => "rider_flight",
        /// Not read by the loader (it takes bit 1 only); the writer fills it from `+0x68d`
        /// bit 7, which no code sets or tests. Set on 14 aircraft.
        2 => "unused_bit2",
        /// `+0x68c` bit 2 (adjective property 0x3b `equip_speed`): gives the wearer its speed
        /// (shoes, fins, wings).
        3 => "equip_speed",
        /// `+0x68c` bit 3: passes equipment movement on to its wearer (clothing).
        6 => "relays_equipment_movement",
        /// `+0x620` (adjective property 0x3e `animated`; `frozen`/`petrified` clear it).
        7 => "animated",
    }
);

enum8!(
    /// How the object moves on the ground (`+0x61c`; the walk controller chosen by
    /// `FUN_00698bf0` case 1: 1 and 2 build `FUN_006954e0(obj, 1|2)`, 0 and 3 `FUN_00695420`).
    /// Names from the objects using each value.
    Locomotion {
        0 => Walk "walk",
        /// Hops, kept upright (most inanimate objects).
        1 => Hop "hop",
        /// Balls, rings.
        2 => Roll "roll",
        /// Carts, roads.
        3 => Slide "slide",
    }
);

/// Section 0x0D: movement.
///
/// ```text
/// 0x0D; u16 speed; u8 jump; u8 flags (bits 4-5 locomotion); u8 equip_movement; u8 abilities;
/// u8 ai_movement
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Movement {
    /// Movement speed (x0.6 (`DAT_00831fc0`) into `FUN_00691e50`, which clamps it to 150; 30
    /// for most objects, 35 humans, 100 vehicles).
    pub speed: u16,
    /// Jump strength (x1.5 into `FUN_0068f390`; the player always gets 0x30).
    pub jump: u8,
    pub flags: MovementBits,
    /// Flags bits 4-5 (`+0x61c`).
    #[serde(default, skip_serializing_if = "Locomotion::is_walk")]
    pub locomotion: Locomotion,
    /// Movement granted to the wearer when equipped (shoes: walk, wings: fly;
    /// `+0x5f4`/`+0x600`).
    pub equip_movement: MovementFlags,
    /// Movement abilities (`+0x5fc`/`+0x608`).
    pub abilities: MovementFlags,
    /// Movement the AI uses (`+0x5f8`/`+0x604`).
    pub ai_movement: MovementFlags,
}

flags8!(
    /// Section 0x01 flags (`FUN_006bbd70` stores one byte per bit; `FUN_006b1d00` writes them
    /// back).
    StatsFlags {
        /// `+0x3fc`: the object has an AI brain (adjective property 0x08 `animate`; set by every
        /// phobia, cleared by `_dead`; with it the loader also builds a character body).
        0 => "animate",
        /// `+0x400`: digs (the AI picks dig task 0x13, `FUN_00712800`; dogs, bunnies).
        1 => "digger",
        /// `+0x401`: no reader besides the loader and the writer `FUN_006b1d00` (mammoth,
        /// elephant, woodpecker, beaver, redwood).
        2 => "unused_401",
        /// `+0x402`: no reader besides the loader and the writer (cockatrice, dragon, gorgon).
        3 => "unused_402",
        /// `+0x40e`: only copied into the saved object state (flag 0x2000, `FUN_006c84a0`,
        /// restored by `FUN_00686760`) and written back; nothing branches on it (only the
        /// car). Formerly `unknown_40e`.
        4 => "unused_40e",
    }
);

/// Section 0x01: senses and combat.
///
/// ```text
/// 0x01; u8 unused; u8 flags; u8 sight_range; u8 visibility; u8 aggression; u8 attack_damage
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    /// Byte after the marker: `FUN_006bbd70` skips it and the writer `FUN_006b1d00` always
    /// writes 1 (2 on carnivorous fish, 0 on four files). Omitted when 1.
    #[serde(default = "one_u8", skip_serializing_if = "is_one_u8")]
    pub unused: u8,
    pub flags: StatsFlags,
    /// How far the object sees (`+0x3d1`; squared x256 into `+0x3d4`). `blind` = 0,
    /// `nearsighted` = 5, `farsighted` = 14; 8 default, 11 humans.
    pub sight_range: u8,
    /// Visibility percent (`+0x3d8` = value << 12 / 100; the writer converts back): `invisible` = 0, `camouflage` = 10, `ninja` = 60.
    pub visibility: u8,
    /// Temperament (`+0x3c0`): `fearless` = 1, `peaceful`/`scared` = 2, teams = 3.
    pub aggression: u8,
    /// Attack damage (`+0x3d0`): `harmless` = 0, `painful` +10, `mighty` x2.
    pub attack_damage: u8,
}

flags8!(
    /// Container flags (container component at `+0x17c`, flag byte `+0x1a4`; mapping from
    /// `FUN_006bbd70`, written back by `FUN_006b1d00`).
    ContainerFlags {
        /// `+0x1a4` bit 1: no lid (the object editor's HAS LID checkbox,
        /// `menu_main.scriptingcontainerb.HasLid`, shows its inverse, `FUN_005830d0`;
        /// `container_nolid_*` objects).
        0 => "open_top",
        /// `+0x1a4` bit 2 (`FUN_0066e0e0`): contents stay in the world and are drawn and
        /// updated inside the container (`FUN_0066ede0`, `FUN_00671e70`; cages, ice blocks,
        /// jars; editor text SEE THROUGH).
        1 => "see_through",
        /// `+0x1a4` bit 6: with `see_through`, contents keep their own position inside the
        /// container (`FUN_00671e70`/`FUN_00671640` place them via `FUN_0066e9d0` instead of
        /// at the centre; cages, pig pen, tornado). Formerly `locked`: locking is the `lock`
        /// action's key list (`FUN_00670b60`), not this bit.
        2 => "contents_keep_position",
        /// `+0x1a4` bit 0: `fire_projectile` fires the contents instead of spawning new
        /// projectiles (`FUN_00545270`; artillery).
        3 => "launcher",
    }
);

/// An object inside a container.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContainedObject {
    /// The `.so` (u16 pmindex index; resolved to a gender variant by `FUN_006c05f0`).
    pub object: ResRef,
    /// Display word of the spawned object (stored at the new object's `+0x162`, a `.dtm` word
    /// key; present when the container's `with_name_words` flag is set; the engine's writer
    /// always sets it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_word: Option<u16>,
}

/// Section 0x02: container.
///
/// ```text
/// 0x02; u8 flags (bit4 = contents carry `name_word`); u8 width; u8 height;
/// u8 n; n x (u16 object [bit4: u16 name_word])
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Container {
    pub flags: ContainerFlags,
    /// Flag bit 4: every content entry has `name_word`.
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub with_name_words: bool,
    /// Interior size (`+0x1a2`/`+0x1a3` via `FUN_0066ead0`; 0 = not a container). The object
    /// editor shows max(width, height) as `static_containersize` NONE / SMALL (< 32) / MEDIUM
    /// (< 60) / LARGE (`FUN_005830d0`).
    pub width: u8,
    pub height: u8,
    /// Objects the container starts with (spawned by the loader only with spawn flag 0x02).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents: Vec<ContainedObject>,
}

flags8!(
    /// Electrical flags (`+0x1d8`/`+0x1d9`; bits 0-1 = [`ShockMode`], kept separately; bits 2-3
    /// have no reader in `FUN_006bbd70`).
    ElectricalFlags {
        /// `+0x1d9` bit 0 (adjective property 0x3f `chargeable`): electrifying it fires its
        /// charge event (`FUN_0066a780` calls `FUN_006a7c70`).
        4 => "chargeable",
        /// `+0x1d8` bit 7 (adjective property 0x1f `ignores_shock`): skips the shock reaction
        /// (`FUN_00484ae0`; vehicles, generators).
        5 => "ignores_shock",
        /// `+0x1d8` bit 5 (adjective property 0x20 `shocking`): a charged object with it
        /// shocks what it touches (`FUN_006a2af0` sets the other's `+0x1d8` bit 6).
        6 => "shocking",
        /// `+0x1d8` bit 2 (adjective property 0x1d `conductive`; `metal` sets it): passes a
        /// charge on to attached objects (`FUN_0066a780`).
        7 => "conductive",
    }
);

enum8!(
    /// How electricity switches the object (`+0x1d8` bits 3-4; adjective property 0x1e).
    ShockMode {
        /// Not affected.
        0 => None "none",
        /// Electrifying it activates it (`FUN_006a7c70`, `FUN_0066a780`: `(+0x1d8 & 0x18) ==
        /// 8` sets `+0x270` bit 3) and an EMP switches it off (vehicles, lamps).
        1 => PoweredByShock "powered_by_shock",
        /// Only switched off by an EMP (outlets, gadgets, appliances).
        2 => Electronic "electronic",
    }
);

impl ShockMode {
    pub fn is_none(&self) -> bool {
        *self == ShockMode::None
    }
}

#[allow(clippy::derivable_impls)]
impl Default for ShockMode {
    fn default() -> Self {
        ShockMode::None
    }
}

/// Section 0x03: electricity.
///
/// ```text
/// 0x03; u8 powered; u8 flags
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Electrical {
    /// Non-zero = powered (`+0x1d8` bit 0 = value != 0, adjective property 0x1c; a few
    /// objects store 50 or 100, which the engine treats as 1).
    pub powered: u8,
    /// Flags bits 0-1. The EMP action (class vtable `0x008339b8`, created for the `emp` and
    /// `empgrenade` objects by `FUN_004b5b90`) runs `FUN_00541aa0`, which calls
    /// `FUN_006a00a0` (switch off, `+0x1d9` bit 1) on objects in range whose mode is 1 or 2.
    #[serde(default, skip_serializing_if = "ShockMode::is_none")]
    pub shock_mode: ShockMode,
    pub flags: ElectricalFlags,
}

flags8!(
    /// Section 0x04 flags (`+0x245`/`+0x246` layer state; bits 6-7 have no reader).
    LayerFlags {
        /// `+0x245` bit 2, draw layer 2 (`+0x246` = 2): the sky layer. The object editor's
        /// writer `FUN_006b1610` saves it as the `.odt` `sky_object` flag (text SKY OBJECT,
        /// `static_objectproperties` 24): the 56 shipped objects with it are exactly those
        /// the `.odt` marks `sky_object` (stars, clouds, zodiac). Such objects get no mass (`FUN_006bbd70`), cannot be stuck to
        /// (`FUN_006afcc0`) and count as scenery (`FUN_006b12c0`). Formerly `foreground`.
        0 => "sky_object",
        /// `+0x245` bit 3, draw layer 3 (adjective property 0x23; `_background`, `immovable`).
        1 => "background",
        /// `+0x245` bit 1 (adjective property 0x24 `reversible`): can be flipped by the player
        /// (`FUN_005e7b60`).
        2 => "reversible",
        /// `+0x245` bit 0: the player can rotate it in 90-degree steps while holding it
        /// (`FUN_005e7b60`, which also needs a body free to rotate, see
        /// [`Physical::rotates`]); a wide object without it flies lying flat (fly controller
        /// `FUN_006986d0`/`FUN_00698860`). Set on items, never on creatures, vehicles or
        /// buildings. Formerly `carry_upright`.
        3 => "rotatable",
        /// `+0x24d` (adjective property 0x26 `grabbable`: `wieldy` sets it).
        4 => "grabbable",
        /// `+0x250`: turns itself upright every tick (`FUN_006c3a50`; buildings, doors).
        5 => "self_righting",
    }
);

/// Section 0x04.
///
/// ```text
/// 0x04; u8 solidity; u8 flags
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    /// `+0x244` (adjective property 0x21 `solidity`: `solid`, `stone`, `ice` = 0; 1 on large
    /// scenery, which `FUN_006b12c0` counts as scenery; 2 on liquids and powders; ropes do
    /// not stick to 3, `FUN_006afcc0`).
    pub solidity: u8,
    pub flags: LayerFlags,
}

enum8!(
    /// Object material (`+0x1e4`/`+0x1e8`, set through `FUN_006c33b0`; adjective property
    /// 0x28). Values 0-12 are the object editor's MATERIAL list in order (`static_materials`:
    /// PLANT, ANIMAL, FOOD, WOOD, EARTH, STONE, FIRE, WATER, METAL, GAS, PLASTIC, FABRIC,
    /// GLASS; also `static_objectproperties` 63-75). Code checks e.g. `fire` (6) in
    /// `FUN_006c33f0` (fire cannot hurt it) and `animal` (1) in `FUN_006afcc0`.
    Material {
        0 => Plant "plant",
        1 => Animal "animal",
        2 => Food "food",
        3 => Wood "wood",
        4 => Earth "earth",
        5 => Stone "stone",
        6 => Fire "fire",
        7 => Water "water",
        8 => Metal "metal",
        9 => Gas "gas",
        10 => Plastic "plastic",
        11 => Fabric "fabric",
        12 => Glass "glass",
        /// Not in the editor list; only the starite.
        13 => Special "special",
    }
);

enum8!(
    /// What fire does to the object (`+0x1f0`, adjective property 0x2c). The object editor's
    /// FIRE INTERACTION / EXPLOSION SIZE pickers (`FUN_0058d010`, `FUN_0058d1c0`,
    /// `FUN_0058d2a0`, `FUN_0058cd80`; texts `static_fireinteraction` NO DAMAGE / DAMAGE /
    /// EXPLODE and `static_explosionsize` HARMLESS / SMALL / NORMAL / LARGE / NUCLEAR) write
    /// these values, and `FUN_006c3410` turns the explosive ones into the explosion size of the
    /// `explode` action (`static_action_explosiontype`: SMALL, NORMAL, BIG, AUTO, SUPER,
    /// HARMLESS). The old names came from the adjectives that set each value.
    Combustion {
        /// Burns and takes fire damage unless its material is `fire` (`FUN_006c33f0`,
        /// `FUN_004d6670`; editor DAMAGE or NO DAMAGE). Formerly `normal` (unchanged).
        0 => Normal "normal",
        /// Fire does no damage (`FUN_006c33f0`; editor NO DAMAGE; `brick`, `brass`,
        /// `_invincible`).
        1 => Fireproof "fireproof",
        /// Explodes, SMALL explosion (hair spray, soap, nail polish). Formerly `sulfurous`.
        2 => ExplodesSmall "explodes_small",
        /// Explodes, NORMAL explosion (rockets, mines, oil barrels). Formerly `reactive`.
        3 => ExplodesNormal "explodes_normal",
        /// Explodes, BIG explosion (editor LARGE; gasoline, gunpowder). Formerly `fuel`.
        4 => ExplodesBig "explodes_big",
        /// Explodes, AUTO size: the editor picks SMALL / NORMAL / LARGE from the object's
        /// size (`FUN_0058cd80`; the editor's EXPLODE choice writes 5; grenades, munitions).
        /// Formerly `explosive`.
        5 => ExplodesAuto "explodes_auto",
        /// Burns and takes fire damage even when its material is `fire` (editor DAMAGE writes
        /// 6; adjective `immolated`). Formerly `immolated`.
        6 => Burns "burns",
        /// Explodes, SUPER explosion (editor NUCLEAR; the nuke). Formerly `nuclear`.
        7 => ExplodesSuper "explodes_super",
        /// Explodes harmlessly (editor HARMLESS).
        8 => ExplodesHarmless "explodes_harmless",
    }
);

flags8!(
    /// Section 0x05 flags (bit 5 = `unused_4b` present, kept separately; bit 7 has no reader).
    PhysicalFlags {
        /// `+0x209` (adjective property 0x2e `waterproof`); sets physics body flag 0x2000.
        0 => "waterproof",
        /// Stored inverted at `+0x273` bit 0: sticky things cannot attach (`FUN_006afcc0`).
        1 => "non_stick",
        /// Tied to its holder by a rope joint (component `+0x774`, `FUN_00669720`; kites,
        /// balloons; adjective property 0x47 `tethered`).
        2 => "tethered",
        /// `+0x77d` (adjective property 0x48 `lighter_than_air`; `balloon` sets it).
        3 => "lighter_than_air",
        /// `+0x20d`: points along its velocity while flying (`FUN_006c5100`; arrows, bombs).
        4 => "aligns_to_velocity",
        /// `+0x202` (adjective property 0x4c `blast_proof`; the bomb suit, bomb shelter).
        6 => "blast_proof",
    }
);

enum8!(
    /// How this object sticks to others (`+0x1ec`, checked by `FUN_006afcc0`, which also
    /// needs the other object's `non_stick` bit clear).
    StickMode {
        /// Does not stick (`FUN_006afcc0` returns false for 0). Formerly `normal`.
        0 => None "none",
        /// Sticks, but not to `animal` material (letters).
        1 => Letter "letter",
        /// Sticks (stickers, putty, gum, tar).
        2 => Sticky "sticky",
        /// Sticks, but not to `solidity` 3 objects (whips, cables, hoses).
        3 => Rope "rope",
    }
);

/// Section 0x05: physical properties.
///
/// ```text
/// 0x05; u8 stick_mode; u8 material; u8 weight; i16 health; u8 rotates; u8 combustion;
/// u8 buoyancy; u8 flags; [flags bit5] u16 unused_4b
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Physical {
    #[serde(default, skip_serializing_if = "StickMode::is_none")]
    pub stick_mode: StickMode,
    pub material: Material,
    /// Weight: `FUN_006bbd70` turns it into the body mass through `FUN_006c3600` (feather 1,
    /// human 50, cow 65, boulder 80); forced to 0 when the object has no shapes or is a
    /// `sky_object`.
    pub weight: u8,
    /// Hit points (`+0x1f8` current / `+0x1fc` maximum; adjective property 0x2a).
    pub health: i16,
    /// 0 locks the physics body's rotation: after building the body `FUN_006bbd70` clears its
    /// inverse inertia (body `+0x68` = 0, the value `FUN_005caec0` keeps at 0) and `+0x273`
    /// bit 2 (0x006bd264). 0 on creatures, which stay upright; 1 on inanimate objects. Forced
    /// to 0 with `weight`. Formerly `rigid`.
    pub rotates: u8,
    pub combustion: Combustion,
    /// Buoyancy (`FUN_006c3530`, `+0x208`): metal/stone 0, neutral 50, wood 55-70,
    /// humans 80, balloons 90.
    pub buoyancy: u8,
    pub flags: PhysicalFlags,
    /// `+0x200` (u16; adjective property 0x4b): read only by the loader, the writer
    /// `FUN_006b1d00` and the adjective property table `FUN_00621990`. Absent in shipped files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_4b: Option<u16>,
}

/// Section 0x06: temperatures (each stored << 12 by `FUN_006bbd70`).
///
/// ```text
/// 0x06; u8 temperature; u8 cold_temperature; u8 hot_temperature
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Thermal {
    /// Own temperature (`+0x25c` base / `+0x260` current, the value `set_temperature` writes):
    /// 50 normal, 20 ice, 100-200 fire.
    pub temperature: u8,
    /// `+0x264`: the `on_become_cold` trigger fires when the current temperature drops below
    /// it, `on_become_warm` when it climbs back (temperature trigger check at `0x005aade0`,
    /// mode `+0x30` 0/1). 30 default. Formerly `freeze_temperature`.
    pub cold_temperature: u8,
    /// `+0x268`: `on_become_hot` fires when the temperature rises to it (mode 2),
    /// `on_become_warm` when it falls back. 180 default, 100 food, 240 vehicles, 255 never.
    /// Formerly `burn_temperature`.
    pub hot_temperature: u8,
}

/// Sound slots (`+0x90c + slot*12`, played through `FUN_0069d770(slot, flags)`). With no
/// sound set, slot 2 falls back to `interaction_consume.wav`, 7 to `interaction_steal.wav`
/// and 16 to `interaction_mount.wav` (`FUN_0069d770`); slot 11 is the only one played with
/// the looping flag 0x200. The other names follow the AI and object events whose handlers
/// call `FUN_0069d770` with each constant, and the sounds shipped in each slot.
pub const SOUND_SLOTS: &[(u8, &str)] = &[
    (0, "angry"), (1, "death"), (2, "eat"), (3, "investigate"), (4, "interact"), (5, "scared"), (6, "ambient"), (7, "steal"),
    (8, "satisfied"), (9, "sick"), (10, "sleep"), (11, "loop"), (12, "shoot"), (13, "attack"), (14, "activate"), (15, "deactivate"),
    (16, "mount"), (17, "dismount"), (18, "bounce"),
];

enum8!(
    /// When a sound plays (see [`SOUND_SLOTS`]).
    SoundSlot {
        0 => Angry "angry", 1 => Death "death", 2 => Eat "eat", 3 => Investigate "investigate", 4 => Interact "interact",
        5 => Scared "scared", 6 => Ambient "ambient", 7 => Steal "steal", 8 => Satisfied "satisfied", 9 => Sick "sick",
        10 => Sleep "sleep", 11 => Loop "loop", 12 => Shoot "shoot", 13 => Attack "attack", 14 => Activate "activate",
        15 => Deactivate "deactivate", 16 => Mount "mount", 17 => Dismount "dismount", 18 => Bounce "bounce",
    }
);

/// Sound table entry (section 0x0B).
///
/// ```text
/// u8 slot; u32 sound; u8 unused
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SoundEntry {
    pub slot: SoundSlot,
    /// The `.wav` (`null` = none).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<ResRef>,
    /// Stored at `+0x910 + slot*12` by `FUN_006bbd70`, but only copied by the object editor
    /// (`FUN_00586580`) and written back by `FUN_006b1d00`; playback (`FUN_0069d770`,
    /// `FUN_0069d670`) never reads it. 1 in every file; omitted when 1.
    #[serde(default = "one_u8", skip_serializing_if = "is_one_u8")]
    pub unused: u8,
}

fn one_u8() -> u8 {
    1
}

fn is_one_u8(v: &u8) -> bool {
    *v == 1
}

/// One entry of the animation table.
///
/// ```text
/// u8 slot; u32 anim; u8 has_events; [has_events] u16 events[3]
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationEntry {
    /// Animation slot (see `fmt_anim::slots`).
    pub slot: AnimSlot,
    /// The `.anim` resource.
    pub anim: Option<ResRef>,
    /// Event frames in file order `[action, aim_up, aim_down]`, 0xFFFF = unset. They copy the
    /// `.anim`'s `action`/`aim_up`/`aim_down` event frames (true for 966 of 991 entries with
    /// aim frames). `FUN_006677a0` passes them to `FUN_00665c00`, which stores them in `.anim`
    /// event-kind order `[action, aim_down, aim_up]` (kinds 0, 1, 2).
    /// `action` is the frame the animation's effect happens on (read through `FUN_00665d20`
    /// index 0 by `FUN_0053f710` for the current animation and for `fiddle`). The other two
    /// are the frames `fire_projectile` interpolates towards for a target below / above
    /// (slot 27 `shoot`: index 1 for a positive aim angle, index 2 for a negative one); which
    /// of them is "down" depends on the world's y direction and is not confirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<[u16; 3]>,
}

/// Animation slot number, printed with its canonical animation name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimSlot(pub u8);

impl Serialize for AnimSlot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match fmt_anim::slots::SLOT_NAMES.get(self.0 as usize).copied().flatten() {
            Some(n) => s.serialize_str(n),
            None => s.serialize_u8(self.0),
        }
    }
}

impl<'de> Deserialize<'de> for AnimSlot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            N(u8),
            S(String),
        }
        match Repr::deserialize(d)? {
            Repr::N(n) => Ok(AnimSlot(n)),
            Repr::S(s) => fmt_anim::slots::SLOT_NAMES
                .iter()
                .position(|n| *n == Some(s.as_str()))
                .map(|i| AnimSlot(i as u8))
                .ok_or_else(|| serde::de::Error::custom(format!("unknown animation slot {s:?}"))),
        }
    }
}

/// Section 0x09 (`FUN_006677a0`, called by `FUN_006bbd70`, which then starts slot 14
/// `idle`).
///
/// ```text
/// 0x09; u8 flags (bit0, bit1, bit2: which durations follow); u8 n; n x AnimationEntry;
/// [bit0] u16 attack_duration; [bit1] u16 unused_swim_attack_duration;
/// [bit2] u16 unused_swim_attack2_duration
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationTable {
    pub entries: Vec<AnimationEntry>,
    /// Length of the attack in frames (`+8` of the animation component, the only duration
    /// `FUN_006677a0` stores; `FUN_0065cc30`; the writer saves it from `+0x4f8`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attack_duration: Option<u16>,
    /// Flag bit 1: skipped by `FUN_006677a0` (`*pos += 2`); the writer `FUN_006b1d00` stores
    /// 0. 60 in shipped files next to the `swimairattack` slots it was presumably meant for.
    /// Formerly `swim_air_attack_duration`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_swim_attack_duration: Option<u16>,
    /// Flag bit 2: skipped like bit 1 (60 in shipped files). Formerly
    /// `swim_air_attack2_duration`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_swim_attack2_duration: Option<u16>,
    /// Flag bits 3-7: no reader in `FUN_006677a0` (zero in shipped files; the engine's writer
    /// writes the flag byte as 0xFF). JSON `unused_flags`.
    #[serde(default, rename = "unused_flags", skip_serializing_if = "crate::util::is_zero_u8")]
    pub extra_flags: u8,
}

// ---------------------------------------------------------------------------------------------

fn read_counters(r: &mut Reader, main: bool) -> Result<(BudgetCounters, [u8; 4])> {
    // main: cost, instances, a(u16), b, c, memory_a, memory_b, flag, d, groups
    // sub:  a(u16), c, memory_a, memory_b, d, groups
    let mut ex = [0u8; 4];
    if main {
        ex[0] = r.u8()?;
        ex[1] = r.u8()?;
    }
    let node_count = r.u16()?;
    if main {
        ex[2] = r.u8()?;
    }
    let jointed_part_count = r.u8()?;
    let vector_memory = r.u32()?;
    let unused_memory = r.u32()?;
    if main {
        ex[3] = r.u8()?;
    }
    let relation_count = r.u8()?;
    let trigger_stats = TriggerStats { collision: r.array()?, proximity: r.array()?, unused: r.array()?, other: r.array()? };
    Ok((BudgetCounters { node_count, jointed_part_count, vector_memory, unused_memory, relation_count, trigger_stats }, ex))
}

fn write_counters(c: &BudgetCounters, w: &mut Writer, main: Option<&Budget>) {
    if let Some(b) = main {
        w.u8(b.budget_cost).u8(b.object_count);
    }
    w.u16(c.node_count);
    if let Some(b) = main {
        w.u8(b.scenery);
    }
    w.u8(c.jointed_part_count).u32(c.vector_memory).u32(c.unused_memory);
    if let Some(b) = main {
        w.u8(b.has_brain);
    }
    w.u8(c.relation_count);
    let t = &c.trigger_stats;
    for g in [&t.collision, &t.proximity, &t.unused, &t.other] {
        w.bytes(g);
    }
}

impl Budget {
    fn read(r: &mut Reader) -> Result<Self> {
        let (counters, ex) = read_counters(r, true)?;
        let mut subs = [None, None, None];
        for s in &mut subs {
            let count = r.u8()?;
            if count != 0 {
                *s = Some(SubBudget { count, counters: read_counters(r, false)?.0 });
            }
        }
        let [attached, equipment, contents] = subs;
        Ok(Budget { budget_cost: ex[0], object_count: ex[1], scenery: ex[2], has_brain: ex[3], counters, attached, equipment, contents })
    }
    fn write(&self, w: &mut Writer) -> Result<()> {
        write_counters(&self.counters, w, Some(self));
        for s in [&self.attached, &self.equipment, &self.contents] {
            match s {
                Some(s) => {
                    ensure!(s.count != 0, "sub-budget count must be non-zero (omit it instead)");
                    w.u8(s.count);
                    write_counters(&s.counters, w, None);
                }
                None => {
                    w.u8(0);
                }
            }
        }
        Ok(())
    }
}

fn read_relations(r: &mut Reader, ctx: &Context) -> Result<Vec<Relation>> {
    let n = r.u8()?;
    (0..n).map(|_| Relation::read(r, ctx)).collect()
}

fn write_relations(list: &[Relation], w: &mut Writer, ctx: &Context) -> Result<()> {
    count8(w, list.len(), "relations")?;
    for x in list {
        x.write(w, ctx)?;
    }
    Ok(())
}

impl Body {
    /// Read a body starting at absolute offset `start`.
    pub fn read(data: &[u8], start: usize, ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        r.seek(start)?;
        let deps_offset = r.u32()? as usize;
        let tags_offset = r.u32()? as usize;
        let budget = Budget::read(&mut r)?;
        let width = r.u16()?;
        let height = r.u16()?;

        marker(&mut r, 0x0a)?;
        let f1 = r.u8()?;
        let f2 = r.u8()?;
        let rope_segments = r.u8()?;
        let burst_count = r.u8()?;
        let spawn_list = if f1 & 0x10 != 0 { Some(RefList::read(&mut r, ctx)?) } else { None };
        let reflection = if f2 & 0x20 != 0 { Some([Fx12(r.i32()?), Fx12(r.i32()?)]) } else { None };
        let general = General {
            flags: GeneralFlags(f1 & 0x0f),
            size_fit: SizeFit::from_u8(f1 >> 5),
            vehicle_control: VehicleControl::from_u8(f2 & 0x0f),
            body_flags: GeneralFlags2(f2 & 0xd0),
            rope_segments,
            burst_count,
            spawn_list,
            reflection,
        };

        marker(&mut r, 0x0d)?;
        let speed = r.u16()?;
        let jump = r.u8()?;
        let f3 = r.u8()?;
        let movement = Movement {
            speed,
            jump,
            flags: MovementBits(f3 & !0x30),
            locomotion: Locomotion::from_u8((f3 >> 4) & 3),
            equip_movement: MovementFlags(r.u8()?),
            abilities: MovementFlags(r.u8()?),
            ai_movement: MovementFlags(r.u8()?),
        };

        marker(&mut r, 0x01)?;
        let stats = Stats {
            unused: r.u8()?,
            flags: StatsFlags(r.u8()?),
            sight_range: r.u8()?,
            visibility: r.u8()?,
            aggression: r.u8()?,
            attack_damage: r.u8()?,
        };
        let relations = read_relations(&mut r, ctx)?;
        let reverse_relations = read_relations(&mut r, ctx)?;

        marker(&mut r, 0x02)?;
        let f5 = r.u8()?;
        let cw = r.u8()?;
        let ch = r.u8()?;
        let n = r.u8()?;
        let with_name_words = f5 & 0x10 != 0;
        let contents = (0..n)
            .map(|_| {
                let object = ResRef::from_index(r.u16()? as u32, ctx);
                Ok(ContainedObject { object, name_word: if with_name_words { Some(r.u16()?) } else { None } })
            })
            .collect::<Result<_>>()?;
        let container = Container { flags: ContainerFlags(f5 & !0x10), with_name_words, width: cw, height: ch, contents };

        marker(&mut r, 0x03)?;
        let powered = r.u8()?;
        let ef = r.u8()?;
        let electrical = Electrical { powered, shock_mode: ShockMode::from_u8(ef & 3), flags: ElectricalFlags(ef & !3) };

        marker(&mut r, 0x04)?;
        let layer = Layer { solidity: r.u8()?, flags: LayerFlags(r.u8()?) };

        marker(&mut r, 0x05)?;
        let stick_mode = StickMode::from_u8(r.u8()?);
        let material = Material::from_u8(r.u8()?);
        let weight = r.u8()?;
        let health = r.i16()?;
        let rotates = r.u8()?;
        let combustion = Combustion::from_u8(r.u8()?);
        let buoyancy = r.u8()?;
        let f7 = r.u8()?;
        let unused_4b = if f7 & 0x20 != 0 { Some(r.u16()?) } else { None };
        let physical = Physical {
            stick_mode,
            material,
            weight,
            health,
            rotates,
            combustion,
            buoyancy,
            flags: PhysicalFlags(f7 & !0x20),
            unused_4b,
        };

        marker(&mut r, 0x06)?;
        let thermal = Thermal { temperature: r.u8()?, cold_temperature: r.u8()?, hot_temperature: r.u8()? };

        marker(&mut r, 0x07)?;
        let len = r.u16()? as usize;
        let beh_start = r.pos();
        let behaviours = read_behaviours(&mut r, ctx)?;
        ensure!(r.pos() - beh_start == len, "behaviour list length {len} does not match parsed {}", r.pos() - beh_start);

        marker(&mut r, 0x08)?;
        let character_body = r.u8()?;
        let physics_bodies = r.u8()?;
        let root = Node::read(&mut r, ctx, Flavor::Object, None)?;

        marker(&mut r, 0x0b)?;
        let n = r.u8()?;
        let sounds = (0..n)
            .map(|_| {
                let slot = SoundSlot::from_u8(r.u8()?);
                let sound = crate::util::res32(r.u32()?, ctx);
                Ok(SoundEntry { slot, sound, unused: r.u8()? })
            })
            .collect::<Result<_>>()?;

        let mut m = r.u8()?;
        let animations = if m == 0x09 {
            let f = r.u8()?;
            let n = r.u8()?;
            let entries = (0..n)
                .map(|_| {
                    let slot = AnimSlot(r.u8()?);
                    let anim = crate::util::res32(r.u32()?, ctx);
                    let events = if r.bool()? { Some([r.u16()?, r.u16()?, r.u16()?]) } else { None };
                    Ok(AnimationEntry { slot, anim, events })
                })
                .collect::<Result<_>>()?;
            let attack_duration = if f & 1 != 0 { Some(r.u16()?) } else { None };
            let unused_swim_attack_duration = if f & 2 != 0 { Some(r.u16()?) } else { None };
            let unused_swim_attack2_duration = if f & 4 != 0 { Some(r.u16()?) } else { None };
            m = r.u8()?;
            Some(AnimationTable { entries, attack_duration, unused_swim_attack_duration, unused_swim_attack2_duration, extra_flags: f & 0xf8 })
        } else {
            None
        };
        ensure!(r.pos() - 1 == tags_offset, "tags offset {tags_offset:#x} does not match position {:#x}", r.pos() - 1);
        if m != 0x0e {
            bail!("expected section marker 0x0e at {:#x}, found {m:#04x}", r.pos() - 1);
        }
        let tags = crate::util::read_tags(&mut r, ctx)?;
        ensure!(r.pos() == deps_offset, "dependency offset {deps_offset:#x} does not match position {:#x}", r.pos());
        let dependencies = deps::read(&mut r, ctx)?;

        Ok(Body {
            budget,
            width,
            height,
            general,
            movement,
            stats,
            relations,
            reverse_relations,
            container,
            electrical,
            layer,
            physical,
            thermal,
            behaviours,
            character_body,
            physics_bodies,
            root,
            sounds,
            animations,
            tags,
            dependencies,
        })
    }

    /// Size in bytes this body occupies when written at the end of `r`.
    fn end(data: &[u8], start: usize) -> Result<usize> {
        let mut r = Reader::new(data);
        r.seek(start)?;
        let deps = r.u32()? as usize;
        r.seek(deps)?;
        let n = r.u32()? as usize;
        Ok(deps + 4 + 5 * n)
    }

    /// Append this body to `w` (offsets are absolute within `w`).
    pub fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        let start = w.pos();
        w.u32(0).u32(0);
        self.budget.write(w)?;
        w.u16(self.width).u16(self.height);

        let g = &self.general;
        let size_fit = g.size_fit.to_u8();
        ensure!(g.flags.0 & 0xf0 == 0, "general.flags uses bits reserved for size_fit/spawn_list");
        ensure!(size_fit < 8, "general.size_fit must be < 8");
        ensure!(g.body_flags.0 & 0x2f == 0, "general.body_flags bits 0-3 are `vehicle_control`, bit 5 is derived from reflection");
        let vc = g.vehicle_control.to_u8();
        ensure!(vc < 16, "general.vehicle_control must be < 16");
        w.u8(0x0a);
        w.u8(g.flags.0 | (g.spawn_list.is_some() as u8) << 4 | size_fit << 5);
        w.u8(g.body_flags.0 | vc | (g.reflection.is_some() as u8) << 5);
        w.u8(g.rope_segments).u8(g.burst_count);
        if let Some(l) = &g.spawn_list {
            l.write(w, ctx)?;
        }
        if let Some([x, y]) = g.reflection {
            w.i32(x.0).i32(y.0);
        }

        let m = &self.movement;
        let loco = m.locomotion.to_u8();
        ensure!(m.flags.0 & 0x30 == 0 && loco < 4, "movement.flags bits 4-5 are `locomotion`");
        w.u8(0x0d).u16(m.speed).u8(m.jump).u8(m.flags.0 | loco << 4);
        w.u8(m.equip_movement.0).u8(m.abilities.0).u8(m.ai_movement.0);

        let s = &self.stats;
        w.u8(0x01).u8(s.unused).u8(s.flags.0).u8(s.sight_range).u8(s.visibility).u8(s.aggression).u8(s.attack_damage);
        write_relations(&self.relations, w, ctx)?;
        write_relations(&self.reverse_relations, w, ctx)?;

        let c = &self.container;
        ensure!(c.flags.0 & 0x10 == 0, "container.flags bit 4 is `with_name_words`");
        w.u8(0x02).u8(c.flags.0 | (c.with_name_words as u8) << 4).u8(c.width).u8(c.height);
        count8(w, c.contents.len(), "container contents")?;
        for o in &c.contents {
            let i = o.object.to_index(ctx)?;
            w.u16(u16::try_from(i).map_err(|_| scribble_core::anyhow!("contained object index {i} does not fit in 16 bits"))?);
            match (c.with_name_words, o.name_word) {
                (true, Some(e)) => {
                    w.u16(e);
                }
                (false, None) => {}
                _ => bail!("every content entry must have `name_word` exactly when `with_name_words` is set"),
            }
        }

        let e = &self.electrical;
        let sm = e.shock_mode.to_u8();
        ensure!(e.flags.0 & 3 == 0 && sm < 4, "electrical.flags bits 0-1 are `shock_mode`");
        w.u8(0x03).u8(e.powered).u8(e.flags.0 | sm);

        w.u8(0x04).u8(self.layer.solidity).u8(self.layer.flags.0);

        let p = &self.physical;
        ensure!(p.flags.0 & 0x20 == 0, "physical.flags bit 5 is derived from unused_4b");
        w.u8(0x05).u8(p.stick_mode.to_u8()).u8(p.material.to_u8()).u8(p.weight).i16(p.health).u8(p.rotates);
        w.u8(p.combustion.to_u8()).u8(p.buoyancy).u8(p.flags.0 | (p.unused_4b.is_some() as u8) << 5);
        if let Some(v) = p.unused_4b {
            w.u16(v);
        }

        let t = &self.thermal;
        w.u8(0x06).u8(t.temperature).u8(t.cold_temperature).u8(t.hot_temperature);

        w.u8(0x07);
        let len_at = w.pos();
        w.u16(0);
        let beh_start = w.pos();
        write_behaviours(&self.behaviours, w, ctx)?;
        let len = u16::try_from(w.pos() - beh_start).map_err(|_| scribble_core::anyhow!("behaviour list too large"))?;
        w.patch_u16(len_at, len);

        w.u8(0x08).u8(self.character_body).u8(self.physics_bodies);
        self.root.write(w, ctx, Flavor::Object, None)?;

        w.u8(0x0b);
        count8(w, self.sounds.len(), "sounds")?;
        for s in &self.sounds {
            w.u8(s.slot.to_u8()).u32(crate::util::from_res32(&s.sound, ctx)?).u8(s.unused);
        }

        if let Some(a) = &self.animations {
            ensure!(a.extra_flags & 7 == 0, "animations.extra_flags bits 0-2 are derived");
            w.u8(0x09);
            w.u8(a.extra_flags | a.attack_duration.is_some() as u8 | (a.unused_swim_attack_duration.is_some() as u8) << 1 | (a.unused_swim_attack2_duration.is_some() as u8) << 2);
            count8(w, a.entries.len(), "animation entries")?;
            for e in &a.entries {
                w.u8(e.slot.0).u32(crate::util::from_res32(&e.anim, ctx)?);
                w.bool(e.events.is_some());
                if let Some(ev) = e.events {
                    w.u16(ev[0]).u16(ev[1]).u16(ev[2]);
                }
            }
            for v in [a.attack_duration, a.unused_swim_attack_duration, a.unused_swim_attack2_duration].into_iter().flatten() {
                w.u16(v);
            }
        }

        let tags_at = w.pos();
        w.u8(0x0e);
        crate::util::write_tags(&self.tags, w, ctx)?;
        let deps_at = w.pos();
        deps::write(&self.dependencies, w, ctx)?;
        w.patch_u32(start, deps_at as u32);
        w.patch_u32(start + 4, tags_at as u32);
        Ok(())
    }

    /// Every `.vec` the body draws (vector nodes, their variants and effect nodes), in tree order.
    pub fn vectors(&self) -> Vec<&ResRef> {
        let mut out = Vec::new();
        for n in self.root.walk() {
            match &n.kind {
                crate::tree::NodeKind::Vector(v) => {
                    out.extend(v.vector.iter().map(|x| &x.vector));
                    out.extend(v.variants.iter().flatten().map(|x| &x.vector));
                }
                crate::tree::NodeKind::Effect(e) => out.extend(e.vector.iter().chain(e.legacy_vector.iter())),
                _ => {}
            }
        }
        out
    }
}

impl Format for ScribbleObject {
    const NAME: &'static str = "so";
    const DESCRIPTION: &'static str = "Scribble object definition: properties, behaviours, node tree, animations";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let id = ObjectPath::read(&mut r, ctx)?;
        let g = r.u8()?;
        let link = r.u32()?;
        let gender = Gender::from_u8(g & 0x0f);
        let body = Body::read(data, 13, ctx)?;
        let (counterpart, female_body) = match gender {
            Gender::Male | Gender::Female => ((link != 0).then(|| ResRef::from_index(link, ctx)), None),
            _ if link != 0 || gender == Gender::Both => {
                let end = Body::end(data, 13)?;
                ensure!(link as usize == end, "second body offset {link:#x} does not follow the first body ({end:#x})");
                (None, Some(Body::read(data, link as usize, ctx)?))
            }
            _ => (None, None),
        };
        let end = Body::end(data, female_body.as_ref().map_or(13, |_| link as usize))?;
        ensure!(end == data.len(), "{} trailing bytes after the last body", data.len() as isize - end as isize);
        Ok(ScribbleObject { id, gender, default_gender: DefaultGender::from_u8(g >> 4), counterpart, body, female_body })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        self.id.write(&mut w, ctx)?;
        let dg = self.default_gender.to_u8();
        ensure!(dg < 16, "default_gender must be < 16");
        let g = self.gender.to_u8();
        ensure!(g < 16, "gender must be < 16");
        w.u8(g | dg << 4);
        let link_at = w.pos();
        let male_female = matches!(self.gender, Gender::Male | Gender::Female);
        w.u32(match (&self.counterpart, male_female) {
            (Some(c), true) => c.to_index(ctx)?,
            (None, _) => 0,
            (Some(_), false) => bail!("`counterpart` requires gender `male` or `female`"),
        });
        self.body.write(&mut w, ctx)?;
        match (&self.female_body, male_female) {
            (Some(b), false) => {
                let at = w.pos() as u32;
                w.patch_u32(link_at, at);
                b.write(&mut w, ctx)?;
            }
            (Some(_), true) => bail!("a male/female object cannot also have `female_body`"),
            (None, _) => ensure!(self.gender != Gender::Both, "gender `both` needs `female_body`"),
        }
        Ok(w.into_inner())
    }
}

impl Locomotion {
    pub fn is_walk(&self) -> bool {
        *self == Locomotion::Walk
    }
}

#[allow(clippy::derivable_impls)] // the variants come from `enum8!`
impl Default for Locomotion {
    fn default() -> Self {
        Locomotion::Walk
    }
}

impl StickMode {
    pub fn is_none(&self) -> bool {
        *self == StickMode::None
    }
}

#[allow(clippy::derivable_impls)]
impl Default for StickMode {
    fn default() -> Self {
        StickMode::None
    }
}
