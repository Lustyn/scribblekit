//! Assemble a scribble object's node tree (optionally posed by an animation) into flat,
//! object-space geometry: vertex-coloured triangles from its `.vec` art plus debug overlays.
//!
//! Every convention below is taken from the engine (`re/decompiled.c`); the evidence ledger is
//! `docs/evidence/words-art.md`.
//!
//! # Units and placement
//!
//! Positions are world pixels, y down (node `x`/`y` are 20.12 fixed point). A `.vec` of
//! `W x H` art pixels covers `W/4 x H/4` world pixels ([`ART_PIXELS_PER_UNIT`]): the mesh-part
//! parser `FUN_006b6c00` (case 5) multiplies the stored bounds by 4 (its render object
//! `FUN_005afed0` does the same to the corners), and the engine
//! sizes art by its texture size `floor(W/4)*4 x floor(H/4)*4` art pixels (`FUN_00734ae0`).
//!
//! * **Whole-art vector nodes** (a type-4 node without an atlas / mesh-part children) are drawn by
//!   a vector render object built with a zero offset (`FUN_005afa80` passes position `(0,0)` to
//!   `FUN_00628fe0`), i.e. the art is centred on the node: normalised art coordinates
//!   `[-0.5, 0.5]²` scale to the render object's size, which `FUN_00629760` takes from the
//!   resource's texture size ([`world_size`]).
//! * **Mesh parts** (type 5) carry four corners of `(x, y, u, v)` (`FUN_006b6c00` case 5 copies
//!   them to the render object `FUN_005afed0`, which scales all 16 values by 4 with
//!   `* 0x4000 >> 12`). `u` runs right from the atlas' left edge, `v` counts from the atlas'
//!   bottom edge, both in world pixels, so the atlas spans `u in [0, W/4]`, `v in [-H/4, 0]`
//!   (shipped UVs are negative upward): `FUN_005b18a0` normalises them as `u / W` and `-v / H`.
//!   The part draws its atlas triangles (`FUN_005afd20` -> `FUN_00731970`) translated so the
//!   UV rectangle's top-left lands on the quad's top-left ([`atlas_placement`]).
//! * **Which `.vec` part a mesh part shows**: the part (bone) whose id is the mesh part's
//!   *tree-order index* among the object's mesh parts. `FUN_0069ea20` numbers the mesh-part
//!   nodes in depth-first order, stores the index at node `+0x5d` and in the render object
//!   (`+0x548`), and `FUN_005afce0` -> `FUN_00735a30` -> `FUN_007355e0` looks the `.vec` part map
//!   up with that index. `.anim` tracks address parts by the same index (the part list).
//!   `MeshPart::part` is *not* a bone id: it is the part's **draw rank** (see below).
//!
//! # Transforms
//!
//! Each node is placed relative to its parent: translate by `(x, y)`, then rotate by `angle`
//! (degrees; the parser converts to 65536-per-turn units with `angle * 182.04 >> 12`), angles
//! accumulating down the tree. `FUN_0071ad60` (scene-node world transform) does exactly this:
//! world angle `+0x30` = parent angle + local angle (negated for mirrored nodes), world
//! position `+0x28` = parent position + R(parent angle) * local position, with
//! `R = [cos -sin; sin cos]` from the sine table `DAT_0086f338` (`FUN_004f0530`: angle 0x4000
//! gives sin = 4096). With y down a positive angle therefore turns clockwise on screen. A
//! playing `.anim` replaces a mesh part's local transform with `rest + key offset`
//! (`FUN_006ec550` adds the rest pose captured in the part list to every key; `FUN_006ed5c0`
//! resets each part to its rest pose every tick before the clips are applied; the `.sao`
//! evaluator's spin rule sets an absolute angle, see `PartPose::rotation_is_absolute`).
//!
//! # Draw order
//!
//! `FUN_004c5b10` assigns every render object of an object an increasing depth (`+0xe`, turned
//! into a z by `FUN_004cbde0`: z = (1 - depth/0x7fff) * 128, so a higher depth is nearer the
//! camera and drawn over lower ones; the engine's own passes use 0 for the backdrop and 0x7fff
//! last, `FUN_0062db10`). Within one object:
//!
//! 1. effect nodes (type 9) whose layer byte (`+0x5a`, i8) is **negative**, in tree order;
//! 2. objects linked to this one as children (object `+0x184` list) and joint partners
//!    (`+0x6e0`), recursively, i.e. behind it;
//! 3. the object's art: for a mesh-part rig, the mesh parts sorted **ascending by
//!    `MeshPart::part`** (`qsort` with comparator `FUN_004c4a00` on node `+0x58`, ties never
//!    occur in shipped data), each followed by the items equipped at its `equip_slot` hotspots
//!    (`FUN_004c58d0`: slots 2, 4, 5, 8, 9, 12, 14, 15 (and 1 / 13 conditionally) first, then
//!    3, 6, 7, 10, 11); otherwise the object's main vector node (`+0x6c4`) and the equipped
//!    items of its hotspots. Riders split the list at the `rider_split` part (`FUN_004c5b10`
//!    passes 1/2/3 draw the parts behind the root part, up to the split, and after it);
//! 4. effect nodes whose layer is **>= 0**, in tree order;
//! 5. further attached objects (`FUN_0069a0e0(0xc / 0, ..)` lists), then one overlay object
//!    (`+0x8e0`) at depth + 500.
//!
//! Only the sign of an effect's layer matters. Attached and equipped objects are separate
//! objects and are not drawn here; this renderer draws only steps 1, 3 and 4. `.sao`
//! backgrounds (`FUN_006e8100`) give each mesh part depth `base + part`, the same order. The mesh part's flags bits 0-5 (node `+0x60`, `layer` /
//! body-part class in `fmt_object`) play no part in drawing: it is the body-part class that
//! attachment code looks parts up by (`FUN_00477450`: 1 body, 3 head, 4 arm, 5 leg).
//!
//! Before this was traced the renderer drew *higher* `layer` values first, which put Maxwell's
//! torso (layer 1) over his head (layer 3); by `part` his parts go far arm 0, far leg 1,
//! torso 2, head 3, near leg 4, near arm 5.

