//! The object's node tree: its visual parts (vector art, mesh parts cut from a vector atlas,
//! effects), physics shapes (collision boxes/circles/outlines, zones, hit boxes) and attachment
//! points (hotspots).
//!
//! Parsed recursively by `FUN_006b6c00` for `.so` bodies and by the simpler `FUN_006e8100` for
//! `.sao` background objects; the object editor writes it back with `FUN_006b30d0`. Every node is
//!
//! ```text
//! u8  type
//! i32 x, i32 y        20.12 position (world pixels) relative to the parent node
//! i32 angle           20.12 degrees, accumulated down the tree (`angle * 0xb60b60b` -> 1/65536 turns)
//! u8  visible         passed to the node's vtable+0x58 after the payload (FUN_006b6c00:
//!                     `(**(code **)(*piVar22 + 0x58))(local_b4)`)
//! <type-specific payload>
//! u8  child_count; child_count x Node
//! ```
//!
//! The mesh-part nodes (type 5) are what an animation drives: the engine collects them in tree
//! order into the object's part list (`FUN_0069ea20`, which also gives each part's renderer its
//! tree-order index at `+0x548` = the atlas piece it draws); each gives the quad the piece is
//! drawn on.

use crate::record::{read_fields, write_fields, Cond, Field, Field::*, Part};
use crate::refs::RefList;
use crate::util::{count8, enum8, flags8, from_res16, from_res32, res16, res32};
use scribble_core::{bail, ensure, Context, Fx12, Hex, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Which parser the tree belongs to; `.sao` files use a reduced node set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    /// `.so` (`FUN_006b6c00`).
    Object,
    /// `.sao` (`FUN_006e8100`): node types 0, 4, 5 and 9 only.
    Simple,
}

/// A node of the object tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Position relative to the parent, in world pixels (20.12; y down). The same unit as the
    /// `.so` width/height and `.anim` translations; `.vec` art is drawn at 4 art pixels per
    /// world pixel. Stored in the node transform `+4`/`+8` (`FUN_006b6c00`).
    pub x: Fx12,
    pub y: Fx12,
    /// Rotation in degrees relative to the parent (20.12; the parser converts it to the
    /// engine's 65536-per-turn angles with `angle * 182.04 >> 12`, transform `+0x1c`).
    #[serde(default, skip_serializing_if = "is_zero_fx")]
    pub angle: Fx12,
    /// Enabled / visible: passed to the node's vtable `+0x58` setter once the payload is read
    /// (`FUN_006b6c00`). Hidden hotspots are the disabled ones (see [`Hotspot::disabled`]).
    pub visible: bool,
    #[serde(flatten)]
    pub kind: NodeKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
}

fn is_zero_fx(v: &Fx12) -> bool {
    v.0 == 0
}

/// Node payload, by node type (`FUN_006b6c00` outer switch).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeKind {
    /// Type 0: transform/grouping node (`FUN_0067ab10`; a few object ids get special subclasses;
    /// the root is always one).
    Group,
    /// Type 1: rectangle. Without parameter blocks it is a solid collision box added to the
    /// object's body (`FUN_005dbb10` box shape, after the children); with one it is a zone
    /// (force field, thermal, liquid or damage).
    BoxZone(BoxZone),
    /// Type 2: circle; like type 1 (`FUN_005c13a0` circle shape) but without liquid zones.
    CircleZone(CircleZone),
    /// Type 3: polygon collision outline (`FUN_0067a940`; shape `FUN_005d5be0`).
    Outline(Outline),
    /// Type 4: vector art (`.vec`) node; parent of the mesh parts cut from it.
    Vector(VectorNode),
    /// Type 5: mesh part - a quad textured with a piece of the parent vector (`FUN_006b4760`).
    MeshPart(MeshPart),
    /// Type 6: the object's hit box, one per animation slot (`FUN_006795a0` -> object `+0x524`).
    HitBoxes(HitBoxes),
    /// Type 7: hotspot / attachment point.
    Hotspot(Hotspot),
    /// Type 8: another object attached at this position (`FUN_006b5610`).
    AttachedObject(AttachedObject),
    /// Type 9: animated vector effect with a flipbook texture (`FUN_006809f0`).
    Effect(EffectNode),
    /// Type 10: no payload and no engine node (the loader's switch has no case 10; unused by
    /// shipped files).
    Marker,
    /// Type 11: stamp / decal image (`FUN_0057e1f0`; the object editor's stamps, reordered by
    /// `FUN_00566fc0`); unused by shipped files.
    Decal(Decal),
    /// Type 12: rope collision node (`FUN_00680150`: a shape built from the object's rope
    /// renderer `+0x6d4`); one ignored byte.
    Rope(RopeNode),
}

impl NodeKind {
    pub fn type_id(&self) -> u8 {
        match self {
            NodeKind::Group => 0,
            NodeKind::BoxZone(_) => 1,
            NodeKind::CircleZone(_) => 2,
            NodeKind::Outline(_) => 3,
            NodeKind::Vector(_) => 4,
            NodeKind::MeshPart(_) => 5,
            NodeKind::HitBoxes(_) => 6,
            NodeKind::Hotspot(_) => 7,
            NodeKind::AttachedObject(_) => 8,
            NodeKind::Effect(_) => 9,
            NodeKind::Marker => 10,
            NodeKind::Decal(_) => 11,
            NodeKind::Rope(_) => 12,
        }
    }
}

enum8!(
    /// How a force field's strength fades across the zone (mode bits 0-3, `FUN_005c6a50`: 2 and
    /// 4 become 1 and 3 with the reverse flag `+0x10e`; applied by `FUN_005c61a0`, which scales
    /// by `1 - d/extent` for 1 and its square for 3).
    Falloff {
        /// Same strength everywhere.
        0 => Constant "constant",
        1 => Linear "linear",
        /// Linear, strongest at the far side.
        2 => LinearReversed "linear_reversed",
        3 => Quadratic "quadratic",
        4 => QuadraticReversed "quadratic_reversed",
    }
);

