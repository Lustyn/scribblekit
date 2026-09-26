//! Searchable list of every resource in the workspace.

use crate::workspace::Workspace;
use eframe::egui;

#[derive(Default)]
pub struct Browser {
    pub query: String,
    pub selected: Option<usize>,
    filtered: Vec<usize>,
    last_query: Option<String>,
}

impl Browser {
    /// Draw the browser; returns the newly selected manifest index, if the selection changed.
    pub fn ui(&mut self, ui: &mut egui::Ui, ws: &Workspace) -> Option<usize> {
        ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("search paths (space-separated terms)").desired_width(f32::INFINITY));
        if self.last_query.as_deref() != Some(self.query.as_str()) {
            let terms: Vec<String> = self.query.to_lowercase().split_whitespace().map(str::to_string).collect();
            self.filtered = ws
                .present
                .iter()
                .copied()
                .filter(|&i| {
                    let p = ws.file(i).path.to_lowercase();
                    terms.iter().all(|t| p.contains(t.as_str()))
                })
                .collect();
            self.last_query = Some(self.query.clone());
        }
        ui.label(format!("{} resources", self.filtered.len()));
        ui.separator();
        let mut changed = None;
        let row_h = ui.text_style_height(&egui::TextStyle::Body);
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, self.filtered.len(), |ui, range| {
            for &i in &self.filtered[range] {
                let path = &ws.file(i).path;
                let selected = self.selected == Some(i);
                if ui.selectable_label(selected, path).clicked() && !selected {
                    self.selected = Some(i);
                    changed = Some(i);
                }
            }
        });
        changed
    }
}
