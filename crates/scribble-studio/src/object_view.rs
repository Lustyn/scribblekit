//! Object viewer and editor: browse scribble objects by name, see them assembled from their
//! vector art and animated, and edit their properties.

use crate::object_resolve::{self, Resolved, WordIndex};
use crate::object_scene::{self, Overlay, Scene, VecSource};
use crate::workspace::Workspace;
use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Shape, Stroke, Vec2};
use fmt_anim::Animation;
use fmt_object::tree::Node;
use fmt_object::{ScribbleObject, SimpleObject};
use fmt_vec::VectorArt;
use scribble_core::{Codec, Format, ResRef};
use std::collections::HashMap;
use std::sync::Arc;

/// An entry of the object list.
struct Entry {
    /// Index into the workspace manifest.
    file: usize,
    /// Words that spawn this object (english), or empty.
    words: Vec<String>,
    path: String,
    label: String,
}

enum Obj {
    Scribble(Box<ScribbleObject>),
    Simple(Box<SimpleObject>),
}

impl Obj {
    fn root(&self, female: bool) -> Option<&Node> {
        match self {
            Obj::Scribble(o) => Some(match (&o.female_body, female) {
                (Some(b), true) => &b.root,
                _ => &o.body.root,
            }),
            Obj::Simple(o) => o.root.as_ref(),
        }
    }

    /// `(slot name, anim resource)` pairs.
    fn animations(&self, female: bool) -> Vec<(String, Option<ResRef>)> {
        match self {
            Obj::Scribble(o) => {
                let body = match (&o.female_body, female) {
                    (Some(b), true) => b,
                    _ => &o.body,
                };
                body.animations
                    .iter()
                    .flat_map(|t| t.entries.iter())
                    .map(|e| (fmt_anim::slot_name(e.slot.0).map(str::to_string).unwrap_or(format!("slot {}", e.slot.0)), e.anim.clone()))
                    .collect()
            }
            Obj::Simple(o) => o.idle_animation.iter().map(|a| ("idle".to_string(), Some(a.clone()))).collect(),
        }
    }
}

struct Loaded {
    file: usize,
    codec: &'static dyn Codec,
    obj: Option<Obj>,
    /// Edit buffer and last saved state.
    value: serde_json::Value,
    saved: serde_json::Value,
    parse_error: Option<String>,
    female: bool,
    anims: Vec<(String, Option<ResRef>)>,
    anim: Option<usize>,
    playing: bool,
    frame: f64,
    speed: f64,
    /// Rest-pose bounds, used to frame the view so animation doesn't move the camera.
    frame_bounds: Option<([f32; 2], [f32; 2])>,
    show_json: bool,
    json_text: String,
    /// What the game really draws for this object (placeholders, ropes); computed lazily.
    resolved: Option<Resolved>,
    /// The body toggle was initialised from the resolved object's default gender.
    gender_chosen: bool,
}

pub struct ObjectViewer {
    entries: Option<Vec<Entry>>,
    /// English words <-> objects, built with the entries.
    word_index: Option<Arc<WordIndex>>,
    /// DDS textures (rope textures) by logical path.
    textures: HashMap<String, Option<egui::TextureHandle>>,
    query: String,
    filtered: Vec<usize>,
    filter_stale: bool,
    current: Option<Loaded>,
    vec_cache: HashMap<String, Option<Arc<VectorArt>>>,
    anim_cache: HashMap<String, Option<Arc<Animation>>>,
    show_shapes: bool,
    show_hotspots: bool,
    zoom: f32,
    pan: Vec2,
    status: Option<(bool, String)>,
}

impl Default for ObjectViewer {
    fn default() -> Self {
        ObjectViewer {
            entries: None,
            word_index: None,
            textures: HashMap::new(),
            query: String::new(),
            filtered: Vec::new(),
            filter_stale: true,
            current: None,
            vec_cache: HashMap::new(),
            anim_cache: HashMap::new(),
            show_shapes: false,
            show_hotspots: false,
            zoom: 1.0,
            pan: Vec2::ZERO,
            status: None,
        }
    }
}

