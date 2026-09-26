//! Drive the dictionary editor through its UI: add a word, save, and read it back from disk.

use egui_kittest::{kittest::Queryable, Harness};
use fmt_dictionary::{Dictionary, WordKind};
use scribble_studio::{workspace::Workspace, App, Tab};
use std::path::{Path, PathBuf};

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

#[test]
fn add_word_and_save() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    if !src.join("manifest.json").exists() {
        return;
    }
    // A minimal workspace: the manifest plus the dictionary directory.
    let tmp = std::env::temp_dir().join(format!("scribble-dict-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::copy(src.join("manifest.json"), tmp.join("manifest.json")).unwrap();
    let d = Path::new("data/_game/scribbleobjects/d");
    copy_dir(&src.join(d), &tmp.join(d));

    let root = tmp.clone();
    let mut h = Harness::builder().with_size(eframe::egui::vec2(1400.0, 900.0)).build_eframe(|_cc| App::new(root));
    {
        let app = h.state_mut();
        app.set_tab(Tab::Dictionary);
        let ws = Workspace::open(&tmp).unwrap();
        app.dictionary.select(&ws, "english", WordKind::Object, "COW");
        app.dictionary.set_new_word_form("moocow", "hooved_cow");
    }
    h.run_steps(3);
    h.get_by_label("create word").click();
    h.run_steps(3);
    h.get_by_label("Save dictionary").click();
    h.run_steps(3);

    let ws = Workspace::open(&tmp).unwrap();
    let dict = Dictionary::load_dir(&tmp, "english", &ws.ctx).unwrap();
    let moocow = dict.find("MOOCOW", WordKind::Object).expect("MOOCOW saved");
    let cow = dict.find("COW", WordKind::Object).unwrap();
    assert_eq!(moocow.meanings[0].id, cow.meanings[0].id);
    std::fs::remove_dir_all(&tmp).unwrap();
}