use fmt_anim::Pose;
use fmt_object::tree::{Node, NodeKind};
use fmt_vec::VectorArt;
use scribble_core::ResRef;
use std::sync::Arc;

/// `.vec` art is authored at 4x the world resolution (mesh-part bounds are stored /4 and
/// multiplied by 4 in `FUN_006b6c00`).
pub const ART_PIXELS_PER_UNIT: f32 = 4.0;

/// 2D affine transform `[a b c; d e f]` mapping `(x, y) -> (a x + b y + c, d x + e y + f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine(pub [f32; 6]);

impl Affine {
    pub const IDENTITY: Affine = Affine([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
    pub fn translate(x: f32, y: f32) -> Self {
        Affine([1.0, 0.0, x, 0.0, 1.0, y])
    }
    pub fn rotate_degrees(deg: f32) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Affine([c, -s, 0.0, s, c, 0.0])
    }
    pub fn scale(sx: f32, sy: f32) -> Self {
        Affine([sx, 0.0, 0.0, 0.0, sy, 0.0])
    }
    pub fn then(self, inner: Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = inner.0;
        Affine([a * a2 + b * d2, a * b2 + b * e2, a * c2 + b * f2 + c, d * a2 + e * d2, d * b2 + e * e2, d * c2 + e * f2 + f])
    }
    pub fn apply(&self, [x, y]: [f32; 2]) -> [f32; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * x + b * y + c, d * x + e * y + f]
    }
    /// The affine map sending `src[i]` to `dst[i]` for three points, if they are not collinear.
    pub fn from_points(src: [[f32; 2]; 3], dst: [[f32; 2]; 3]) -> Option<Affine> {
        let [[x0, y0], [x1, y1], [x2, y2]] = src;
        let det = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
        if det.abs() < 1e-9 {
            return None;
        }
        // Inverse of the source basis, then compose with the destination basis.
        let inv = Affine([(y2 - y0) / det, -(x2 - x0) / det, 0.0, -(y1 - y0) / det, (x1 - x0) / det, 0.0]).then(Affine::translate(-x0, -y0));
        let [[u0, v0], [u1, v1], [u2, v2]] = dst;
        Some(Affine([u1 - u0, u2 - u0, u0, v1 - v0, v2 - v0, v0]).then(inv))
    }
}