struct Vecs<'a> {
    ws: &'a Workspace,
    cache: &'a mut HashMap<String, Option<Arc<VectorArt>>>,
}

fn res_name(r: &ResRef, ws: &Workspace) -> Option<String> {
    match r {
        ResRef::Path(p) => Some(p.clone()),
        ResRef::Index(i) => ws.ctx.name(*i).map(str::to_string),
    }
}

impl VecSource for Vecs<'_> {
    fn vector(&mut self, r: &ResRef) -> Option<Arc<VectorArt>> {
        let name = res_name(r, self.ws)?;
        self.cache
            .entry(name.clone())
            .or_insert_with(|| {
                let data = self.ws.read_logical(&name).ok()?;
                VectorArt::decode(&data, &self.ws.ctx).ok().map(Arc::new)
            })
            .clone()
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

impl ObjectViewer {
    fn build_entries(ws: &Workspace) -> (Vec<Entry>, WordIndex) {
        let index = fmt_dictionary::Dictionary::load_dir(&ws.root, "english", &ws.ctx).map(|d| WordIndex::from_dictionary(&d)).unwrap_or_default();
        let mut out: Vec<Entry> = ws
            .present
            .iter()
            .filter_map(|&i| {
                let f = ws.file(i);
                let lower = f.path.to_lowercase();
                if !(lower.ends_with(".so") || lower.ends_with(".sao")) {
                    return None;
                }
                let w = index.words_for(&f.path).to_vec();
                let label = match w.first() {
                    Some(first) if w.len() > 1 => format!("{first} (+{})  ·  {}", w.len() - 1, file_name(&f.path)),
                    Some(first) => format!("{first}  ·  {}", file_name(&f.path)),
                    None => file_name(&f.path).to_string(),
                };
                Some(Entry { file: i, words: w, path: f.path.clone(), label })
            })
            .collect();
        out.sort_by(|a, b| (a.words.is_empty(), &a.label).cmp(&(b.words.is_empty(), &b.label)));
        (out, index)
    }

    /// Open an object by logical path.
    pub fn open(&mut self, ws: &Workspace, logical: &str) {
        let Some(file) = ws.find(logical) else {
            self.status = Some((false, format!("no resource {logical}")));
            return;
        };
        self.status = None;
        let data = match ws.read(file) {
            Ok(d) => d,
            Err(e) => {
                self.status = Some((false, format!("{e:#}")));
                return;
            }
        };
        let Some(scribble_formats::Handler::Codec(codec)) = scribble_formats::handler_for(logical, &data) else {
            self.status = Some((false, format!("{logical} is not an object")));
            return;
        };
        let value = match codec.decode_json(&data, &ws.ctx) {
            Ok(v) => v,
            Err(e) => {
                self.status = Some((false, format!("decode failed: {e:#}")));
                return;
            }
        };
        let mut l = Loaded {
            file,
            codec,
            obj: None,
            saved: value.clone(),
            value,
            parse_error: None,
            female: false,
            anims: Vec::new(),
            anim: None,
            playing: true,
            frame: 0.0,
            speed: 1.0,
            frame_bounds: None,
            show_json: false,
            json_text: String::new(),
            resolved: None,
            gender_chosen: false,
        };
        l.obj = parse_obj(codec, &l.value, &mut l.parse_error);
        self.refresh_anims(&mut l);
        self.current = Some(l);
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
    }

    /// Choose an animation slot by name (`None` = rest pose) at a fixed frame (tests, scripting).
    #[doc(hidden)]
    pub fn set_pose(&mut self, slot: Option<&str>, frame: f64, shapes: bool, hotspots: bool) {
        self.show_shapes = shapes;
        self.show_hotspots = hotspots;
        if let Some(l) = &mut self.current {
            l.anim = slot.and_then(|s| l.anims.iter().position(|(n, _)| n == s));
            l.frame = frame;
            l.playing = false;
        }
    }

    /// The last status or error message, and the current object's parse error.
    pub fn messages(&self) -> (Option<&str>, Option<&str>) {
        (self.status.as_ref().map(|s| s.1.as_str()), self.current.as_ref().and_then(|l| l.parse_error.as_deref()))
    }

    /// Whether the open object has unsaved edits.
    pub fn is_dirty(&self) -> bool {
        self.current.as_ref().is_some_and(|l| l.value != l.saved)
    }

    /// Edit the open object's JSON programmatically (tests, scripting); the preview updates.
    #[doc(hidden)]
    pub fn edit_value(&mut self, f: impl FnOnce(&mut serde_json::Value)) {
        if let Some(l) = &mut self.current {
            f(&mut l.value);
            l.obj = parse_obj(l.codec, &l.value, &mut l.parse_error);
            l.frame_bounds = None;
            l.resolved = None;
        }
    }

    fn refresh_anims(&self, l: &mut Loaded) {
        l.anims = l.obj.as_ref().map(|o| o.animations(l.female)).unwrap_or_default();
        let idle = l.anims.iter().position(|(n, _)| n == "idle");
        l.anim = idle.or(if l.anims.is_empty() { None } else { Some(0) });
        l.frame = 0.0;
        l.frame_bounds = None;
    }

    fn animation(&mut self, ws: &Workspace, r: &ResRef) -> Option<Arc<Animation>> {
        let name = res_name(r, ws)?;
        self.anim_cache
            .entry(name.clone())
            .or_insert_with(|| {
                let data = ws.read_logical(&name).ok()?;
                Animation::decode(&data, &ws.ctx).ok().map(Arc::new)
            })
            .clone()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ws: &Workspace) {
        if self.entries.is_none() {
            let (entries, index) = Self::build_entries(ws);
            self.entries = Some(entries);
            self.word_index = Some(Arc::new(index));
        }
        egui::Panel::left("object_list").default_size(340.0).show(ui, |ui| self.list_ui(ui, ws));
        if self.current.is_some() {
            egui::Panel::right("object_props").default_size(460.0).show(ui, |ui| self.properties_ui(ui, ws));
        }
        egui::CentralPanel::default().show(ui, |ui| self.canvas_ui(ui, ws));
    }

    fn list_ui(&mut self, ui: &mut egui::Ui, ws: &Workspace) {
        if ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("search objects (COW, mammal, hat…)").desired_width(f32::INFINITY)).changed() {
            self.filter_stale = true;
        }
        let entries = self.entries.as_ref().unwrap();
        if self.filter_stale {
            let q = self.query.to_lowercase();
            self.filtered = entries
                .iter()
                .enumerate()
                .filter(|(_, e)| q.is_empty() || e.label.to_lowercase().contains(&q) || e.words.iter().any(|w| w.to_lowercase().contains(&q)))
                .map(|(i, _)| i)
                .collect();
            self.filter_stale = false;
        }
        ui.label(format!("{} objects", self.filtered.len()));
        let row_h = ui.text_style_height(&egui::TextStyle::Body);
        let mut open = None;
        let current = self.current.as_ref().map(|c| c.file);
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, self.filtered.len(), |ui, range| {
            for &i in &self.filtered[range] {
                let e = &entries[i];
                if ui.selectable_label(current == Some(e.file), &e.label).on_hover_text(&e.path).clicked() {
                    open = Some(e.path.clone());
                }
            }
        });
        if let Some(p) = open {
            self.open(ws, &p);
        }
    }

    fn canvas_ui(&mut self, ui: &mut egui::Ui, ws: &Workspace) {
        let Some(mut l) = self.current.take() else {
            ui.centered_and_justified(|ui| ui.label(self.status.as_ref().map(|s| s.1.as_str()).unwrap_or("Pick an object on the left.")));
            return;
        };
        let f = ws.file(l.file);
        ui.heading(file_name(&f.path));
        if let Some(e) = self.entries.as_ref().and_then(|es| es.iter().find(|e| e.file == l.file))
            && !e.words.is_empty()
        {
            ui.label(format!("typed as: {}", e.words.join(", ")));
        }
        if l.resolved.is_none() {
            l.resolved = Some(match &l.obj {
                Some(Obj::Scribble(o)) => object_resolve::resolve(&f.path, o, ws, self.word_index.as_deref()),
                _ => Resolved::own(),
            });
            if !l.gender_chosen {
                l.gender_chosen = true;
                // The avatar's own animation table has no female variant, so the slots stay.
                if l.resolved.as_ref().is_some_and(Resolved::prefers_female) {
                    l.female = true;
                    l.frame_bounds = None;
                }
            }
        }
        let has_female = matches!(&l.obj, Some(Obj::Scribble(o)) if o.female_body.is_some()) || l.resolved.as_ref().is_some_and(Resolved::has_female_body);
        // Toolbar: body, animation, playback, overlays.
        ui.horizontal_wrapped(|ui| {
            if has_female {
                let before = l.female;
                ui.selectable_value(&mut l.female, false, "male body");
                ui.selectable_value(&mut l.female, true, "female body");
                if l.female != before {
                    self.refresh_anims(&mut l);
                }
            }
            let current = l.anim.and_then(|i| l.anims.get(i)).map(|a| a.0.clone()).unwrap_or("rest pose".into());
            egui::ComboBox::from_id_salt("anim").selected_text(current).show_ui(ui, |ui| {
                if ui.selectable_label(l.anim.is_none(), "rest pose").clicked() {
                    l.anim = None;
                }
                for (i, (name, _)) in l.anims.iter().enumerate() {
                    if ui.selectable_label(l.anim == Some(i), name).clicked() {
                        l.anim = Some(i);
                        l.frame = 0.0;
                    }
                }
            });
            if ui.button(if l.playing { "⏸" } else { "▶" }).clicked() {
                l.playing = !l.playing;
            }
            ui.add(egui::Slider::new(&mut l.speed, 0.1..=3.0).text("speed"));
            ui.checkbox(&mut self.show_shapes, "physics shapes");
            ui.checkbox(&mut self.show_hotspots, "hotspots");
            if ui.button("reset view").clicked() {
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
        });

        let anim = l.anim.and_then(|i| l.anims.get(i)).and_then(|(_, r)| r.clone()).and_then(|r| self.animation(ws, &r));
        if let Some(a) = &anim {
            let dur = a.duration_frames().max(1.0);
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut l.frame, 0.0..=dur).text(format!("frame / {dur:.0}")));
            });
            if l.playing {
                l.frame += ui.input(|i| i.stable_dt) as f64 * fmt_anim::FRAMES_PER_SECOND * l.speed;
                if l.frame > dur {
                    l.frame = if a.looping { l.frame % dur } else { 0.0 };
                }
                ui.ctx().request_repaint();
            }
        }

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());
        if response.dragged() {
            self.pan += response.drag_delta();
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(0.05, 40.0);
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_rgb(222, 226, 232));

        let resolved = l.resolved.take().unwrap_or_else(Resolved::own);
        self.load_textures(ui.ctx(), ws, &resolved);
        let root = resolved.root(l.obj.as_ref().and_then(|o| o.root(l.female)), l.female);
        if root.is_some() || !resolved.ropes.is_empty() {
            let mut vecs = Vecs { ws, cache: &mut self.vec_cache };
            if l.frame_bounds.is_none() {
                let rest = root.and_then(|r| object_scene::build(r, &mut vecs, None).bounds());
                l.frame_bounds = object_resolve::union_bounds(rest, resolved.rope_bounds());
            }
            let pose = anim.as_ref().map(|a| a.sample(l.frame));
            let scene = root.map(|r| object_scene::build(r, &mut vecs, pose.as_ref())).unwrap_or_default();
            let view = fit(l.frame_bounds.or(scene.bounds()), rect, self.zoom, self.pan);
            paint_ropes(&painter, &resolved, &self.textures, &view);
            paint_scene(&painter, &scene, &view, self.show_shapes, self.show_hotspots);
            if !scene.missing.is_empty() {
                painter.text(rect.left_bottom() + egui::vec2(6.0, -6.0), egui::Align2::LEFT_BOTTOM, format!("missing: {}", scene.missing.join(", ")), egui::FontId::proportional(12.0), Color32::DARK_RED);
            }
        } else if let Some(e) = &l.parse_error {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(14.0), Color32::DARK_RED);
        }
        if let Some(note) = &resolved.note {
            let galley = painter.layout(note.clone(), egui::FontId::proportional(13.0), Color32::from_rgb(30, 60, 140), rect.width() - 16.0);
            painter.galley(rect.left_top() + egui::vec2(8.0, 8.0), galley, Color32::from_rgb(30, 60, 140));
        }
        l.resolved = Some(resolved);
        self.current = Some(l);
    }

    /// Upload the DDS textures the resolved ropes use.
    fn load_textures(&mut self, ctx: &egui::Context, ws: &Workspace, r: &Resolved) {
        for rope in &r.ropes {
            self.textures.entry(rope.texture.clone()).or_insert_with(|| {
                let data = ws.read_logical(&rope.texture).ok()?;
                let img = crate::texture::decode_dds(&data).ok()?;
                Some(ctx.load_texture(rope.texture.clone(), img, egui::TextureOptions::LINEAR))
            });
        }
    }

    /// The canvas note for the open object (what its art was resolved from), if any.
    pub fn resolution_note(&self) -> Option<&str> {
        self.current.as_ref().and_then(|l| l.resolved.as_ref()).and_then(|r| r.note.as_deref())
    }

    fn properties_ui(&mut self, ui: &mut egui::Ui, ws: &Workspace) {
        let Some(l) = &mut self.current else { return };
        ui.horizontal(|ui| {
            let dirty = l.value != l.saved;
            if ui.add_enabled(dirty && l.parse_error.is_none(), egui::Button::new("Save object")).clicked() {
                self.status = Some(match l.codec.encode_json(l.value.clone(), &ws.ctx).and_then(|b| {
                    l.codec.decode_json(&b, &ws.ctx)?;
                    ws.write(l.file, &b)?;
                    Ok(b.len())
                }) {
                    Ok(n) => {
                        l.saved = l.value.clone();
                        (true, format!("saved ({n} bytes)"))
                    }
                    Err(e) => (false, format!("save failed: {e:#}")),
                });
            }
            if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                l.value = l.saved.clone();
                l.obj = parse_obj(l.codec, &l.value, &mut l.parse_error);
                l.frame_bounds = None;
                l.resolved = None;
            }
            ui.separator();
            if ui.selectable_label(!l.show_json, "Properties").clicked() {
                l.show_json = false;
            }
            if ui.selectable_label(l.show_json, "JSON").clicked() {
                l.show_json = true;
                l.json_text = scribble_core::json::to_string(&l.value);
            }
        });
        if let Some((ok, msg)) = &self.status {
            ui.colored_label(if *ok { Color32::from_rgb(80, 170, 80) } else { Color32::from_rgb(220, 80, 80) }, msg);
        }
        if let Some(e) = &l.parse_error {
            ui.colored_label(Color32::from_rgb(220, 80, 80), e);
        }
        ui.separator();
        let changed = if l.show_json {
            let mut changed = false;
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                if ui.add(egui::TextEdit::multiline(&mut l.json_text).code_editor().desired_width(f32::INFINITY)).changed() {
                    match serde_json::from_str(&l.json_text) {
                        Ok(v) => {
                            l.value = v;
                            changed = true;
                        }
                        Err(e) => l.parse_error = Some(format!("JSON: {e}")),
                    }
                }
            });
            changed
        } else {
            // Scroll both ways so wide rows never widen the panel.
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| crate::value_editor::edit(ui, egui::Id::new(("obj", l.file)), &mut l.value)).inner
        };
        if changed {
            l.obj = parse_obj(l.codec, &l.value, &mut l.parse_error);
            l.anims = l.obj.as_ref().map(|o| o.animations(l.female)).unwrap_or_default();
            l.frame_bounds = None;
            l.resolved = None;
        }
    }
}

