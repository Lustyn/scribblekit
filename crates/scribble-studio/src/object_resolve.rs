//! What the game really draws for objects whose own node tree does not show their appearance.
//!
//! Some `.so` files draw nothing useful by themselves: their only art is the placeholder
//! `[platform]\datavector\misc\paper\gift.vec` (a present), or they have no vector at all because
//! the engine renders them as a textured rope. [`resolve`] works out what to show instead; the
//! evidence for each rule is collected in `docs/evidence/placeholders.md`.
//!
//! * **Ropes.** A vector node with a `rope_texture` makes the loader (`FUN_006b6c00`, node type
//!   4, rope branch) build a rope renderer (`FUN_0060f100`) instead of drawing a `.vec`: the rope
//!   is `rope_segments` segments of 16 world pixels (`rope_segments << 4` is passed as the
//!   length and `FUN_0060ee10` divides it by the count), drawn by `rope.gp` with the `*landr`
//!   texture. That texture holds three 128-pixel bands
//!   (`FUN_0060fc50` mode 3: v offsets 0, 0.4 and 0.8, height 0.2): the middle band repeats
//!   along the rope, the other two are the frayed start and end. Each segment maps the whole
//!   band width onto the segment plus a cap at each end (`rope.gp`:
//!   `u = (z*r + w*(r+L)) / (2r+L)`; see [`ROPE_CAP`]). A `rope_anchor` hotspot with a `rope` object (ball and
//!   chain, tow truck, swings) spawns that rope there with `segments` segments.
//! * **Rope pieces** (`tool_rope_pieces__*`) are leftovers from the old piece-by-piece ropes.
//!   Nothing in the game spawns the gift/art-less ones; they stand for one segment of the
//!   textured rope named in [`ROPE_PIECES`] (`X` = a middle segment, `Xend` = the end one).
//! * **Avatars** (`human_player_avatars__*`, hidden `@` words) are not in the player avatar
//!   table (`FUN_00445ba0`: Maxwell, his 40 siblings, Edgar, Julie, Lily) and no code or data
//!   spawns them; only relation/filter lists name them. They carry the NPC biped animation set,
//!   not Maxwell's rig, so they are shown as the object the same word without `@` spawns
//!   (`@BALLET DANCER` -> `BALLET DANCER` -> `human_entertainment_dancer_dancer.so`).
//! * **Engine stand-ins** (`_adjective_*`, `_self_*`, `_stage_*`) only appear as targets in
//!   relation and filter lists and are never spawned, so nothing is drawn.

use crate::object_scene::Affine;
use crate::workspace::Workspace;
use fmt_object::ScribbleObject;
use fmt_object::tree::{Node, NodeKind, VectorNode};
use scribble_core::{Format, Fx12, ResRef};
use std::collections::HashMap;

/// The placeholder art (`misc\paper\gift.vec`, resource 22403).
pub const GIFT_VEC: &str = r"[platform]\datavector\misc\paper\gift.vec";

/// World pixels per rope segment (`FUN_006b6c00`: length = `rope_segments << 4`, split evenly by
/// `FUN_0060ee10`).
pub const ROPE_SEGMENT_LENGTH: f32 = 16.0;
/// End-cap length in world pixels (`rope.gp`'s `a_scale.x`): each segment's quad runs one cap
/// past both ends and the band's width covers `cap + L + cap`. Every `*landr` middle band draws
/// its body between texels ~75 and ~440 of 512, i.e. `cap / (2 cap + L) = 0.146`, so
/// `cap = 0.206 L`. (The rope descriptor also carries 2.5, `DAT_008294b4`, which is probably the
/// physics radius; with it the links would not meet.)
pub const ROPE_CAP: f32 = ROPE_SEGMENT_LENGTH * 75.0 / 362.0;
/// Half the rope's drawn thickness: the 512 x 128 band keeps its aspect ratio.
pub const ROPE_HALF_WIDTH: f32 = (ROPE_SEGMENT_LENGTH + 2.0 * ROPE_CAP) * 128.0 / 512.0 / 2.0;
/// Height of one texture band (`FUN_0060fc50` mode 3).
const BAND_HEIGHT: f32 = 0.2;

/// Which band of a `*landr` rope texture a segment uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// Top band (v 0-0.2): the repeating middle of the rope.
    Middle,
    /// Second band (v 0.4-0.6): the start of the rope, frayed/capped on the left.
    Start,
    /// Third band (v 0.8-1.0): the end of the rope, frayed/capped on the right.
    End,
}