/// Force-field parameters of a zone (flags bit 0; zone class `FUN_005c5de0`, zone kind `0x0b`,
/// configured by `FUN_005c6a50`, applied every tick by `FUN_005c61a0`).
///
/// ```text
/// i8 radial_strength; u8 mode (bits 0-3 falloff, bit7 magnetic); u8 damping;
/// i8 direction_x; i8 direction_y; RefList affects
/// ```
///
/// All values are scaled `<< 12 >> 6` for boxes and `<< 12 >> 4` for circles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ForceField {
    /// Push away from (negative: pull towards) the zone centre (`+0xe0`; a non-zero value makes
    /// the field radial, `+0x10f`, and `direction` is ignored). JSON `radial_strength`.
    #[serde(rename = "radial_strength")]
    pub strength: i8,
    /// Mode bits 0-3.
    pub falloff: Falloff,
    /// Mode bit 7 (`+0x110`): the field also pulls the zone's owner the opposite way
    /// (`FUN_005c61a0`); set on the magnet.
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub magnetic: bool,
    /// Mode bits 4-6: not read by the loader (zero in shipped files).
    #[serde(default, skip_serializing_if = "crate::util::is_zero_u8")]
    pub unused_mode_bits: u8,
    /// Velocity damping of the zone body (`+0x6c`, the physics body's damping factor that
    /// `FUN_005cf580` multiplies velocities by; zero in shipped files).
    pub damping: u8,
    /// Constant push for non-radial fields (`+0xe4`/`+0xe8`; y down).
    pub direction_x: i8,
    pub direction_y: i8,
    /// Which objects the force applies to (`FUN_00675350`; matched by `FUN_00674aa0` in
    /// `FUN_005c61a0`).
    pub affects: RefList,
}

impl ForceField {
    fn read(r: &mut Reader, ctx: &Context) -> Result<Self> {
        let strength = r.i8()?;
        let mode = r.u8()?;
        Ok(ForceField {
            strength,
            falloff: Falloff::from_u8(mode & 0x0f),
            magnetic: mode & 0x80 != 0,
            unused_mode_bits: (mode >> 4) & 7,
            damping: r.u8()?,
            direction_x: r.i8()?,
            direction_y: r.i8()?,
            affects: RefList::read(r, ctx)?,
        })
    }
    fn write(&self, w: &mut Writer, ctx: &Context) -> Result<()> {
        let f = self.falloff.to_u8();
        ensure!(f < 0x10 && self.unused_mode_bits < 8, "force field falloff must be < 16 and unused_mode_bits < 8");
        w.i8(self.strength).u8(f | self.unused_mode_bits << 4 | (self.magnetic as u8) << 7).u8(self.damping);
        w.i8(self.direction_x).i8(self.direction_y);
        self.affects.write(w, ctx)
    }
}

enum8!(
    /// Liquid of a liquid zone (box flags bit 2): the loader creates a liquid zone
    /// (`FUN_005e2f70`) of zone kind `3 - (liquid != 1)` (`+0xda`), i.e. 2 for anything but 1.
    /// Kind 3 is the lava kind (`gameplay_gameonly_editor__zonelava` stores 1,
    /// `__zonewater` 0; `FUN_005e2f20` tests kind 3 and water-material objects are exempt from
    /// kind-3 zones in `FUN_006a2af0`). The `submerged` triggers' zone types 1/2 select kinds 2/3.
    Liquid {
        0 => Water "water",
        1 => Lava "lava",
    }
);

/// Damage-zone parameters (flags bit 3; zone class `FUN_005c4bc0`, zone kind `0x13`, set by
/// `FUN_005c47a0` and applied by `FUN_005c4e20`).
///
/// ```text
/// u8 amount; u8 repeat; u8 unused
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageZone {
    /// Damage dealt to an object entering the zone (`+0xdc`, `FUN_00672260(amount)`).
    pub amount: u8,
    /// Non-zero: hit again every 60 frames while the object stays inside (`+0xe0`, only
    /// tested against 0 in `FUN_005c4e20`).
    pub repeat: u8,
    /// `+0xe4`: stored and saved (`FUN_005c47d0`) but never read.
    pub unused: u8,
}

impl DamageZone {
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(DamageZone { amount: r.u8()?, repeat: r.u8()?, unused: r.u8()? })
    }
    fn write(&self, w: &mut Writer) {
        w.u8(self.amount).u8(self.repeat).u8(self.unused);
    }
}

/// Type 1 node. The zone class depends on the first parameter block present: a force field
/// (bit 0), else a thermal zone (bit 1; `FUN_005c8f10`, zone kind `0x11`), else a liquid (bit 2),
/// else a damage zone (bit 3). With no block the node is a plain solid collision box.
///
/// ```text
/// u8 flags  bit0 force, bit1 temperature, bit2 liquid, bit3 damage (bits 4-7 unused)
/// [bit0] ForceField
/// [bit1] u32 temperature
/// [bit2] u8 liquid
/// [bit3] DamageZone
/// i32 width, i32 height   (20.12 world pixels)
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoxZone {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force: Option<ForceField>,
    /// Thermal zone temperature (`FUN_005c9040(temperature << 12)` -> `+0xdc`; heaters 170-365,
    /// ice blocks 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub liquid: Option<Liquid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage: Option<DamageZone>,
    pub width: Fx12,
    pub height: Fx12,
    /// Flags bits 4-7: never tested by the loader (zero in shipped files).
    #[serde(default, skip_serializing_if = "crate::util::is_zero_u8")]
    pub unused_flags: u8,
}

/// Type 2 node.
///
/// ```text
/// u8 flags  bit0 force, bit1 temperature, bit3 damage (bit2 and bits 4-7 unused)
/// [bit0] ForceField
/// [bit1] u32 temperature
/// [bit3] DamageZone
/// i32 radius (20.12 world pixels)
/// i8  unused
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CircleZone {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force: Option<ForceField>,
    /// Thermal zone temperature (see [`BoxZone::temperature`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub damage: Option<DamageZone>,
    pub radius: Fx12,
    /// Read and stored x7 at the circle shape's `+0x24` (plain collision circles only;
    /// `FUN_006b6c00` case 2, written back as `value / 7` by `FUN_006b30d0`), which none of the
    /// circle routines reads (shape vtable `0x0083be40`, collision `FUN_005bf8f0`,
    /// `FUN_005bfb50`); zero in shipped files.
    pub unused: i8,
    /// Flags bit 2 and bits 4-7: never tested for circles (zero in shipped files).
    #[serde(default, skip_serializing_if = "crate::util::is_zero_u8")]
    pub unused_flags: u8,
}

/// A vertex of an [`Outline`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutlineVertex {
    pub x: Fx12,
    pub y: Fx12,
    /// Surface speed of the edge starting here (conveyors, escalators): x68 (`0x44000 >> 12`)
    /// into the polygon shape's per-edge array `+0x28` (`FUN_0067a940`), applied along the edge
    /// tangent to touching bodies by the collision routines (`FUN_005c0090`, `FUN_005d4e40`).
    pub surface_speed: i8,
}

/// Type 3 node: a polygon collision shape (edges always wrap around, `FUN_005d5810`).
///
/// ```text
/// u8 n; n x (i32 x, i32 y); n x i8 surface_speed; u8 platform
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outline {
    pub vertices: Vec<OutlineVertex>,
    /// Stand-on surface (`+0x6c`): the shape's mode `+0x30` becomes 1 for background-layer
    /// objects (layer `+0x245` bit 3) and 2 otherwise (`FUN_006b6c00` case 3); mode-1 shapes are
    /// one-way platforms that only collide from above (`FUN_005c0090`, `FUN_005d9f90`,
    /// `FUN_005d4e40`). Set on buildings, furniture and instruments.
    pub platform: bool,
}