fn parse_obj(codec: &dyn Codec, v: &serde_json::Value, err: &mut Option<String>) -> Option<Obj> {
    let r = match codec.name() {
        "so" => serde_json::from_value::<ScribbleObject>(v.clone()).map(|o| Obj::Scribble(Box::new(o))),
        "sao" => serde_json::from_value::<SimpleObject>(v.clone()).map(|o| Obj::Simple(Box::new(o))),
        other => {
            *err = Some(format!("{other} has no visual"));
            return None;
        }
    };
    match r {
        Ok(o) => {
            *err = None;
            Some(o)
        }
        Err(e) => {
            *err = Some(format!("invalid object: {e}"));
            None
        }
    }
}

/// Object space -> screen.
pub struct View {
    scale: f32,
    origin: Pos2,
}

impl View {
    fn map(&self, p: [f32; 2]) -> Pos2 {
        Pos2::new(self.origin.x + p[0] * self.scale, self.origin.y + p[1] * self.scale)
    }
}

pub fn fit(bounds: Option<([f32; 2], [f32; 2])>, rect: Rect, zoom: f32, pan: Vec2) -> View {
    let (lo, hi) = bounds.unwrap_or(([-1.0, -1.0], [1.0, 1.0]));
    let size = [(hi[0] - lo[0]).max(0.01), (hi[1] - lo[1]).max(0.01)];
    let scale = (rect.width() / size[0]).min(rect.height() / size[1]) * 0.8 * zoom;
    let center = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    View { scale, origin: rect.center() + pan - egui::vec2(center[0] * scale, center[1] * scale) }
}