impl Band {
    fn v_offset(self) -> f32 {
        match self {
            Band::Middle => 0.0,
            Band::Start => 0.4,
            Band::End => 0.8,
        }
    }
}

/// Rope pieces whose own art is a placeholder, and the rope object whose textured rope they
/// are a segment of (same `*landr` texture family). `nunchuk` is a misspelt duplicate of
/// `nunchuck`; `ballandchain` is the chain that `tool_rope_other_ballandchain.so`'s rope anchor
/// spawns.
pub const ROPE_PIECES: &[(&str, &str)] = &[
    ("ballandchain", "tool_restraint_other_chain"),
    ("barbwire", "tool_special_noequip_barbedwire"),
    ("barrierrope", "misc_fabric_mediumintegrity_velvetrope"),
    ("belt", "clothes_torso_other_belt"),
    ("bungeecord", "tool_rope_other_bungeecord"),
    ("cautiontape", "misc_fabric_lowintegrity_cautiontape"),
    ("christmaslight", "furniture_appliances_light_christmaslights"),
    ("extensioncord", "tool_rope_charged_extensioncord"),
    ("firehose", "tool_spraywater_sub2none_firehose"),
    ("floss", "tool_rope_other_floss"),
    ("guitarstrap", "audio_accessories_other_guitarstrap"),
    ("handcuff", "tool_restraint_other_handcuffs"),
    ("hose", "tool_spraywater_sub2none_hose"),
    ("jumpercable", "tool_rope_charged_jumpercable"),
    ("jumprope", "tool_rope_other_jumprope"),
    ("leash", "tool_restraint_other_leash"),
    ("net", "tool_rope_pieces__net"),
    ("nunchuck", "weapon_melee_bashing_nunchucks"),
    ("nunchuk", "weapon_melee_bashing_nunchucks"),
    ("powerlines", "tool_rope_charged_powerlines"),
    ("rope", "tool_rope_other_rope"),
    ("shackles", "tool_rope_other_shackles"),
    ("shoelace", "tool_rope_other_shoelace"),
    ("string", "audio_accessories_other_string"),
    ("vine", "plants_othergreenery_ropelike_vine"),
    ("whip", "weapon_melee_other_whip"),
    ("wire", "tool_rope_charged_wire"),
];

/// Rope pieces with no textured rope in the game whose art another object draws.
const PIECE_OBJECTS: &[(&str, &str)] = &[("lassoend", "tool_rope_grapple_lasso")];

const OBJECT_DIR: &str = r"data\_game\scribbleobjects\";

/// Words <-> objects (English dictionary), to find what a hidden `@` word's plain twin spawns.
#[derive(Default)]
pub struct WordIndex {
    by_object: HashMap<String, Vec<String>>,
    by_word: HashMap<String, Vec<String>>,
}

impl WordIndex {
    pub fn from_dictionary(d: &fmt_dictionary::Dictionary) -> Self {
        let mut ix = WordIndex::default();
        for e in d.words.iter().filter(|e| e.kind == fmt_dictionary::WordKind::Object) {
            for s in &e.meanings {
                if let Some(ResRef::Path(p)) = d.objects.get(&s.id).and_then(|t| t.properties.resource.as_ref()) {
                    ix.by_object.entry(p.clone()).or_default().push(e.word.clone());
                    ix.by_word.entry(e.word.clone()).or_default().push(p.clone());
                }
            }
        }
        ix
    }

