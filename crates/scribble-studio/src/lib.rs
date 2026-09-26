//! Scribble Studio: browse and edit Scribblenauts Unlimited resources.
//!
//! Run `scribble-studio [extracted-dir]` on the directory produced by `scribble unpack`.

mod browser;
pub mod dictionary_editor;
pub mod object_resolve;
pub mod object_scene;
pub mod object_view;
mod value_editor;
mod resource_view;
mod texture;
mod vec_view;
pub mod workspace;

use eframe::egui;
use std::path::PathBuf;
use workspace::Workspace;

#[derive(PartialEq, Clone, Copy)]
pub enum Tab {
    Objects,
    Dictionary,
    Resources,
}

pub struct App {
    ws: Option<Workspace>,
    error: Option<String>,
    tab: Tab,
    browser: browser::Browser,
    view: Option<resource_view::ResourceView>,
    pub dictionary: dictionary_editor::DictionaryEditor,
    pub objects: object_view::ObjectViewer,
    status: String,
}

impl App {
    pub fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    pub fn workspace(&self) -> Option<&Workspace> {
        self.ws.as_ref()
    }

    /// Select a resource by logical path, as if clicked in the browser (used by tests).
    pub fn open_resource(&mut self, ctx: &egui::Context, logical: &str) {
        if let Some(ws) = &self.ws
            && let Some(i) = ws.find(logical)
        {
            self.tab = Tab::Resources;
            self.browser.selected = Some(i);
            self.view = Some(resource_view::ResourceView::open(ctx, ws, i));
        }
    }

    pub fn new(root: PathBuf) -> Self {
        let (ws, error) = match Workspace::open(&root) {
            Ok(ws) => (Some(ws), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        };
        App {
            ws,
            error,
            tab: Tab::Objects,
            browser: Default::default(),
            view: None,
            dictionary: Default::default(),
            objects: Default::default(),
            status: String::new(),
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Objects, "Objects");
            ui.selectable_value(&mut self.tab, Tab::Dictionary, "Dictionary");
            ui.selectable_value(&mut self.tab, Tab::Resources, "Resources");
            ui.separator();
            if ui.button("Open unpacked folder…").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                *self = App::new(dir);
            }
            if let Some(ws) = &self.ws
                && ui.button("Export game packs…").on_hover_text("Rebuild .p packs, index.bin, pmindex and 1s into a folder").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                self.status = match ws.export_game(&dir) {
                    Ok(()) => format!("exported to {}", dir.display()),
                    Err(e) => format!("export failed: {e:#}"),
                };
            }
            ui.label(&self.status);
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));
        let Some(ws) = &self.ws else {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.heading("No workspace");
                ui.label(self.error.as_deref().unwrap_or(""));
            });
            return;
        };
        match self.tab {
            Tab::Resources => {
                egui::Panel::left("browser").default_size(420.0).show(ui, |ui| {
                    if let Some(i) = self.browser.ui(ui, ws) {
                        self.view = Some(resource_view::ResourceView::open(&ctx, ws, i));
                    }
                });
                egui::CentralPanel::default().show(ui, |ui| match &mut self.view {
                    Some(v) => v.ui(ui, ws),
                    None => {
                        ui.label("Select a resource.");
                    }
                });
            }
            Tab::Dictionary => {
                let request = egui::CentralPanel::default().show(ui, |ui| self.dictionary.ui(ui, ws)).inner;
                if let Some(dictionary_editor::Request::OpenResource(path)) = request {
                    if path.ends_with(".so") || path.ends_with(".sao") {
                        self.objects.open(ws, &path);
                        self.tab = Tab::Objects;
                    } else {
                        self.open_resource(&ctx, &path);
                    }
                }
            }
            Tab::Objects => self.objects.ui(ui, ws),
        }
    }
}

/// Open the studio window on `root`.
pub fn run(root: PathBuf) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]).with_title("Scribble Studio"),
        ..Default::default()
    };
    eframe::run_native("Scribble Studio", options, Box::new(move |_cc| Ok(Box::new(App::new(root)))))
}
