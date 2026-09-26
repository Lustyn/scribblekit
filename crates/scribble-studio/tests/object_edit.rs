//! Edit an object's property through the object editor and save it.

use egui_kittest::{kittest::Queryable, Harness};
use scribble_core::Format;
use scribble_studio::{workspace::Workspace, App, Tab};
use std::path::{Path, PathBuf};

const COW: &str = r"data\_game\scribbleobjects\mammal_large_hooved_cow.so";

#[test]
fn change_weight_and_save() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    if !src.join("manifest.json").exists() {
        return;
    }
    let tmp = std::env::temp_dir().join(format!("scribble-obj-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let rel = Path::new("data/_game/scribbleobjects/mammal_large_hooved_cow.so");
    std::fs::create_dir_all(tmp.join(rel.parent().unwrap())).unwrap();
    std::fs::copy(src.join("manifest.json"), tmp.join("manifest.json")).unwrap();
    std::fs::copy(src.join(rel), tmp.join(rel)).unwrap();

    let root = tmp.clone();
    let mut h = Harness::builder().with_size(eframe::egui::vec2(1400.0, 900.0)).build_eframe(|_cc| App::new(root));
    {
        let app = h.state_mut();
        app.set_tab(Tab::Objects);
        let ws = Workspace::open(&tmp).unwrap();
        app.objects.open(&ws, COW);
        app.objects.edit_value(|v| v["body"]["physical"]["weight"] = 99.into());
    }
    h.run_steps(10);
    h.get_by_label("Save object").click();
    h.run_steps(3);
    assert!(h.state().objects.messages().0.is_some_and(|m| m.starts_with("saved")), "{:?}", h.state().objects.messages());

    let ws = Workspace::open(&tmp).unwrap();
    let cow = fmt_object::ScribbleObject::decode(&ws.read_logical(COW).unwrap(), &ws.ctx).unwrap();
    assert_eq!(cow.body.physical.weight, 99);
    std::fs::remove_dir_all(&tmp).unwrap();
}