/// A vector reference with its optional material mask texture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorRef {
    /// The `.vec` resource.
    pub vector: ResRef,
    /// Material-mask texture (only stored when the node's `masked` flag is set; `null` there
    /// means `0xFFFFFFFF`). Object `+0xb44` (default `0x67e4`); `.sao` files read and drop it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<ResRef>,
}

/// Type 4 node: draws a `.vec`, or is the atlas its mesh-part children cut pieces from.
///
/// ```text
/// u8 flags  bit0 vector, bit1 variants, bit2 rope texture, bit3 masked (bits 4-7 unused)
/// [bit0] u32 vector [bit3: u32 mask]
/// [bit1] u8 n; n x (u32 vector [bit3: u32 mask])      (.sao: masks never stored)
/// [bit2] u32 rope_texture
/// u16 atlas_length
/// [atlas_length > 0, and bit2 clear or .sao] atlas_length skipped bytes + u8 mirror_map[6]
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VectorNode {
    /// The vector drawn (the engine defaults to `0x501b`, `akiosegawa_male_texture.vec`, when it
    /// is `0xFFFFFFFF`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<VectorRef>,
    /// Alternative vectors; the engine picks one at random (per-object seed `+0xb78`). `.sao`
    /// files skip the list (`FUN_006e8100`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variants: Option<Vec<VectorRef>>,
    /// Texture of the rope renderer (`FUN_0060f100`, object `+0x6d4`) the node becomes: the
    /// object is a rope of `rope_segments` segments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rope_texture: Option<ResRef>,
    /// Flag bit 3: vectors carry material masks.
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub masked: bool,
    /// Flags bits 4-7: never tested by the loaders (zero in shipped files).
    #[serde(default, rename = "unused_flags", skip_serializing_if = "crate::util::is_zero_u8")]
    pub extra_flags: u8,
    /// Present when the vector is a texture atlas for mesh-part children: the engine then
    /// builds the vector with `FUN_006b4ea0` (render flags 0x4c28) instead of `FUN_006b4e40`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atlas: Option<Atlas>,
    /// A non-zero atlas length on a rope node: read, but neither skipped nor used there
    /// (`FUN_006b6c00` case 4); kept verbatim. Never occurs in shipped files.
    #[serde(default, rename = "unused_atlas_length", skip_serializing_if = "Option::is_none")]
    pub stray_length: Option<u16>,
}

/// Atlas settings of a [`VectorNode`] (`u16 length; length bytes; u8 mirror_map[6]`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Atlas {
    /// Bytes the loader skips (`*param_4 += length`); the object editor always writes a single
    /// 0 (`FUN_006b30d0`), as does every shipped file. JSON `unused`.
    #[serde(default = "one_zero", rename = "unused", skip_serializing_if = "is_one_zero")]
    pub skipped: Hex,
    /// Six bytes copied to an allocation at object `+0x51c` (`.sao`: `+0x4c`) that only the
    /// editor's writer `FUN_006b30d0` reads back (no other access to object `+0x51c`). The
    /// values are a permutation of mesh-part indices swapping left/right limbs (humans
    /// `[0, 1, 3, 2, 5, 4]`, 255 = none), i.e. a mirror map. JSON `unused_mirror_map`.
    #[serde(rename = "unused_mirror_map")]
    pub params: [u8; 6],
}

fn one_zero() -> Hex {
    Hex(vec![0])
}

fn is_one_zero(h: &Hex) -> bool {
    h.0 == [0]
}

/// One corner of a mesh part quad.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshCorner {
    /// Corner position relative to the node (world pixels).
    pub position: [Fx12; 2],
    /// Where the corner samples the vector atlas, in world pixels from the atlas's bottom-left
    /// corner with v growing downward from -height (the atlas covers `vec.width/4 x vec.height/4`).
    pub uv: [Fx12; 2],
}

/// Explicit bounds of a mesh part (stored in file order; the engine multiplies by 4 into
/// `+0x68`, `+0x64`, `+0x6c`, `+0x70`). When absent the engine derives them from corners 1 and 3
/// (`FUN_006e8100`: min/max of their x and y).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bounds {
    pub min_y: Fx12,
    pub min_x: Fx12,
    pub max_x: Fx12,
    pub max_y: Fx12,
}

/// Body-part classes of mesh parts (`MeshPart::layer`, part `+0x60`). 1, 3 and 5 are used by
/// code: a launcher with no launch point fires from the first part of class 3 (launchers 1
/// and 3), 5 (launcher 4) or 1 (others) (`FUN_006aa1a0`), and the animation bounds track the
/// lowest point of class-5 parts, or class 1 when there are none (`FUN_00667f10`). The names
/// follow the equip slots hung under each class in shipped objects (1: back/tail/seats,
/// 3: head/face, 4: hands and arm slots 4-7, 5: leg slots 8-11, 6: tail slot 15).
pub const BODY_PARTS: &[(u8, &str)] =
    &[(0, "none"), (1, "body"), (2, "neck"), (3, "head"), (4, "arm"), (5, "leg"), (6, "tail"), (7, "wing")];

mod body_part {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u8, s: S) -> Result<S::Ok, S::Error> {
        match super::BODY_PARTS.iter().find(|(k, _)| k == v) {
            Some((_, n)) => s.serialize_str(n),
            None => s.serialize_u8(*v),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            N(u8),
            S(String),
        }
        match Repr::deserialize(d)? {
            Repr::N(n) => Ok(n),
            Repr::S(s) => super::BODY_PARTS
                .iter()
                .find(|(_, n)| *n == s)
                .map(|(k, _)| *k)
                .ok_or_else(|| serde::de::Error::custom(format!("unknown body part {s:?}"))),
        }
    }
}

/// Type 5 node: a piece of the parent vector's atlas drawn on a quad; this is what animation
/// tracks move. The piece is chosen by the part's tree-order index (`FUN_0069ea20`).
///
/// ```text
/// u8  flags   bits 0-5 body_part, bit6 rider_split, bit7 has bounds   (.sao: bits 0-6 = body_part)
/// u16 draw_order
/// 4 x { i32 x, i32 y (position), i32 u, i32 v (atlas coordinate) }
/// [bit7] i32 min_y, i32 min_x, i32 max_x, i32 max_y
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshPart {
    /// Body-part class (flags bits 0-5, part `+0x60`; see [`BODY_PARTS`]). JSON `body_part`.
    #[serde(rename = "body_part", with = "body_part")]
    pub layer: u8,
    /// Flags bit 6 (`+0x5c`): when a creature is ridden (seat `mount_type` 1) the rider is drawn
    /// between the mount's parts, in front of this part and the ones after it in draw order
    /// (`FUN_004c5b10`). JSON `rider_split`.
    #[serde(default, skip_serializing_if = "crate::util::is_false")]
    pub rider_split: bool,
    /// Draw order among the object's parts (`+0x58`/`+0x5a`; `FUN_004c5b10` sorts parts by it,
    /// comparator `0x004c4a00`, and `FUN_004cbde0` turns it into the initial depth; the editor's
    /// PUSH BEHIND / BRING IN FRONT swaps it, `FUN_00566ea0`). A permutation of `0..n` in
    /// shipped objects. JSON `draw_order`.
    #[serde(rename = "draw_order")]
    pub part: u16,
    pub corners: [MeshCorner; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
}

/// One collision box.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitBox {
    pub x: Fx12,
    pub y: Fx12,
    pub width: Fx12,
    pub height: Fx12,
}