    /// Words that spawn an object (logical path).
    pub fn words_for(&self, object: &str) -> &[String] {
        self.by_object.get(object).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Objects (logical paths) a word spawns.
    pub fn objects_for(&self, word: &str) -> &[String] {
        self.by_word.get(word).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// A textured quad in object space: corners and texture coordinates (0..1, v down).
#[derive(Clone, Copy, Debug)]
pub struct TexQuad {
    pub pos: [[f32; 2]; 4],
    pub uv: [[f32; 2]; 4],
}

/// A rope to draw with a DDS texture.
#[derive(Clone, Debug)]
pub struct RopeDraw {
    /// Logical path of the `*landr` texture.
    pub texture: String,
    pub quads: Vec<TexQuad>,
}

/// Which node tree to draw.
#[derive(Clone, Debug)]
pub enum Body {
    /// The object's own tree.
    Own,
    /// Another object's tree (its female body when the female toggle is on).
    Object { path: String, object: Box<ScribbleObject> },
    /// Nothing: the object is never drawn by the game.
    Nothing,
}

/// What to draw for an object, and why.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub body: Body,
    /// Textured ropes, drawn behind the body.
    pub ropes: Vec<RopeDraw>,
    /// Shown on the canvas when the appearance comes from somewhere else.
    pub note: Option<String>,
}

impl Resolved {
    pub fn own() -> Self {
        Resolved { body: Body::Own, ropes: Vec::new(), note: None }
    }

    /// The tree to draw: `own` is the opened object's tree.
    pub fn root<'a>(&'a self, own: Option<&'a Node>, female: bool) -> Option<&'a Node> {
        match &self.body {
            Body::Own => own,
            Body::Object { object, .. } => Some(match (&object.female_body, female) {
                (Some(b), true) => &b.root,
                _ => &object.body.root,
            }),
            Body::Nothing => None,
        }
    }

    /// Whether the substituted object spawns with its female body unless told otherwise
    /// (`default_gender` female, e.g. Cleopatra).
    pub fn prefers_female(&self) -> bool {
        matches!(&self.body, Body::Object { object, .. } if object.female_body.is_some() && object.default_gender == fmt_object::so::DefaultGender::Female)
    }

    /// Whether the substituted object has a female body to toggle to.
    pub fn has_female_body(&self) -> bool {
        matches!(&self.body, Body::Object { object, .. } if object.female_body.is_some())
    }

    /// Bounding box of the ropes.
    pub fn rope_bounds(&self) -> Option<([f32; 2], [f32; 2])> {
        let mut it = self.ropes.iter().flat_map(|r| r.quads.iter().flat_map(|q| q.pos));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])])))
    }

    fn note(&mut self, s: String) {
        self.note = Some(match self.note.take() {
            Some(n) => format!("{n}\n{s}"),
            None => s,
        });
    }
}

/// Union of two optional bounding boxes.
pub fn union_bounds(a: Option<([f32; 2], [f32; 2])>, b: Option<([f32; 2], [f32; 2])>) -> Option<([f32; 2], [f32; 2])> {
    match (a, b) {
        (Some((l1, h1)), Some((l2, h2))) => Some(([l1[0].min(l2[0]), l1[1].min(l2[1])], [h1[0].max(h2[0]), h1[1].max(h2[1])])),
        (a, b) => a.or(b),
    }
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit('\\').next().unwrap_or(path);
    name.strip_suffix(".so").unwrap_or(name)
}

fn res_path(r: &ResRef, ws: &Workspace) -> Option<String> {
    match r {
        ResRef::Path(p) => Some(p.clone()),
        ResRef::Index(i) => ws.ctx.name(*i).map(str::to_string),
    }
}

fn load_object(ws: &Workspace, path: &str) -> Option<ScribbleObject> {
    let data = ws.read_logical(path).ok()?;
    ScribbleObject::decode(&data, &ws.ctx).ok()
}

/// `.vec` references drawn by vector nodes of a tree.
fn drawn_vectors(root: &Node) -> Vec<&ResRef> {
    let mut out = Vec::new();
    for n in root.walk() {
        if let NodeKind::Vector(v) = &n.kind {
            out.extend(v.vector.as_ref().map(|r| &r.vector));
            out.extend(v.variants.iter().flatten().map(|r| &r.vector));
        }
    }
    out
}

fn is_gift(r: &ResRef, ws: &Workspace) -> bool {
    res_path(r, ws).is_some_and(|p| p.eq_ignore_ascii_case(GIFT_VEC))
}

/// Whether the tree's only art is the gift placeholder.
pub fn is_gift_placeholder(root: &Node, ws: &Workspace) -> bool {
    let v = drawn_vectors(root);
    !v.is_empty() && v.iter().all(|r| is_gift(r, ws))
}

/// A real rope texture on a vector node (not absent / `0xFFFFFFFF`).
fn rope_texture(v: &VectorNode, ws: &Workspace) -> Option<String> {
    let r = v.rope_texture.as_ref()?;
    if matches!(r, ResRef::Index(u32::MAX)) {
        return None;
    }
    res_path(r, ws)
}