/// A triangle in object space with straight-alpha RGBA vertex colours.
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub pos: [[f32; 2]; 3],
    pub color: [[u8; 4]; 3],
}

/// One drawable piece (a mesh part or a whole vector).
#[derive(Clone, Debug)]
pub struct Piece {
    /// Draw rank inside the object (see the module docs): `MeshPart::part` for mesh parts, 0 for
    /// whole-art vectors, and for effect nodes `-1` (behind) or [`FRONT_EFFECT_RANK`].
    pub rank: i32,
    /// Index in the animated part list, for mesh parts.
    pub part_index: Option<usize>,
    pub triangles: Vec<Tri>,
}

#[derive(Clone, Debug)]
pub enum Overlay {
    /// Closed polygon (zones, hit boxes, outlines) in object space.
    Polygon { points: Vec<[f32; 2]>, kind: &'static str },
    Circle { center: [f32; 2], radius: f32 },
    /// A labelled point (hotspots, attached objects, node origins).
    Point { at: [f32; 2], label: String },
}

#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// In draw order (back to front).
    pub pieces: Vec<Piece>,
    pub overlays: Vec<Overlay>,
    /// Mesh parts found (length of the animated part list).
    pub part_count: usize,
    /// Resources that could not be loaded.
    pub missing: Vec<String>,
}

impl Scene {
    /// Bounding box of all triangles `(min, max)`.
    pub fn bounds(&self) -> Option<([f32; 2], [f32; 2])> {
        let mut it = self.pieces.iter().flat_map(|p| p.triangles.iter().flat_map(|t| t.pos));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])))
    }
}

/// Rank of effect nodes with a non-negative layer: drawn after (over) all of the object's art.
pub const FRONT_EFFECT_RANK: i32 = 0x10000;
/// Rank of effect nodes with a negative layer: drawn before (under) all of the object's art.
pub const BACK_EFFECT_RANK: i32 = -1;

/// Loads `.vec` art by resource reference.
pub trait VecSource {
    fn vector(&mut self, r: &ResRef) -> Option<Arc<VectorArt>>;
}

fn fx(v: scribble_core::Fx12) -> f32 {
    v.to_f64() as f32
}

struct Builder<'a> {
    vecs: &'a mut dyn VecSource,
    pose: Option<&'a Pose>,
    scene: Scene,
    order: usize,
    /// (rank, tree order) per piece, for the final sort.
    keys: Vec<(i32, usize)>,
}

/// Build the scene for a node tree. `pose` offsets mesh parts (by tree-order index).
pub fn build(root: &Node, vecs: &mut dyn VecSource, pose: Option<&Pose>) -> Scene {
    let mut b = Builder { vecs, pose, scene: Scene::default(), order: 0, keys: Vec::new() };
    b.node(root, Affine::IDENTITY, None);
    // Back to front: ascending rank (`FUN_004c5b10`), ties in tree order.
    let mut idx: Vec<usize> = (0..b.scene.pieces.len()).collect();
    idx.sort_by_key(|&i| b.keys[i]);
    let pieces = std::mem::take(&mut b.scene.pieces);
    let mut slots: Vec<Option<Piece>> = pieces.into_iter().map(Some).collect();
    b.scene.pieces = idx.into_iter().map(|i| slots[i].take().unwrap()).collect();
    b.scene
}