/// Type 6 node: the object's hit box (object `+0x524`, `FUN_006795a0`).
///
/// ```text
/// i32 width, i32 height; u8 has_boxes; [has_boxes] u8 n; n x (i32 x, y, w, h)
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitBoxes {
    /// Default box size (`+0x54`/`+0x58`).
    pub width: Fx12,
    pub height: Fx12,
    /// One box per animation slot (60 in every shipped file; index = slot, see
    /// `fmt_anim::slots`): table `+0x1e0 + slot*8` (x, -y) and `+slot*8` (w, h). The box of the
    /// playing slot (object `+0x500`) is used relative to the idle slot 14 (`FUN_006c84a0`);
    /// slot 20 takes slot 28's box. JSON `animation_boxes`.
    #[serde(default, rename = "animation_boxes", skip_serializing_if = "Option::is_none")]
    pub boxes: Option<Vec<HitBox>>,
}

/// Type 7 node: a hotspot (attachment point, spawn point, launch point, ...).
///
/// ```text
/// u8 kind_flags   bits 0-4 kind, bit5 has rope_end, bit6 mirrored, bit7 disabled
/// [bit5] u8 rope_end
/// <kind-specific fields>
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Hotspot {
    /// Hotspot class (bits 0-4, see [`HOTSPOTS`]; hotspot `+0x54`, looked up by
    /// `FUN_0069a0e0(kind, ...)`).
    pub kind: u8,
    /// Bit 5: `rope_end` (`+0x5c`, default 1; vtable `+0x60` getter `0x004d2780`): when an item
    /// is equipped from a rope object, non-zero attaches it at the rope's last segment, 0 at
    /// the first (`FUN_006adbc0`). The loader overrides it for grips (slot 0x81) and attach
    /// points. JSON `rope_end`.
    pub rope_end: Option<u8>,
    /// Bit 6 (`+0x58`, passed to the constructors): the hotspot is mirrored — seats negate the
    /// rider offset (`FUN_006aeed0`), launch points pass it to their emitter (`FUN_004d4ed0`).
    /// JSON `mirrored`.
    pub mirrored: bool,
    /// Bit 7: starts disabled (`!bit7` goes to vtable `+0x58`; fire points keep it as unlit;
    /// the `toggle_hotspot` modifier switches it). JSON `disabled`.
    pub disabled: bool,
    pub fields: Map<String, Value>,
}

/// Type 8 node.
///
/// ```text
/// [parent is a group node] u16 object
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttachedObject {
    /// The attached `.so` (u16 pmindex index; spawned by `FUN_006b5610` through the same
    /// spawn-resource slot `DAT_008a8700+0xe8` as equip-slot objects; only stored under group
    /// nodes — elsewhere the loader reads nothing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ResRef>,
}

flags8!(
    /// Flags of an effect node (`FUN_006809f0`).
    EffectFlags {
        /// Starts stopped (`+0x58` = !bit0; the renderer's playback rate `+0x51c` is zeroed).
        0 => "stopped",
        /// Plays on its own (`+0x59` = !bit1: when clear the animation only runs while driven;
        /// `FUN_0069bd50`).
        1 => "ignore_driver",
    }
);

/// Type 9 node (`FUN_006809f0`).
///
/// ```text
/// u16 legacy_vector; u32 vector (0xFFFFFFFF = use legacy_vector); u8 animation; i8 layer;
/// i32 speed (20.12); u8 flags; u16 flipbook
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectNode {
    /// 16-bit vector index used when `vector` is `0xFFFFFFFF`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_vector: Option<ResRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vector: Option<ResRef>,
    /// Flipbook animation (sequence) index (`FUN_006ef830(animation, 0x1000, 1)`). JSON
    /// `animation`.
    #[serde(rename = "animation")]
    pub layer: u8,
    /// Draw layer (`+0x5a`, i8: negative = drawn behind the object's parts, `FUN_004c5b10`).
    /// JSON `layer`.
    #[serde(rename = "layer")]
    pub param: u8,
    /// Playback speed (`+0x54`, 20.12, default 1.0).
    pub speed: Fx12,
    pub flags: EffectFlags,
    /// Flipbook texture (`.sfb`, `FUN_006efd80`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flipbook: Option<ResRef>,
}

/// Type 11 node (`FUN_0057e1f0`; unused by shipped files).
///
/// ```text
/// u8 painted; u32 vector; i32 scale; u32 order; u8 flip; u8 has_mesh;
/// [has_mesh] u8 part; 4 x MeshCorner; i32 bounds[4]
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decal {
    /// Non-zero: the image gets its own instance so it can be painted (`FUN_006b6510` unique id
    /// OR'd into the vector index); the writer sets it when the object has a paint layer for
    /// the vector (`FUN_0067a000`).
    pub painted: u8,
    /// Vector resource (the loader keeps the low 16 bits).
    pub vector: u32,
    /// Scale (renderer `+0x14`/`+0x18`, 20.12; the writer stores its absolute value).
    pub scale: Fx12,
    /// Stacking order among the stamps (short at `+0x72`; reordered by `FUN_00566fc0`).
    pub order: u32,
    /// Horizontally flipped (compared with the renderer's flip `+0xc`; negates the x scale).
    pub flip: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<DecalMesh>,
}

/// Mesh of a [`Decal`] drawn from an atlas piece.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecalMesh {
    /// Atlas piece (renderer `+0x548`, the index `FUN_0069ea20` assigns to mesh parts).
    pub part: u8,
    pub corners: [MeshCorner; 4],
    /// Stored at `+0x7c`, `+0x78`, `+0x80`, `+0x84` in the same pattern as mesh-part bounds.
    pub bounds: Bounds,
}

/// Type 12 node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RopeNode {
    /// Byte skipped by the engine (`FUN_00680150` starts with `inc dword ptr [pos]`).
    #[serde(default, skip_serializing_if = "crate::util::is_zero_u8")]
    pub unused: u8,
}

// ---------------------------------------------------------------------------------------------
// Hotspot kinds