/// The first textured rope of an object: `(texture, rope_segments)`.
fn object_rope(o: &ScribbleObject, ws: &Workspace) -> Option<(String, u8)> {
    o.body.root.walk().find_map(|n| match &n.kind {
        NodeKind::Vector(v) => rope_texture(v, ws),
        _ => None,
    })
    .map(|t| (t, o.body.general.rope_segments))
}

/// One rope segment from `a` to `b` using `band` of the texture.
pub fn segment_quad(a: [f32; 2], b: [f32; 2], band: Band) -> TexQuad {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = (dx * dx + dy * dy).sqrt().max(1e-6);
    let t = [dx / len, dy / len];
    // Normal pointing "down" for a left-to-right rope (y down).
    let n = [-t[1], t[0]];
    let (r, w) = (ROPE_CAP, ROPE_HALF_WIDTH);
    let p = |base: [f32; 2], along: f32, across: f32| [base[0] + t[0] * along + n[0] * across, base[1] + t[1] * along + n[1] * across];
    let v0 = band.v_offset();
    let v1 = v0 + BAND_HEIGHT;
    TexQuad {
        pos: [p(a, -r, -w), p(b, r, -w), p(b, r, w), p(a, -r, w)],
        uv: [[0.0, v0], [1.0, v0], [1.0, v1], [0.0, v1]],
    }
}

/// Quads for a rope through `points` (one segment per consecutive pair); the first and last
/// segments use the start/end bands.
pub fn rope_quads(points: &[[f32; 2]]) -> Vec<TexQuad> {
    let n = points.len().saturating_sub(1);
    (0..n)
        .map(|i| {
            let band = if n == 1 {
                Band::Middle
            } else if i == 0 {
                Band::Start
            } else if i == n - 1 {
                Band::End
            } else {
                Band::Middle
            };
            segment_quad(points[i], points[i + 1], band)
        })
        .collect()
}

/// A free rope of `segments` segments centred on `center` along `axis`, sagging a little like a
/// rope held at both ends (a preview path; in game the rope is simulated).
pub fn sagging_path(center: [f32; 2], axis: [f32; 2], segments: usize) -> Vec<[f32; 2]> {
    let n = segments.max(1);
    let step = ROPE_SEGMENT_LENGTH * 0.96;
    let sag = 0.12 * n as f32 * ROPE_SEGMENT_LENGTH;
    let down = [-axis[1], axis[0]];
    (0..=n)
        .map(|i| {
            let s = (i as f32 - n as f32 / 2.0) * step;
            let k = 2.0 * i as f32 / n as f32 - 1.0;
            let d = if n > 1 { sag * (1.0 - k * k) } else { 0.0 };
            [center[0] + axis[0] * s + down[0] * d, center[1] + axis[1] * s + down[1] * d]
        })
        .collect()
}

/// A rope hanging from `start`, leaving it along `dir` and bending towards gravity (+y).
pub fn hanging_path(start: [f32; 2], dir: [f32; 2], segments: usize) -> Vec<[f32; 2]> {
    let mut pts = vec![start];
    let mut d = dir;
    for i in 0..segments.max(1) {
        if i > 0 {
            d = [d[0], d[1] + 0.35];
            let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
            d = [d[0] / l, d[1] / l];
        }
        let p = *pts.last().unwrap();
        pts.push([p[0] + d[0] * ROPE_SEGMENT_LENGTH, p[1] + d[1] * ROPE_SEGMENT_LENGTH]);
    }
    pts
}

fn fx(v: Fx12) -> f32 {
    v.to_f64() as f32
}

/// Walk a tree with object-space transforms (same composition as `object_scene`).
fn walk_world<'a>(n: &'a Node, parent: Affine, f: &mut dyn FnMut(&'a Node, Affine)) {
    let world = parent.then(Affine::translate(fx(n.x), fx(n.y))).then(Affine::rotate_degrees(fx(n.angle)));
    f(n, world);
    for c in &n.children {
        walk_world(c, world, f);
    }
}

