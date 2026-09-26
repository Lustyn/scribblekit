//! Dictionary editor: the words players can type, what each one spawns, and their properties.
//!
//! Works on one language at a time through `fmt_dictionary::Dictionary`, which regenerates every
//! derived table (jump tables, word-id tables, single-word index, details, `.dtm`) on save.

use crate::workspace::Workspace;
use eframe::egui;
use fmt_dictionary::{Dictionary, Entry, Gender, Sense, Target, WordKind, ENGINE_LANGUAGES, LANGUAGES};
use scribble_core::ResRef;

pub struct DictionaryEditor {
    language: String,
    dict: Option<Dictionary>,
    kind: WordKind,
    query: String,
    /// Indices into the current kind's word list matching `query`.
    filtered: Vec<usize>,
    filter_stale: bool,
    selected: Option<String>,
    rename_to: String,
    new_word: String,
    target_query: String,
    dirty: bool,
    status: Option<(bool, String)>,
}

/// Edits requested while drawing, applied afterwards (the UI only borrows the dictionary).
enum Action {
    Select(String),
    Rename { from: String, to: String },
    Remove(String),
    SetLabel { word: String, meaning: usize, label: String },
    RemoveMeaning { word: String, meaning: usize },
    AddMeaning { word: String, id: u16 },
    AddWord { word: String, id: u16 },
    EditTarget { id: u16, target: Target },
}

/// A request for the rest of the app.
pub enum Request {
    /// Show this object/adjective resource in the object viewer.
    OpenResource(String),
}

impl Default for DictionaryEditor {
    fn default() -> Self {
        DictionaryEditor {
            language: "english".into(),
            dict: None,
            kind: WordKind::Object,
            query: String::new(),
            filtered: Vec::new(),
            filter_stale: true,
            selected: None,
            rename_to: String::new(),
            new_word: String::new(),
            target_query: String::new(),
            dirty: false,
            status: None,
        }
    }
}

fn kind_label(k: WordKind) -> &'static str {
    match k {
        WordKind::Object => "objects",
        WordKind::Adjective => "adjectives",
        WordKind::Tag => "tags",
        WordKind::Other(_) => "other",
    }
}

fn words_of(d: &Dictionary, kind: WordKind) -> &[Entry] {
    match kind {
        WordKind::Tag => d.tags.as_ref().map_or(&[][..], |t| &t.words[..]),
        _ => &d.words,
    }
}

fn resource_path(t: &Target) -> String {
    match &t.properties.resource {
        Some(ResRef::Path(p)) => p.clone(),
        Some(ResRef::Index(i)) => format!("#{i}"),
        None => String::new(),
    }
}