/// Hotspot kind definition: name and payload schema (`FUN_006b6c00` case 7).
pub struct HotspotDef {
    pub id: u8,
    pub name: &'static str,
    pub fields: &'static [Field],
}

/// Hotspot kind names (`FUN_006b6c00` case 7), for tables that refer to kinds.
pub const HOTSPOT_KINDS: &[(u8, &str)] = &[
    (0, "rope_anchor"), (1, "hook_point"), (2, "seat"), (3, "sit_point"), (4, "container_opening"), (5, "equip_slot"), (6, "climb_point"),
    (7, "split_parts"), (8, "light"), (9, "launch_point"), (10, "fire_point"), (12, "jointed_part"), (13, "unused_0d"), (14, "attach_point"),
    (15, "bare_rope_anchor"), (17, "dig_point"), (18, "unused_12"),
];

/// Seat anchors: which point of the rider's bounding box sits on the seat (3 x 3 grid;
/// `FUN_006aeed0` takes the top for 0-2, bottom for 6-8, left for 0/3/6, right for 2/5/8 and
/// centres on 4).
const ANCHORS: &[(u8, &str)] = &[
    (0, "top_left"), (1, "top"), (2, "top_right"), (3, "left"), (4, "center"), (5, "right"), (6, "bottom_left"), (7, "bottom"), (8, "bottom_right"),
];

/// Hotspot kinds with their payloads (parsers named per kind; the game's own writer is
/// `FUN_006b30d0`). Kinds without an entry (11, 16, 19-31) get a plain hotspot
/// (`FUN_004d26b0`) with no payload.
pub static HOTSPOTS: &[HotspotDef] = &[
    // FUN_004d2870; `rope` -> +0x68, `segments` -> +0x6c: the loader spawns `rope` and lays its
    // segments from here (`segments` overrides the rope's own count; 0 keeps it).
    HotspotDef {
        id: 0,
        name: "rope_anchor",
        fields: &[Flags("flags", &["has_rope", "has_segments"]), If(Cond::Bit("flags", 1), &[Res32("rope"), If(Cond::Bit("flags", 2), &[U8("segments")])])],
    },
    // Plain hotspot (FUN_004d26b0); the rope code attaches its line here (FUN_0067b8a0,
    // FUN_0067c9e0 look up kind 1): fishing hooks, grappling hooks, the trailer hitch.
    HotspotDef { id: 1, name: "hook_point", fields: &[] },
    // FUN_004d8c60; mount FUN_006aeed0, dismount FUN_006ad2f0: the rider plays
    // `rider_animation` (+0x64), its bounding box `anchor` (+0xa4, 4 when `has_anchor` is clear)
    // is aligned with the seat; `driver` (+0xa0) marks the driver's seat; `mount_type` (+0x68)
    // decides where the rider is drawn (FUN_004c5b10: 1 between the mount's parts at its
    // `rider_split` part, 2 after them); `riders` (+0x6c) limits who may sit; `occupant` is
    // spawned into the seat. Bits 2-3 of the second byte are never tested; `unused_a1` (+0xa1)
    // is only read by the writer and by a getter (0x006b0ee0) nothing calls.
    HotspotDef {
        id: 2,
        name: "seat",
        fields: &[
            AnimSlot("rider_animation"),
            Split(&[Part::Bool("driver", 0x01), Part::Bool("has_anchor", 0x02), Part::Rest("unused_bits", 0x0c), Part::Enum("anchor", 0xf0, ANCHORS)]),
            Enum("mount_type", &[(0, "vehicle"), (1, "creature"), (2, "furniture")]),
            Bool("unused_a1"),
            RefList("riders"),
            Bool("has_occupant"),
            If(Cond::NonZero("has_occupant"), &[Res16("occupant")]),
        ],
    },
    // What this rider may sit on (FUN_004d8e00, +0x68; FUN_004d8aa0).
    HotspotDef { id: 3, name: "sit_point", fields: &[RefList("mounts")] },
    // Where contents enter and leave a container (FUN_004d4410; looked up by the container
    // code FUN_0066e000, FUN_0066e100, FUN_0066fa20).
    HotspotDef { id: 4, name: "container_opening", fields: &[] },
    // FUN_004d5d40: equip slot (+0x70: 1 hand, 2 head, 3 torso, 4/5 arms, 6/7 hands, 8/9 legs,
    // 10/11 feet, 12 waist, 13 back, 14 face, 15 tail; bit 7 = the item's own grip; the slot
    // decides the draw order of the equipped item, FUN_004c5b10) with accepted objects (+0x7c)
    // and an optional object equipped at spawn (skipped with spawn flag 0x10).
    HotspotDef { id: 5, name: "equip_slot", fields: &[I8("slot"), RefList("accepts"), Bool("has_object"), If(Cond::NonZero("has_object"), &[Res16("object")])] },
    // FUN_004d8f40: two per object form a ladder (the loader builds the climb object at
    // object +0x904 from the first and second point, FUN_005b7820).
    HotspotDef { id: 6, name: "climb_point", fields: &[] },
    // FUN_004d8e90: the two parts the object splits into (spawned only if both are set), with
    // their positions (same unit as node positions) and angles (engine units, transform +0x1c).
    HotspotDef {
        id: 7,
        name: "split_parts",
        fields: &[Res16("first"), Field::Fx12("first_x"), Field::Fx12("first_y"), U32("first_angle"), Res16("second"), Field::Fx12("second_x"), Field::Fx12("second_y"), U32("second_angle")],
    },
    // FUN_004d8090 / FUN_004e5da0 (gpuprograms/light.gp): a point light of `radius` (+0x82), or
    // a spot light with `near`/`far` (+0x84/+0x86, in 1/148 of its length) and a full cone
    // angle `spread` (+0x88, radians); the flicker pattern (+0x68), depth, phase and period
    // (+0x6c..+0x74); texture rows (+0x80/+0x81; without `has_textures` one byte is skipped);
    // colour A, R, G, B (+0x7f, +0x7c, +0x7d, +0x7e; candles 255,255,0, fire 255,0,0).
    HotspotDef {
        id: 8,
        name: "light",
        fields: &[
            Flags("flags", &["has_flicker", "has_textures", "tinted"]),
            Enum("shape", &[(0, "point"), (1, "spot")]),
            If(Cond::Eq("shape", 0), &[U16("radius")]),
            If(Cond::Eq("shape", 1), &[U16("near"), U16("far"), Field::Fx12("spread")]),
            U8("flicker_pattern"),
            If(Cond::Bit("flags", 1), &[Field::Fx12("flicker_depth"), Field::Fx12("flicker_phase"), Field::Fx12("flicker_period")]),
            IfElse(Cond::Bit("flags", 2), &[U8("attenuation_row"), U8("aperture_row")], &[U8("unused")]),
            U8("alpha"), U8("red"), U8("green"), U8("blue"),
        ],
    },
    // FUN_004d4890 / launcher FUN_004d4ed0: fires `projectile` (+0x80, or particle `effect`
    // +0xa4 with `effect_only` +0xa0) every `interval` frames (+0x84) along `aim` (+0x68/+0x6c;
    // (0, 0) = the hotspot's angle) +- `spread` degrees (+0x70); `gravity` +0x7d, `burst` +0x7e,
    // `particles` +0x90 (set: +0x80 is not an object but the `stream_type` passed to the
    // particle-stream spawner FUN_00474f60, as in set_launcher), `clear_on_stop` +0xa1 (effect
    // branch only); `unused_flag` (+0x7c) is
    // only read by the writer FUN_004d4b40. `launcher_id` (+0x94) is what set_launcher and
    // fire actions refer to (FUN_006aa1a0).
    HotspotDef {
        id: 9,
        name: "launch_point",
        fields: &[
            Flags("flags", &["effect_only", "unused_flag", "gravity", "clear_on_stop"]),
            IfElse(
                Cond::Bit("flags", 1),
                &[Res32("effect")],
                &[
                    Field::Fx12("aim_x"), Field::Fx12("aim_y"), Field::Fx12("spread"), U32("interval"), Field::Fx12("speed"), U8("burst"), Bool("particles"),
                    IfElse(Cond::Eq("particles", 1), &[U32("stream_type")], &[Res32("projectile")]),
                ],
            ),
            U8("launcher_id"),
        ],
    },
    // FUN_004d6150 ("FireSpot"; header `disabled` = starts unlit): size +0x64 (bits 0-6),
    // follow_rotation +0xa8 (bit 7: the flame takes the node angle).
    HotspotDef {
        id: 10,
        name: "fire_point",
        fields: &[Split(&[
            Part::Enum("size", 0x7f, &[(0, "tiny"), (1, "small"), (2, "medium"), (3, "large"), (4, "huge"), (5, "auto")]),
            Part::Bool("follow_rotation", 0x80),
        ])],
    },
    // FUN_004d7cb0 / FUN_006952b0: an object jointed to this one at `pivot` (wheels, windmill
    // blades, fruit); wheels (+0x7d) are driven, `front_wheel`s (+0x7c) always; spin +0x80,
    // stiffness +0x81 (default 100).
    HotspotDef {
        id: 12,
        name: "jointed_part",
        fields: &[
            Flags("flags", &["front_wheel", "wheel"]), I8("spin_speed"), U8("stiffness"), Res16("part"), Field::Fx12("pivot_x"), Field::Fx12("pivot_y"),
            Field::Fx12("part_angle"),
        ],
    },
    // The loader skips 16 bytes and creates no node (`case 0xd: *param_4 += 0x10`).
    HotspotDef { id: 13, name: "unused_0d", fields: &[Bytes("unused", 16)] },
    // FUN_004d3210: where attachments connect (connection types 0x40/0x80 look up kind 14,
    // FUN_006872c0, FUN_006c6960, FUN_004d2920).
    HotspotDef { id: 14, name: "attach_point", fields: &[] },
    // Builds a rope anchor (FUN_004d2870 constructs kind 0) with no payload: no rope.
    HotspotDef { id: 15, name: "bare_rope_anchor", fields: &[] },
    // FUN_006b4da0 (+0x64); FUN_0053fd00 passes dig_size / 16 + 1 to a stub in the PC build.
    HotspotDef { id: 17, name: "dig_point", fields: &[I16("dig_size")] },
    // A plain hotspot (FUN_004d26b0) that no code looks up (no FUN_0069a0e0(0x12, ...) and no
    // kind-0x12 test); the freezer and the Christmas lights carry one.
    HotspotDef { id: 18, name: "unused_12", fields: &[] },
];