/// Textured ropes (behind the scene).
fn paint_ropes(painter: &egui::Painter, resolved: &Resolved, textures: &HashMap<String, Option<egui::TextureHandle>>, view: &View) {
    for r in &resolved.ropes {
        let Some(Some(tex)) = textures.get(&r.texture) else { continue };
        let mut mesh = Mesh::with_texture(tex.id());
        for q in &r.quads {
            let base = mesh.vertices.len() as u32;
            for k in 0..4 {
                mesh.vertices.push(egui::epaint::Vertex { pos: view.map(q.pos[k]), uv: Pos2::new(q.uv[k][0], q.uv[k][1]), color: Color32::WHITE });
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        painter.add(Shape::mesh(mesh));
    }
}

pub fn paint_scene(painter: &egui::Painter, scene: &Scene, view: &View, shapes: bool, hotspots: bool) {
    let mut mesh = Mesh::default();
    for piece in &scene.pieces {
        for t in &piece.triangles {
            for k in 0..3 {
                let [r, g, b, a] = t.color[k];
                mesh.colored_vertex(view.map(t.pos[k]), Color32::from_rgba_unmultiplied(r, g, b, a));
            }
            let n = mesh.vertices.len() as u32;
            mesh.add_triangle(n - 3, n - 2, n - 1);
        }
    }
    painter.add(Shape::mesh(mesh));
    for o in &scene.overlays {
        match o {
            Overlay::Polygon { points, kind } if shapes => {
                let color = if *kind == "hit_box" { Color32::from_rgb(40, 120, 220) } else { Color32::from_rgb(20, 160, 60) };
                painter.add(Shape::closed_line(points.iter().map(|&p| view.map(p)).collect(), Stroke::new(1.5, color)));
            }
            Overlay::Circle { center, radius } if shapes => {
                painter.circle_stroke(view.map(*center), radius * view.scale, Stroke::new(1.5, Color32::from_rgb(20, 160, 60)));
            }
            Overlay::Point { at, label } if hotspots => {
                let p = view.map(*at);
                painter.circle_filled(p, 3.5, Color32::from_rgb(220, 60, 60));
                painter.text(p + egui::vec2(5.0, -5.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(11.0), Color32::from_rgb(120, 20, 20));
            }
            _ => {}
        }
    }
}