/// Ropes an object's own tree makes the engine draw: textured rope nodes and rope anchors.
fn tree_ropes(o: &ScribbleObject, ws: &Workspace, out: &mut Resolved) {
    let mut found = Vec::new();
    // A textured vector node's rope is laid out in the frame of its rope child (type 12, which
    // holds the renderer: FUN_00680150); Christmas lights rotate the vector 180 degrees and the
    // rope node back.
    let mut pending: Option<(String, Affine)> = None;
    walk_world(&o.body.root, Affine::IDENTITY, &mut |n, world| match &n.kind {
        NodeKind::Vector(v) => {
            if let Some((tex, w)) = pending.take() {
                found.push((tex, None, w, None));
            }
            if let Some(tex) = rope_texture(v, ws) {
                pending = Some((tex, world));
            }
        }
        NodeKind::Rope(_) => {
            if let Some((tex, _)) = pending.take() {
                found.push((tex, None, world, None));
            }
        }
        NodeKind::Hotspot(h) if h.kind == 0 && !h.disabled => {
            let has_rope = h.fields.get("flags").and_then(|f| f.as_array()).is_some_and(|a| a.iter().any(|x| x == "has_rope"));
            if let (true, Some(rope)) = (has_rope, h.fields.get("rope").and_then(|r| r.as_str())) {
                let seg = h.fields.get("segments").and_then(|s| s.as_u64()).filter(|&s| s > 0).map(|s| s as usize);
                found.push((String::new(), Some(rope.to_string()), world, seg));
            }
        }
        _ => {}
    });
    found.extend(pending.map(|(tex, w)| (tex, None, w, None)));
    for (tex, rope_obj, world, seg) in found {
        let origin = world.apply([0.0, 0.0]);
        match rope_obj {
            None => {
                let n = o.body.general.rope_segments.max(1) as usize;
                let x = world.apply([1.0, 0.0]);
                let axis = [x[0] - origin[0], x[1] - origin[1]];
                out.ropes.push(RopeDraw { texture: tex.clone(), quads: rope_quads(&sagging_path(origin, axis, n)) });
                out.note(format!("rope: {n} segments of {}, drawn by the engine's rope renderer (no .vec)", file_stem(&tex)));
            }
            Some(path) => {
                let Some((tex, own_n)) = load_object(ws, &path).and_then(|r| object_rope(&r, ws)) else { continue };
                let n = seg.unwrap_or(own_n.max(1) as usize);
                // The rope leaves the anchor along the hotspot's local -y axis.
                let up = world.apply([0.0, -1.0]);
                let dir = [up[0] - origin[0], up[1] - origin[1]];
                out.ropes.push(RopeDraw { texture: tex.clone(), quads: rope_quads(&hanging_path(origin, dir, n)) });
                out.note(format!("rope anchor: {} x{n} ({}) spawned here", file_stem(&path), file_stem(&tex)));
            }
        }
    }
}

/// Resolve what to draw for the `.so` at `path`.
pub fn resolve(path: &str, o: &ScribbleObject, ws: &Workspace, words: Option<&WordIndex>) -> Resolved {
    let mut out = Resolved::own();
    let stem = file_stem(path);
    let placeholder = is_gift_placeholder(&o.body.root, ws);

    // Engine stand-ins: never spawned.
    let stand_in = match stem {
        "_adjective_adjective_adjective1" | "_adjective_adjective_adjective2" | "_adjective_adjective_adjective3" => Some(format!(
            "engine stand-in: relation target for the object's adjective slot {} (FUN_00658180); never spawned, not drawn",
            &stem[stem.len() - 1..]
        )),
        "_self_self_me" => Some("engine stand-in: \"me\" in filters = the owner itself (FUN_006766b0); never spawned, not drawn".into()),
        "_self_self_myobject" => Some("engine stand-in: \"myobject\" = the owner's type in filters (FUN_006766b0) and spawns (FUN_0053e890 substitutes the owner); never spawned itself, not drawn".into()),
        "_stage_stage_stageobject" => Some("engine stand-in: relation target meaning a stage position (FUN_00655d60); never spawned, not drawn".into()),
        _ => None,
    };
    if let Some(n) = stand_in {
        out.body = Body::Nothing;
        out.note = Some(n);
        return out;
    }

    tree_ropes(o, ws, &mut out);

    if let Some(name) = stem.strip_prefix("human_player_avatars__")
        && placeholder
    {
        match avatar_counterpart(path, name, ws, words) {
            Some((word, cpath, object)) => {
                out.note(format!(
                    "avatar placeholder (hidden word {word}, no avatar art in the game): shown as {} -> {}.so",
                    word.trim_start_matches('@'),
                    file_stem(&cpath)
                ));
                out.body = Body::Object { path: cpath, object: Box::new(object) };
            }
            None => out.note("avatar placeholder: gift.vec stands in for art the game does not ship".into()),
        }
        return out;
    }

    if let Some(piece) = stem.strip_prefix("tool_rope_pieces__")
        && out.ropes.is_empty()
        && (placeholder || drawn_vectors(&o.body.root).is_empty())
    {
        resolve_piece(piece, ws, &mut out);
        return out;
    }

    if placeholder && stem != "misc_paper_thick_gift" {
        out.note("placeholder art: gift.vec; nothing in the game spawns this object, so the gift is all it has".into());
    }
    out
}