fn art_triangles(art: &VectorArt, bone: Option<u16>) -> Vec<fmt_vec::render::Triangle> {
    match bone {
        Some(b) => art.part_mesh(b).map(|m| m.triangles).unwrap_or_default(),
        None => art.draw_triangles(),
    }
}

impl Builder<'_> {
    fn load(&mut self, r: &ResRef) -> Option<Arc<VectorArt>> {
        let v = self.vecs.vector(r);
        if v.is_none() {
            let name = match r {
                ResRef::Path(p) => p.clone(),
                ResRef::Index(i) => format!("#{i}"),
            };
            if !self.scene.missing.contains(&name) {
                self.scene.missing.push(name);
            }
        }
        v
    }

    fn push(&mut self, rank: i32, part_index: Option<usize>, triangles: Vec<Tri>) {
        self.keys.push((rank, self.order));
        self.order += 1;
        self.scene.pieces.push(Piece { rank, part_index, triangles });
    }

    /// Whole-art placement: normalised art coordinates -> node space (centred on the node).
    fn whole_art(&mut self, art: &VectorArt, world: Affine, rank: i32) {
        let (w, h) = world_size(art);
        let m = world.then(Affine::scale(w, h));
        let tris = art_triangles(art, None).iter().map(|t| tri(t, m)).collect();
        self.push(rank, None, tris);
    }

    fn node(&mut self, n: &Node, parent: Affine, art: Option<Arc<VectorArt>>) {
        let (mut x, mut y, mut angle) = (fx(n.x), fx(n.y), fx(n.angle));
        let mut part_index = None;
        if let NodeKind::MeshPart(_) = n.kind {
            let i = self.scene.part_count;
            self.scene.part_count += 1;
            part_index = Some(i);
            // Animation tracks address mesh parts by tree-order index (the part list built by
            // `FUN_0069ea20`), not by `MeshPart::part`; key values are offsets from the rest pose.
            if let Some(p) = self.pose.map(|p| p.part(i)) {
                if let Some(r) = p.rotation {
                    if p.rotation_is_absolute {
                        angle = r.degrees() as f32;
                    } else {
                        angle += r.degrees() as f32;
                    }
                }
                if let Some((dx, dy)) = p.translation {
                    x += dx.to_f64() as f32;
                    y += dy.to_f64() as f32;
                }
            }
        }
        let world = parent.then(Affine::translate(x, y)).then(Affine::rotate_degrees(angle));
        let mut art = art;
        match &n.kind {
            NodeKind::Vector(v) => {
                let vref = v.vector.as_ref().map(|r| r.vector.clone()).or_else(|| v.variants.as_ref().and_then(|vs| vs.first()).map(|r| r.vector.clone()));
                art = vref.and_then(|r| self.load(&r));
                let has_parts = n.walk().any(|c| matches!(c.kind, NodeKind::MeshPart(_)));
                if !has_parts && let Some(a) = art.clone() {
                    self.whole_art(&a, world, 0);
                }
            }
            NodeKind::MeshPart(m) => {
                if let Some(a) = &art {
                    let mtx = world.then(atlas_placement(a, &m.corners));
                    // The atlas piece is the vec part whose bone id is this mesh part's index in
                    // tree order (`FUN_0069ea20` -> `FUN_005afce0`); its UV rectangle frames
                    // exactly that part's bounds. `part` is the draw rank.
                    let bone = part_index.unwrap_or(0) as u16;
                    let tris = art_triangles(a, Some(bone)).iter().map(|t| tri(t, mtx)).collect();
                    self.push(m.part as i32, part_index, tris);
                }
            }
            NodeKind::Effect(e) => {
                if let Some(r) = e.vector.as_ref().or(e.legacy_vector.as_ref())
                    && let Some(a) = self.load(r)
                {
                    // Only the sign of the i8 layer byte matters (`FUN_004c5b10`).
                    let rank = if (e.param as i8) < 0 { BACK_EFFECT_RANK } else { FRONT_EFFECT_RANK };
                    self.whole_art(&a, world, rank);
                }
            }
            NodeKind::BoxZone(z) => self.rect(world, 0.0, 0.0, fx(z.width), fx(z.height), "zone"),
            NodeKind::CircleZone(z) => {
                self.scene.overlays.push(Overlay::Circle { center: world.apply([0.0, 0.0]), radius: fx(z.radius) });
            }
            // The node's own box; its `boxes` list holds alternates the engine swaps in.
            NodeKind::HitBoxes(h) => self.rect(world, 0.0, 0.0, fx(h.width), fx(h.height), "hit_box"),
            NodeKind::Outline(o) => {
                let points = o.vertices.iter().map(|v| world.apply([fx(v.x), fx(v.y)])).collect();
                self.scene.overlays.push(Overlay::Polygon { points, kind: "outline" });
            }
            NodeKind::Hotspot(h) => {
                let name = fmt_object::tree::HOTSPOTS.iter().find(|d| d.id == h.kind).map(|d| d.name.to_string()).unwrap_or(format!("hotspot {}", h.kind));
                let slot = h.fields.get("slot").map(|s| format!(" {s}")).unwrap_or_default();
                self.scene.overlays.push(Overlay::Point { at: world.apply([0.0, 0.0]), label: format!("{name}{slot}") });
            }
            NodeKind::AttachedObject(_) => {
                self.scene.overlays.push(Overlay::Point { at: world.apply([0.0, 0.0]), label: "attached object".into() });
            }
            _ => {}
        }
        for c in &n.children {
            self.node(c, world, art.clone());
        }
    }

    fn rect(&mut self, m: Affine, cx: f32, cy: f32, w: f32, h: f32, kind: &'static str) {
        let (hw, hh) = (w / 2.0, h / 2.0);
        let points = [[cx - hw, cy - hh], [cx + hw, cy - hh], [cx + hw, cy + hh], [cx - hw, cy + hh]].map(|p| m.apply(p)).to_vec();
        self.scene.overlays.push(Overlay::Polygon { points, kind });
    }
}