const NO_FIELDS: &[Field] = &[];

fn hotspot_fields(kind: u8) -> &'static [Field] {
    HOTSPOTS.iter().find(|d| d.id == kind).map_or(NO_FIELDS, |d| d.fields)
}

fn hotspot_name(kind: u8) -> String {
    HOTSPOTS.iter().find(|d| d.id == kind).map_or_else(|| format!("hotspot_{kind:02x}"), |d| d.name.to_string())
}

impl Serialize for Hotspot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut m = Map::new();
        m.insert("kind".into(), hotspot_name(self.kind).into());
        if let Some(c) = self.rope_end {
            m.insert("rope_end".into(), c.into());
        }
        if self.mirrored {
            m.insert("mirrored".into(), true.into());
        }
        if self.disabled {
            m.insert("disabled".into(), true.into());
        }
        for (k, v) in &self.fields {
            m.insert(k.clone(), v.clone());
        }
        m.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Hotspot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        use serde::de::Error;
        let mut m = Map::deserialize(d)?;
        let name = m.remove("kind").and_then(|v| v.as_str().map(str::to_string)).ok_or_else(|| D::Error::custom("hotspot needs \"kind\""))?;
        let kind = match HOTSPOTS.iter().find(|h| h.name == name) {
            Some(h) => h.id,
            None => name
                .strip_prefix("hotspot_")
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| D::Error::custom(format!("unknown hotspot kind {name:?}")))?,
        };
        let rope_end = m.remove("rope_end").map(serde_json::from_value).transpose().map_err(D::Error::custom)?;
        let mirrored = m.remove("mirrored").and_then(|v| v.as_bool()).unwrap_or(false);
        let disabled = m.remove("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
        Ok(Hotspot { kind, rope_end, mirrored, disabled, fields: m })
    }
}

// ---------------------------------------------------------------------------------------------
// Binary

fn fx(r: &mut Reader) -> Result<Fx12> {
    Ok(Fx12(r.i32()?))
}

fn corners(r: &mut Reader) -> Result<[MeshCorner; 4]> {
    let mut c = [MeshCorner { position: [Fx12(0); 2], uv: [Fx12(0); 2] }; 4];
    for k in &mut c {
        k.position = [fx(r)?, fx(r)?];
        k.uv = [fx(r)?, fx(r)?];
    }
    Ok(c)
}

fn write_corners(c: &[MeshCorner; 4], w: &mut Writer) {
    for k in c {
        w.i32(k.position[0].0).i32(k.position[1].0).i32(k.uv[0].0).i32(k.uv[1].0);
    }
}

fn read_bounds(r: &mut Reader) -> Result<Bounds> {
    Ok(Bounds { min_y: fx(r)?, min_x: fx(r)?, max_x: fx(r)?, max_y: fx(r)? })
}

fn write_bounds(b: &Bounds, w: &mut Writer) {
    w.i32(b.min_y.0).i32(b.min_x.0).i32(b.max_x.0).i32(b.max_y.0);
}

