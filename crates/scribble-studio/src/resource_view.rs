//! Generic view of one resource: JSON editor for game formats, preview for textures,
//! and a summary for other standard files.

use crate::{texture, workspace::Workspace};
use eframe::egui;
use scribble_core::Codec;
use scribble_formats::{handler_for, Handler};

pub struct ResourceView {
    pub index: usize,
    kind: Kind,
    status: Option<(bool, String)>,
}

enum Kind {
    Vec { view: Box<crate::vec_view::VecView>, codec: &'static dyn Codec, text: String, saved_text: String, show_json: bool },
    Json { codec: &'static dyn Codec, text: String, saved_text: String },
    Texture { tex: egui::TextureHandle, size: [usize; 2] },
    Text(String),
    Other { description: String, len: usize },
}

impl ResourceView {
    pub fn open(ctx: &egui::Context, ws: &Workspace, index: usize) -> Self {
        let mut status = None;
        let kind = match Self::load(ctx, ws, index) {
            Ok(k) => k,
            Err(e) => {
                status = Some((false, format!("{e:#}")));
                Kind::Other { description: "could not decode".into(), len: 0 }
            }
        };
        ResourceView { index, kind, status }
    }

    fn load(ctx: &egui::Context, ws: &Workspace, index: usize) -> anyhow::Result<Kind> {
        let data = ws.read(index)?;
        let path = &ws.file(index).path;
        Ok(match handler_for(path, &data) {
            Some(Handler::Codec(codec)) if codec.name() == "vec" => {
                let art = <fmt_vec::VectorArt as scribble_core::Format>::decode(&data, &ws.ctx)?;
                let text = codec.decode_text(&data, &ws.ctx)?;
                Kind::Vec { view: Box::new(crate::vec_view::VecView::new(art)), codec, saved_text: text.clone(), text, show_json: false }
            }
            Some(Handler::Codec(codec)) => {
                let text = codec.decode_text(&data, &ws.ctx)?;
                Kind::Json { codec, saved_text: text.clone(), text }
            }
            Some(Handler::Standard { extension: "dds", .. }) => {
                let img = texture::decode_dds(&data)?;
                let size = img.size;
                Kind::Texture { tex: ctx.load_texture(path, img, egui::TextureOptions::LINEAR), size }
            }
            Some(Handler::Standard { extension: "gp", .. }) => Kind::Text(String::from_utf8_lossy(&data).into_owned()),
            Some(Handler::Standard { description, .. }) => Kind::Other { description: description.to_string(), len: data.len() },
            None => Kind::Other { description: "no codec yet".into(), len: data.len() },
        })
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ws: &Workspace) {
        let f = ws.file(self.index);
        ui.heading(&f.path);
        ui.label(format!("resource #{}  ·  pack {}  ·  {}", f.index, f.pack.as_deref().unwrap_or("-"), if f.compressed { "zlib" } else { "stored" }));
        if let Some((ok, msg)) = &self.status {
            ui.colored_label(if *ok { egui::Color32::from_rgb(80, 170, 80) } else { egui::Color32::from_rgb(220, 80, 80) }, msg);
        }
        ui.separator();
        match &mut self.kind {
            Kind::Vec { view, codec, text, saved_text, show_json } => {
                ui.horizontal(|ui| {
                    ui.selectable_value(show_json, false, "Preview");
                    ui.selectable_value(show_json, true, "JSON");
                });
                if !*show_json {
                    view.ui(ui);
                } else {
                    let before = saved_text.clone();
                    json_editor(ui, ws, self.index, *codec, text, saved_text, &mut self.status);
                    if *saved_text != before
                        && let Ok(art) = serde_json::from_str::<fmt_vec::VectorArt>(saved_text)
                    {
                        view.art = art;
                    }
                }
            }
            Kind::Json { codec, text, saved_text } => json_editor(ui, ws, self.index, *codec, text, saved_text, &mut self.status),
            Kind::Texture { tex, size } => {
                ui.label(format!("{} x {}", size[0], size[1]));
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.add(egui::Image::new(&*tex).bg_fill(egui::Color32::from_gray(40)));
                });
            }
            Kind::Text(t) => {
                egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut t.as_str()).code_editor().desired_width(f32::INFINITY));
                });
            }
            Kind::Other { description, len } => {
                ui.label(format!("{description} ({len} bytes)"));
            }
        }
    }
}

/// Text editor over a codec's JSON with Save (encode + write) and Revert.
fn json_editor(
    ui: &mut egui::Ui,
    ws: &Workspace,
    index: usize,
    codec: &'static dyn Codec,
    text: &mut String,
    saved_text: &mut String,
    status: &mut Option<(bool, String)>,
) {
    ui.horizontal(|ui| {
        ui.label(format!("{} — {}", codec.name(), codec.description()));
        let dirty = text != saved_text;
        if ui.add_enabled(dirty, egui::Button::new("Save")).clicked() {
            *status = Some(match save(ws, index, codec, text) {
                Ok(n) => {
                    *saved_text = text.clone();
                    (true, format!("saved ({n} bytes)"))
                }
                Err(e) => (false, format!("{e:#}")),
            });
        }
        if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
            *text = saved_text.clone();
            *status = None;
        }
    });
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        ui.add(egui::TextEdit::multiline(text).code_editor().desired_width(f32::INFINITY));
    });
}

fn save(ws: &Workspace, index: usize, codec: &dyn Codec, text: &str) -> anyhow::Result<usize> {
    let bytes = codec.encode_text(text, &ws.ctx)?;
    // Refuse to write something the decoder would not read back.
    codec.decode_text(&bytes, &ws.ctx)?;
    ws.write(index, &bytes)?;
    Ok(bytes.len())
}