/// World-pixel size of a vector's art: its texture size (`floor(W/4)*4 x floor(H/4)*4` art
/// pixels, the render object's `+0x46`/`+0x44` set from the resource in `FUN_00629760`) at
/// [`ART_PIXELS_PER_UNIT`] art pixels per world pixel.
pub fn world_size(art: &VectorArt) -> (f32, f32) {
    let (w, h) = art.texture_size();
    (w as f32 / ART_PIXELS_PER_UNIT, h as f32 / ART_PIXELS_PER_UNIT)
}

/// Normalised art coordinates -> mesh-part node space, as `FUN_005b18a0` builds it: a pure
/// translation that puts the atlas point `(u_min, v_min)` (smallest `u` / `v` over the four
/// corners; `u` from the atlas' left edge, `v` from its bottom edge, negative upward) on the
/// quad's top-left corner `(min(x1, x3), min(y1, y3))`. The quad's size and the corner pairing
/// play no further part (in shipped data the quad always matches the UV rectangle).
pub fn atlas_placement(art: &VectorArt, corners: &[fmt_object::tree::MeshCorner; 4]) -> Affine {
    let (w, h) = world_size(art);
    let u_min = corners.iter().map(|c| fx(c.uv[0])).fold(f32::INFINITY, f32::min);
    let v_min = corners.iter().map(|c| fx(c.uv[1])).fold(f32::INFINITY, f32::min);
    let x_min = fx(corners[1].position[0]).min(fx(corners[3].position[0]));
    let y_min = fx(corners[1].position[1]).min(fx(corners[3].position[1]));
    // atlas u = (nx + 0.5) * w, atlas v = (ny + 0.5) * h - h; node = (x_min, y_min) + (u, v) - (u_min, v_min)
    Affine([w, 0.0, 0.5 * w + x_min - u_min, 0.0, h, -0.5 * h + y_min - v_min])
}