impl Node {
    /// Read a node and its subtree. `parent` is the parent's node type (`None` for the root).
    pub fn read(r: &mut Reader, ctx: &Context, flavor: Flavor, parent: Option<u8>) -> Result<Self> {
        let at = r.pos();
        let t = r.u8()?;
        let x = fx(r)?;
        let y = fx(r)?;
        let angle = fx(r)?;
        let visible = r.bool()?;
        if flavor == Flavor::Simple && !matches!(t, 0 | 4 | 5 | 9) {
            bail!("node type {t} at {at:#x} is not valid in a simple object");
        }
        let kind = match t {
            0 => NodeKind::Group,
            1 => {
                let f = r.u8()?;
                let force = if f & 1 != 0 { Some(ForceField::read(r, ctx)?) } else { None };
                let temperature = if f & 2 != 0 { Some(r.u32()?) } else { None };
                let liquid = if f & 4 != 0 { Some(Liquid::from_u8(r.u8()?)) } else { None };
                let damage = if f & 8 != 0 { Some(DamageZone::read(r)?) } else { None };
                NodeKind::BoxZone(BoxZone { force, temperature, liquid, damage, width: fx(r)?, height: fx(r)?, unused_flags: f & 0xf0 })
            }
            2 => {
                let f = r.u8()?;
                let force = if f & 1 != 0 { Some(ForceField::read(r, ctx)?) } else { None };
                let temperature = if f & 2 != 0 { Some(r.u32()?) } else { None };
                let damage = if f & 8 != 0 { Some(DamageZone::read(r)?) } else { None };
                NodeKind::CircleZone(CircleZone { force, temperature, damage, radius: fx(r)?, unused: r.i8()?, unused_flags: f & 0xf4 })
            }
            3 => {
                let n = r.u8()? as usize;
                let pts = (0..n).map(|_| Ok((fx(r)?, fx(r)?))).collect::<Result<Vec<_>>>()?;
                let vals = (0..n).map(|_| r.i8()).collect::<Result<Vec<_>>>()?;
                let vertices = pts.into_iter().zip(vals).map(|((x, y), surface_speed)| OutlineVertex { x, y, surface_speed }).collect();
                NodeKind::Outline(Outline { vertices, platform: r.bool()? })
            }
            4 => NodeKind::Vector(read_vector(r, ctx, flavor)?),
            5 => {
                let f = r.u8()?;
                let part = r.u16()?;
                let corners = corners(r)?;
                let bounds = if f & 0x80 != 0 { Some(read_bounds(r)?) } else { None };
                NodeKind::MeshPart(MeshPart { layer: f & 0x3f, rider_split: f & 0x40 != 0, part, corners, bounds })
            }
            6 => {
                let width = fx(r)?;
                let height = fx(r)?;
                let boxes = if r.bool()? {
                    let n = r.u8()?;
                    Some((0..n).map(|_| Ok(HitBox { x: fx(r)?, y: fx(r)?, width: fx(r)?, height: fx(r)? })).collect::<Result<Vec<_>>>()?)
                } else {
                    None
                };
                NodeKind::HitBoxes(HitBoxes { width, height, boxes })
            }
            7 => {
                let b = r.u8()?;
                let rope_end = if b & 0x20 != 0 { Some(r.u8()?) } else { None };
                let kind = b & 0x1f;
                let fields = read_fields::<crate::behaviour::Hooks>(hotspot_fields(kind), r, ctx)
                    .map_err(|e| scribble_core::anyhow!("hotspot {kind} at {at:#x}: {e}"))?;
                NodeKind::Hotspot(Hotspot { kind, rope_end, mirrored: b & 0x40 != 0, disabled: b & 0x80 != 0, fields })
            }
            8 => NodeKind::AttachedObject(AttachedObject {
                object: if parent == Some(0) { Some(ResRef::from_index(r.u16()? as u32, ctx)) } else { None },
            }),
            9 => NodeKind::Effect(EffectNode {
                legacy_vector: res16(r.u16()?, ctx),
                vector: res32(r.u32()?, ctx),
                layer: r.u8()?,
                param: r.u8()?,
                speed: fx(r)?,
                flags: EffectFlags(r.u8()?),
                flipbook: res16(r.u16()?, ctx),
            }),
            10 => NodeKind::Marker,
            11 => {
                let painted = r.u8()?;
                let vector = r.u32()?;
                let scale = fx(r)?;
                let order = r.u32()?;
                let flip = r.u8()?;
                let mesh = if r.u8()? != 0 { Some(DecalMesh { part: r.u8()?, corners: corners(r)?, bounds: read_bounds(r)? }) } else { None };
                NodeKind::Decal(Decal { painted, vector, scale, order, flip, mesh })
            }
            12 => NodeKind::Rope(RopeNode { unused: r.u8()? }),
            t => bail!("unknown node type {t} at {at:#x}"),
        };
        let n = r.u8()?;
        let children = (0..n).map(|_| Node::read(r, ctx, flavor, Some(t))).collect::<Result<_>>()?;
        Ok(Node { x, y, angle, visible, kind, children })
    }

