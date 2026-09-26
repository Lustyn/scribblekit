//! Interactive preview of `.vec` vector art, drawn directly as egui meshes.

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Shape, Stroke, Vec2};
use fmt_vec::VectorArt;

pub struct VecView {
    pub art: VectorArt,
    /// Only draw this bone's part.
    pub solo_bone: Option<u16>,
    pub show_outlines: bool,
    zoom: f32,
    pan: Vec2,
}

impl VecView {
    pub fn new(art: VectorArt) -> Self {
        VecView { art, solo_bone: None, show_outlines: false, zoom: 1.0, pan: Vec2::ZERO }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(format!(
                "{} x {} · {} vertices · {} triangles · {} parts",
                self.art.width,
                self.art.height,
                self.art.vertices.len(),
                self.art.triangles.len(),
                self.art.parts.len()
            ));
            ui.checkbox(&mut self.show_outlines, "outlines");
            egui::ComboBox::from_id_salt("solo")
                .selected_text(match self.solo_bone {
                    Some(b) => format!("bone {b}"),
                    None => "all parts".into(),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.solo_bone, None, "all parts");
                    for p in &self.art.parts {
                        ui.selectable_value(&mut self.solo_bone, Some(p.bone), format!("bone {}", p.bone));
                    }
                });
            if ui.button("reset view").clicked() {
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
        });
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());
        if response.dragged() {
            self.pan += response.drag_delta();
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.zoom = (self.zoom * (1.0 + scroll * 0.002)).clamp(0.1, 50.0);
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, checker_bg());
        paint_art(&painter, &self.art, fit_rect(&self.art, rect, self.zoom, self.pan), self.solo_bone, self.show_outlines);
    }
}

fn checker_bg() -> Color32 {
    Color32::from_gray(200)
}

/// The screen rect the art's normalised `[-0.5, 0.5]` square maps to, preserving aspect.
pub fn fit_rect(art: &VectorArt, avail: Rect, zoom: f32, pan: Vec2) -> Rect {
    let aspect = if art.height > 0.0 { art.width / art.height } else { 1.0 };
    let mut size = avail.size() * 0.9;
    if size.x / size.y > aspect {
        size.x = size.y * aspect;
    } else {
        size.y = size.x / aspect;
    }
    Rect::from_center_size(avail.center() + pan, size * zoom)
}

/// Draw the art's triangles into `target` (the art's normalised square).
pub fn paint_art(painter: &egui::Painter, art: &VectorArt, target: Rect, solo: Option<u16>, outlines: bool) {
    let map = |p: [f32; 2]| Pos2::new(target.center().x + p[0] * target.width(), target.center().y + p[1] * target.height());
    let tris: Vec<_> = match solo {
        Some(b) => art.part_mesh(b).map(|m| m.triangles).unwrap_or_default(),
        None => art.draw_triangles(),
    };
    let mut mesh = Mesh::default();
    for t in &tris {
        for v in &t.vertices {
            let [r, g, b, a] = v.color;
            mesh.colored_vertex(map(v.position), Color32::from_rgba_unmultiplied(r, g, b, a));
        }
        let n = mesh.vertices.len() as u32;
        mesh.add_triangle(n - 3, n - 2, n - 1);
    }
    painter.add(Shape::mesh(mesh));
    if outlines {
        let bones: Vec<u16> = match solo {
            Some(b) => vec![b],
            None => art.parts.iter().map(|p| p.bone).collect(),
        };
        for b in bones {
            for line in art.outline_polylines(b) {
                painter.add(Shape::closed_line(line.into_iter().map(map).collect(), Stroke::new(1.0, Color32::RED)));
            }
        }
    }
}