fn resolve_piece(piece: &str, ws: &Workspace, out: &mut Resolved) {
    let obj_path = |stem: &str| format!("{OBJECT_DIR}{stem}.so");
    if let Some(&(_, owner)) = PIECE_OBJECTS.iter().find(|(p, _)| *p == piece)
        && let Some(object) = load_object(ws, &obj_path(owner))
    {
        out.note(format!("rope piece: the game draws this as {owner}.so"));
        out.body = Body::Object { path: obj_path(owner), object: Box::new(object) };
        return;
    }
    let (base, band) = match piece.strip_suffix("end") {
        Some(b) if ROPE_PIECES.iter().any(|(p, _)| *p == b) => (b, Band::End),
        _ => (piece, Band::Middle),
    };
    let Some(&(_, owner)) = ROPE_PIECES.iter().find(|(p, _)| *p == base) else {
        out.body = Body::Nothing;
        out.note("rope piece: legacy segment object; no textured rope in the game matches it (rope_texture none)".into());
        return;
    };
    let Some((tex, _)) = load_object(ws, &obj_path(owner)).and_then(|o| object_rope(&o, ws)) else { return };
    let half = ROPE_SEGMENT_LENGTH / 2.0;
    out.ropes.push(RopeDraw { texture: tex.clone(), quads: vec![segment_quad([-half, 0.0], [half, 0.0], band)] });
    out.body = Body::Nothing;
    let which = if band == Band::End { "end" } else { "middle" };
    out.note(format!("rope segment: drawn by {owner}.so's rope ({}, {which} band); this legacy piece is never spawned", file_stem(&tex)));
}

/// The object the avatar's word spawns once its hidden-word `@` is dropped.
fn avatar_counterpart(path: &str, name: &str, ws: &Workspace, words: Option<&WordIndex>) -> Option<(String, String, ScribbleObject)> {
    let usable = |p: &str| p.ends_with(".so") && !p.contains("human_player_avatars__");
    if let Some(ix) = words {
        for w in ix.words_for(path).iter().filter(|w| w.starts_with('@')) {
            let cands: Vec<&String> = ix.objects_for(&w[1..]).iter().filter(|p| usable(p)).collect();
            // Prefer the candidate with the avatar's own object name (HERO also spawns a sandwich).
            let best = cands.iter().find(|p| file_stem(p).rsplit('_').next() == Some(name)).or(cands.first());
            if let Some(p) = best
                && let Some(o) = load_object(ws, p)
            {
                return Some((w.clone(), (*p).clone(), o));
            }
        }
    }
    // No plain twin word (`@HAIR DRESSER`): the object with the same object name.
    let f = ws.manifest.files.iter().find(|f| {
        let s = file_stem(&f.path);
        f.path.ends_with(".so") && usable(&f.path) && f.pack.is_some() && s.rsplit('_').next() == Some(name) && !s.contains("__")
    })?;
    let word = words.and_then(|ix| ix.words_for(path).first().cloned()).unwrap_or_else(|| format!("@{}", name.to_uppercase()));
    load_object(ws, &f.path).map(|o| (word, f.path.clone(), o))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_quad_covers_segment_plus_caps() {
        let q = segment_quad([0.0, 0.0], [16.0, 0.0], Band::Middle);
        assert_eq!(q.pos[0], [-ROPE_CAP, -ROPE_HALF_WIDTH]);
        assert_eq!(q.pos[2], [16.0 + ROPE_CAP, ROPE_HALF_WIDTH]);
        assert_eq!(q.uv[2], [1.0, BAND_HEIGHT]);
    }

    #[test]
    fn rope_bands() {
        let pts = sagging_path([0.0, 0.0], [1.0, 0.0], 4);
        let q = rope_quads(&pts);
        assert_eq!(q.len(), 4);
        assert_eq!(q[0].uv[0][1], 0.4);
        assert_eq!(q[1].uv[0][1], 0.0);
        assert_eq!(q[3].uv[0][1], 0.8);
    }
}