    pub fn write(&self, w: &mut Writer, ctx: &Context, flavor: Flavor, parent: Option<u8>) -> Result<()> {
        let t = self.kind.type_id();
        if flavor == Flavor::Simple && !matches!(t, 0 | 4 | 5 | 9) {
            bail!("node type {t} is not valid in a simple object");
        }
        w.u8(t).i32(self.x.0).i32(self.y.0).i32(self.angle.0).bool(self.visible);
        match &self.kind {
            NodeKind::Group | NodeKind::Marker => {}
            NodeKind::BoxZone(z) => {
                ensure!(z.unused_flags & 0x0f == 0, "box zone unused_flags must only use bits 4-7");
                let f = z.unused_flags
                    | z.force.is_some() as u8
                    | (z.temperature.is_some() as u8) << 1
                    | (z.liquid.is_some() as u8) << 2
                    | (z.damage.is_some() as u8) << 3;
                w.u8(f);
                if let Some(ff) = &z.force {
                    ff.write(w, ctx)?;
                }
                if let Some(v) = z.temperature {
                    w.u32(v);
                }
                if let Some(v) = z.liquid {
                    w.u8(v.to_u8());
                }
                if let Some(d) = &z.damage {
                    d.write(w);
                }
                w.i32(z.width.0).i32(z.height.0);
            }
            NodeKind::CircleZone(z) => {
                ensure!(z.unused_flags & 0x0b == 0, "circle zone unused_flags must not use bits 0, 1, 3");
                let f = z.unused_flags | z.force.is_some() as u8 | (z.temperature.is_some() as u8) << 1 | (z.damage.is_some() as u8) << 3;
                w.u8(f);
                if let Some(ff) = &z.force {
                    ff.write(w, ctx)?;
                }
                if let Some(v) = z.temperature {
                    w.u32(v);
                }
                if let Some(d) = &z.damage {
                    d.write(w);
                }
                w.i32(z.radius.0).i8(z.unused);
            }
            NodeKind::Outline(o) => {
                count8(w, o.vertices.len(), "outline vertices")?;
                for v in &o.vertices {
                    w.i32(v.x.0).i32(v.y.0);
                }
                for v in &o.vertices {
                    w.i8(v.surface_speed);
                }
                w.bool(o.platform);
            }
            NodeKind::Vector(v) => write_vector(v, w, ctx, flavor)?,
            NodeKind::MeshPart(m) => {
                ensure!(m.layer < 0x40, "mesh part body_part must be < 64");
                w.u8(m.layer | (m.rider_split as u8) << 6 | (m.bounds.is_some() as u8) << 7);
                w.u16(m.part);
                write_corners(&m.corners, w);
                if let Some(b) = &m.bounds {
                    write_bounds(b, w);
                }
            }
            NodeKind::HitBoxes(h) => {
                w.i32(h.width.0).i32(h.height.0);
                w.bool(h.boxes.is_some());
                if let Some(b) = &h.boxes {
                    count8(w, b.len(), "hit boxes")?;
                    for x in b {
                        w.i32(x.x.0).i32(x.y.0).i32(x.width.0).i32(x.height.0);
                    }
                }
            }
            NodeKind::Hotspot(h) => {
                ensure!(h.kind < 0x20, "hotspot kind must be < 32");
                w.u8(h.kind | (h.rope_end.is_some() as u8) << 5 | (h.mirrored as u8) << 6 | (h.disabled as u8) << 7);
                if let Some(c) = h.rope_end {
                    w.u8(c);
                }
                write_fields::<crate::behaviour::Hooks>(hotspot_fields(h.kind), &h.fields, w, ctx)?;
            }
            NodeKind::AttachedObject(a) => match (parent == Some(0), &a.object) {
                (true, Some(o)) => {
                    let i = o.to_index(ctx)?;
                    w.u16(u16::try_from(i).map_err(|_| scribble_core::anyhow!("attached object index {i} does not fit in 16 bits"))?);
                }
                (false, None) => {}
                (true, None) => bail!("attached object under a group node needs `object`"),
                (false, Some(_)) => bail!("attached object `object` is only stored under group nodes"),
            },
            NodeKind::Effect(e) => {
                w.u16(from_res16(&e.legacy_vector, ctx)?);
                w.u32(from_res32(&e.vector, ctx)?);
                w.u8(e.layer).u8(e.param).i32(e.speed.0).u8(e.flags.0);
                w.u16(from_res16(&e.flipbook, ctx)?);
            }
            NodeKind::Decal(d) => {
                w.u8(d.painted).u32(d.vector).i32(d.scale.0).u32(d.order).u8(d.flip);
                w.bool(d.mesh.is_some());
                if let Some(m) = &d.mesh {
                    w.u8(m.part);
                    write_corners(&m.corners, w);
                    write_bounds(&m.bounds, w);
                }
            }
            NodeKind::Rope(p) => {
                w.u8(p.unused);
            }
        }
        count8(w, self.children.len(), "child nodes")?;
        for c in &self.children {
            c.write(w, ctx, flavor, Some(t))?;
        }
        Ok(())
    }

    /// Depth-first iterator over this node and its descendants.
    pub fn walk(&self) -> impl Iterator<Item = &Node> {
        let mut stack = vec![self];
        std::iter::from_fn(move || {
            let n = stack.pop()?;
            stack.extend(n.children.iter().rev());
            Some(n)
        })
    }

    /// Mesh parts in tree order (the order the engine builds the animated part list in).
    pub fn mesh_parts(&self) -> Vec<&MeshPart> {
        self.walk().filter_map(|n| if let NodeKind::MeshPart(m) = &n.kind { Some(m) } else { None }).collect()
    }
}

fn read_vector(r: &mut Reader, ctx: &Context, flavor: Flavor) -> Result<VectorNode> {
    let f = r.u8()?;
    let masked = f & 8 != 0;
    let vref = |r: &mut Reader, with_mask: bool| -> Result<VectorRef> {
        let vector = ResRef::from_index(r.u32()?, ctx);
        let mask = if with_mask { res32(r.u32()?, ctx) } else { None };
        Ok(VectorRef { vector, mask })
    };
    let vector = if f & 1 != 0 { Some(vref(r, masked)?) } else { None };
    let variants = if f & 2 != 0 {
        let n = r.u8()?;
        Some((0..n).map(|_| vref(r, masked && flavor == Flavor::Object)).collect::<Result<Vec<_>>>()?)
    } else {
        None
    };
    let rope_texture = if f & 4 != 0 { Some(ResRef::from_index(r.u32()?, ctx)) } else { None };
    let len = r.u16()?;
    let (atlas, stray_length) = if len == 0 {
        (None, None)
    } else if f & 4 == 0 || flavor == Flavor::Simple {
        (Some(Atlas { skipped: Hex(r.bytes(len as usize)?.to_vec()), params: r.array()? }), None)
    } else {
        (None, Some(len))
    };
    Ok(VectorNode { vector, variants, rope_texture, masked, extra_flags: f & 0xf0, atlas, stray_length })
}

fn write_vector(v: &VectorNode, w: &mut Writer, ctx: &Context, flavor: Flavor) -> Result<()> {
    ensure!(v.extra_flags & 0x0f == 0, "vector unused_flags must only use bits 4-7");
    let f = v.extra_flags
        | v.vector.is_some() as u8
        | (v.variants.is_some() as u8) << 1
        | (v.rope_texture.is_some() as u8) << 2
        | (v.masked as u8) << 3;
    w.u8(f);
    let wref = |w: &mut Writer, x: &VectorRef, with_mask: bool| -> Result<()> {
        w.u32(x.vector.to_index(ctx)?);
        if with_mask {
            w.u32(from_res32(&x.mask, ctx)?);
        } else {
            ensure!(x.mask.is_none(), "vector mask given but the node is not masked");
        }
        Ok(())
    };
    if let Some(x) = &v.vector {
        wref(w, x, v.masked)?;
    }
    if let Some(list) = &v.variants {
        count8(w, list.len(), "vector variants")?;
        for x in list {
            wref(w, x, v.masked && flavor == Flavor::Object)?;
        }
    }
    if let Some(t) = &v.rope_texture {
        w.u32(t.to_index(ctx)?);
    }
    match (&v.atlas, v.stray_length) {
        (Some(a), None) => {
            ensure!(!a.skipped.0.is_empty(), "atlas.unused must not be empty");
            w.u16(u16::try_from(a.skipped.0.len())?);
            w.bytes(&a.skipped.0).bytes(&a.params);
        }
        (None, Some(n)) => {
            w.u16(n);
        }
        (None, None) => {
            w.u16(0);
        }
        (Some(_), Some(_)) => bail!("vector node cannot have both an atlas and an unused atlas length"),
    }
    Ok(())
}