fn tri(t: &fmt_vec::render::Triangle, m: Affine) -> Tri {
    Tri { pos: t.vertices.map(|v| m.apply(v.position)), color: t.vertices.map(|v| v.color) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scribble_core::Format;

    fn corner(x: f64, y: f64, u: f64, v: f64) -> fmt_object::tree::MeshCorner {
        let f = |v: f64| scribble_core::Fx12::from_f64(v).unwrap();
        fmt_object::tree::MeshCorner { position: [f(x), f(y)], uv: [f(u), f(v)] }
    }

    #[test]
    fn atlas_placement_maps_uv_rect_onto_quad() {
        // 40 x 40 art px = 10 x 10 world px; the quad shows the atlas' bottom-left quarter.
        let art = VectorArt {
            version: 2,
            width: 40.0,
            height: 40.0,
            position_encoding: fmt_vec::PositionEncoding::Float,
            palette: Vec::new(),
            parts: Vec::new(),
            bone_bounds: None,
            mesh_count: None,
            vertices: Vec::new(),
            triangles: Vec::new(),
        };
        let c = [corner(3.0, -2.0, 5.0, -5.0), corner(-2.0, -2.0, 0.0, -5.0), corner(-2.0, 3.0, 0.0, 0.0), corner(3.0, 3.0, 5.0, 0.0)];
        let m = atlas_placement(&art, &c);
        // Atlas (u 0, v -5) = art (-0.5, 0.0) -> quad top-left; atlas bottom-right corner (u 5, v 0).
        let p = m.apply([-0.5, 0.0]);
        assert!((p[0] + 2.0).abs() < 1e-4 && (p[1] + 2.0).abs() < 1e-4, "{p:?}");
        let p = m.apply([0.0, 0.5]);
        assert!((p[0] - 3.0).abs() < 1e-4 && (p[1] - 3.0).abs() < 1e-4, "{p:?}");
    }

    struct Dir(std::path::PathBuf, scribble_core::Context);
    impl VecSource for Dir {
        fn vector(&mut self, r: &ResRef) -> Option<Arc<VectorArt>> {
            let ResRef::Path(p) = r else { return None };
            let data = std::fs::read(p.split('\\').fold(self.0.clone(), |d, c| d.join(c))).ok()?;
            Some(Arc::new(VectorArt::decode(&data, &self.1).ok()?))
        }
    }

    #[test]
    fn maxwell_parts_draw_in_part_order() {
        let Some(root) = scribble_core::testing::extracted_root() else { return };
        let ctx = scribble_formats::load_context(&root).unwrap();
        let data = std::fs::read(root.join("data/_game/scribbleobjects/human_player_maxwell_maxwell.so")).unwrap();
        let so = fmt_object::ScribbleObject::decode(&data, &ctx).unwrap();
        let scene = build(&so.body.root, &mut Dir(root, ctx), None);
        // Tree order: torso(part 2), head(3), arm(5), arm(0), leg(4), leg(1); FUN_004c5b10 sorts by part.
        let order: Vec<i32> = scene.pieces.iter().map(|p| p.rank).collect();
        assert_eq!(order, [0, 1, 2, 3, 4, 5]);
        let tree_index: Vec<Option<usize>> = scene.pieces.iter().map(|p| p.part_index).collect();
        assert_eq!(tree_index, [Some(3), Some(5), Some(0), Some(1), Some(4), Some(2)]);
    }
    #[test]
    fn affine_from_points_roundtrips() {
        let src = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let dst = [[2.0, 3.0], [4.0, 3.0], [2.0, 6.0]];
        let a = Affine::from_points(src, dst).unwrap();
        for i in 0..3 {
            let p = a.apply(src[i]);
            assert!((p[0] - dst[i][0]).abs() < 1e-5 && (p[1] - dst[i][1]).abs() < 1e-5);
        }
    }
}