fn short(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

impl DictionaryEditor {
    pub fn load(&mut self, ws: &Workspace) {
        self.status = None;
        self.selected = None;
        self.dirty = false;
        self.filter_stale = true;
        match Dictionary::load_dir(&ws.root, &self.language, &ws.ctx) {
            Ok(d) => self.dict = Some(d),
            Err(e) => {
                self.dict = None;
                self.status = Some((false, format!("could not load {}: {e:#}", self.language)));
            }
        }
    }

    /// Select a word programmatically (used by tests and cross-tool links).
    pub fn select(&mut self, ws: &Workspace, language: &str, kind: WordKind, word: &str) {
        if self.dict.is_none() || self.language != language {
            self.language = language.to_string();
            self.load(ws);
        }
        self.kind = kind;
        self.query = word.to_string();
        self.filter_stale = true;
        self.selected = Some(word.to_string());
        self.rename_to = word.to_string();
    }

    /// Fill the "new word" form (for scripted use and UI tests).
    #[doc(hidden)]
    pub fn set_new_word_form(&mut self, word: &str, target_query: &str) {
        self.new_word = word.to_string();
        self.target_query = target_query.to_string();
    }

    fn save(&mut self, ws: &Workspace) {
        let Some(d) = &self.dict else { return };
        self.status = Some(match d.to_files(&ws.ctx).and_then(|files| {
            fmt_dictionary::write_files(&ws.root, &files)?;
            Ok(files.len())
        }) {
            Ok(n) => {
                self.dirty = false;
                (true, format!("saved {} ({n} files regenerated)", self.language))
            }
            Err(e) => (false, format!("save failed: {e:#}")),
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ws: &Workspace) -> Option<Request> {
        if self.dict.is_none() && self.status.is_none() {
            self.load(ws);
        }
        let mut request = None;
        self.toolbar(ui, ws);
        // Take the dictionary out while drawing so the UI helpers can borrow `self` mutably.
        let owned = self.dict.take()?;
        let dict = &owned;
        if self.filter_stale {
            let terms = fmt_dictionary::normalize_word(&self.query);
            self.filtered = words_of(dict, self.kind)
                .iter()
                .enumerate()
                .filter(|(_, e)| e.kind == self.kind && e.word.contains(terms.as_str()))
                .map(|(i, _)| i)
                .collect();
            self.filter_stale = false;
        }

        let mut actions = Vec::new();
        egui::Panel::left("dict_words").default_size(320.0).show(ui, |ui| {
            self.word_list(ui, dict, &mut actions);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                if let Some(word) = self.selected.clone() {
                    match dict.find(&word, self.kind) {
                        Some(entry) => self.word_details(ui, dict, entry, &mut actions, &mut request),
                        None => {
                            ui.label(format!("{word:?} is not a {} word.", kind_label(self.kind)));
                        }
                    }
                    ui.separator();
                }
                self.add_word_panel(ui, dict, &mut actions);
            });
        });
        self.dict = Some(owned);
        for a in actions {
            self.apply(a);
        }
        request
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, ws: &Workspace) {
        ui.horizontal(|ui| {
            let before = self.language.clone();
            egui::ComboBox::from_label("language").selected_text(&self.language).show_ui(ui, |ui| {
                for l in LANGUAGES {
                    let playable = ENGINE_LANGUAGES.contains(&l);
                    let text = if playable { l.to_string() } else { format!("{l} (unused by game)") };
                    ui.selectable_value(&mut self.language, l.to_string(), text);
                }
            });
            if self.language != before {
                if self.dirty {
                    self.status = Some((false, format!("discarded unsaved changes to {before}")));
                }
                self.load(ws);
            }
            for k in [WordKind::Object, WordKind::Adjective, WordKind::Tag] {
                if ui.selectable_label(self.kind == k, kind_label(k)).clicked() {
                    self.kind = k;
                    self.filter_stale = true;
                    self.selected = None;
                }
            }
            ui.separator();
            if ui.add_enabled(self.dirty, egui::Button::new("Save dictionary")).clicked() {
                self.save(ws);
            }
            if ui.add_enabled(self.dirty, egui::Button::new("Discard changes")).clicked() {
                self.load(ws);
            }
            if let Some((ok, msg)) = &self.status {
                ui.colored_label(if *ok { egui::Color32::from_rgb(80, 170, 80) } else { egui::Color32::from_rgb(220, 80, 80) }, msg);
            } else if self.dirty {
                ui.label("unsaved changes");
            }
        });
        ui.separator();
    }

    fn word_list(&mut self, ui: &mut egui::Ui, dict: &Dictionary, actions: &mut Vec<Action>) {
        if ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("search words").desired_width(f32::INFINITY)).changed() {
            self.filter_stale = true;
        }
        ui.label(format!("{} {}", self.filtered.len(), kind_label(self.kind)));
        let words = words_of(dict, self.kind);
        let row_h = ui.text_style_height(&egui::TextStyle::Body);
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, self.filtered.len(), |ui, range| {
            for &i in &self.filtered[range] {
                let Some(e) = words.get(i) else { continue };
                let selected = self.selected.as_deref() == Some(e.word.as_str());
                let suffix = if e.meanings.len() > 1 { format!("  ({} meanings)", e.meanings.len()) } else { String::new() };
                if ui.selectable_label(selected, format!("{}{suffix}", e.word)).clicked() {
                    actions.push(Action::Select(e.word.clone()));
                }
            }
        });
    }

    fn word_details(&mut self, ui: &mut egui::Ui, dict: &Dictionary, entry: &Entry, actions: &mut Vec<Action>, request: &mut Option<Request>) {
        ui.heading(&entry.word);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.rename_to).desired_width(260.0));
            if ui.button("Rename").clicked() {
                actions.push(Action::Rename { from: entry.word.clone(), to: self.rename_to.clone() });
            }
            if ui.button("Delete word").clicked() {
                actions.push(Action::Remove(entry.word.clone()));
            }
        });
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Meanings").strong());
        ui.label("Properties belong to the id and are shared by every word naming it; changes here apply to this language's files.");
        let targets = dict.targets(self.kind);
        for (mi, sense) in entry.meanings.iter().enumerate() {
            let Some(target) = targets.and_then(|t| t.get(&sense.id)) else {
                ui.colored_label(egui::Color32::YELLOW, format!("id {} has no target", sense.id));
                continue;
            };
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("id {}", sense.id)).monospace());
                    let path = resource_path(target);
                    if !path.is_empty() {
                        if ui.link(short(&path)).on_hover_text(&path).clicked() {
                            *request = Some(Request::OpenResource(path.clone()));
                        }
                    }
                    if let Some(n) = &target.name {
                        ui.label(format!("displayed as {n}"));
                    }
                    if entry.meanings.len() > 1 && ui.small_button("remove meaning").clicked() {
                        actions.push(Action::RemoveMeaning { word: entry.word.clone(), meaning: mi });
                    }
                });
                if entry.meanings.len() > 1 {
                    let mut label = sense.label.clone().unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label("choice label");
                        if ui.text_edit_singleline(&mut label).changed() {
                            actions.push(Action::SetLabel { word: entry.word.clone(), meaning: mi, label: label.clone() });
                        }
                    });
                }
                let mut t = target.clone();
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("cost");
                    changed |= ui.add(egui::DragValue::new(&mut t.properties.cost)).changed();
                    ui.label("adjective cost ×");
                    changed |= ui.add(egui::DragValue::new(&mut t.properties.cost_multiplier)).changed();
                    if self.kind == WordKind::Object {
                        egui::ComboBox::from_id_salt(("gender", sense.id)).selected_text(format!("{:?}", t.properties.gender).to_lowercase()).show_ui(
                            ui,
                            |ui| {
                                for g in [Gender::None, Gender::Male, Gender::Female, Gender::Either] {
                                    changed |= ui.selectable_value(&mut t.properties.gender, g, format!("{g:?}").to_lowercase()).changed();
                                }
                            },
                        );
                        changed |= ui.checkbox(&mut t.properties.random_gender, "random gender").changed();
                    }
                });
                if changed {
                    actions.push(Action::EditTarget { id: sense.id, target: t });
                }
            });
        }
        ui.add_space(6.0);
        ui.collapsing("Add a meaning", |ui| {
            if let Some(id) = self.target_picker(ui, dict, "add") {
                actions.push(Action::AddMeaning { word: entry.word.clone(), id });
            }
        });
    }

    fn add_word_panel(&mut self, ui: &mut egui::Ui, dict: &Dictionary, actions: &mut Vec<Action>) {
        ui.label(egui::RichText::new(format!("New {} word", kind_label(self.kind).trim_end_matches('s'))).strong());
        ui.horizontal(|ui| {
            ui.label("text");
            ui.add(egui::TextEdit::singleline(&mut self.new_word).hint_text("e.g. MOOCOW").desired_width(260.0));
        });
        ui.label("Pick what it means (search by existing word or resource path):");
        if let Some(id) = self.target_picker(ui, dict, "create word")
            && !self.new_word.trim().is_empty()
        {
            actions.push(Action::AddWord { word: self.new_word.clone(), id });
        }
    }

    /// Search the ids of the current kind; returns the id whose button was clicked.
    fn target_picker(&mut self, ui: &mut egui::Ui, dict: &Dictionary, button: &str) -> Option<u16> {
        ui.add(egui::TextEdit::singleline(&mut self.target_query).hint_text("search: COW, mammal, .so path…").desired_width(360.0));
        let q = self.target_query.to_lowercase();
        if q.len() < 2 {
            return None;
        }
        let mut picked = None;
        let targets = dict.targets(self.kind)?;
        for (&id, t) in targets.iter().filter(|(_, t)| t.name.as_deref().unwrap_or("").to_lowercase().contains(&q) || resource_path(t).to_lowercase().contains(&q)).take(25) {
            ui.horizontal(|ui| {
                if ui.small_button(button).clicked() {
                    picked = Some(id);
                }
                ui.label(format!("id {id} · {} · {}", t.name.as_deref().unwrap_or("(unnamed)"), short(&resource_path(t))));
            });
        }
        picked
    }

    fn apply(&mut self, action: Action) {
        let Some(d) = &mut self.dict else { return };
        let kind = self.kind;
        let result: anyhow::Result<()> = (|| {
            match action {
                Action::Select(w) => {
                    self.rename_to = w.clone();
                    self.selected = Some(w);
                    return Ok(());
                }
                Action::Rename { from, to } => {
                    let new = d.rename_word(&from, kind, &to)?;
                    self.selected = Some(new.clone());
                    self.rename_to = new;
                }
                Action::Remove(w) => {
                    d.remove_word(&w, kind)?;
                    self.selected = None;
                }
                Action::SetLabel { word, meaning, label } => {
                    let mut m = d.find(&word, kind).map(|e| e.meanings.clone()).unwrap_or_default();
                    if let Some(s) = m.get_mut(meaning) {
                        s.label = Some(label);
                    }
                    d.set_meanings(&word, kind, m)?;
                }
                Action::RemoveMeaning { word, meaning } => {
                    let mut m = d.find(&word, kind).map(|e| e.meanings.clone()).unwrap_or_default();
                    m.remove(meaning);
                    d.set_meanings(&word, kind, m)?;
                }
                Action::AddMeaning { word, id } => {
                    let mut m = d.find(&word, kind).map(|e| e.meanings.clone()).unwrap_or_default();
                    anyhow::ensure!(!m.iter().any(|s| s.id == id), "already means id {id}");
                    m.push(Sense::new(id));
                    d.set_meanings(&word, kind, m)?;
                }
                Action::AddWord { word, id } => {
                    let new = d.add_word(&word, kind, vec![Sense::new(id)])?;
                    self.new_word.clear();
                    self.query = new.clone();
                    self.rename_to = new.clone();
                    self.selected = Some(new);
                }
                Action::EditTarget { id, target } => {
                    if let Some(t) = d.targets_mut(kind).and_then(|m| m.get_mut(&id)) {
                        *t = target;
                    }
                }
            }
            self.dirty = true;
            self.filter_stale = true;
            Ok(())
        })();
        if let Err(e) = result {
            self.status = Some((false, format!("{e:#}")));
        } else if self.dirty {
            self.status = None;
        }
    }
}
