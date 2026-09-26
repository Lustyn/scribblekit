//! A real pointer click (not an accessibility action) on the object editor's Revert button.

use egui_kittest::{kittest::Queryable, Harness};
use scribble_studio::{workspace::Workspace, App, Tab};
use std::path::PathBuf;

#[test]
fn pointer_click_reverts_edit() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    if !root.join("manifest.json").exists() {
        return;
    }
    let r = root.clone();
    let mut h = Harness::builder().with_size(eframe::egui::vec2(1400.0, 900.0)).build_eframe(|_cc| App::new(r));
    {
        let app = h.state_mut();
        app.set_tab(Tab::Objects);
        let ws = Workspace::open(&root).unwrap();
        app.objects.open(&ws, r"data\_game\scribbleobjects\mammal_large_hooved_cow.so");
        app.objects.set_pose(None, 0.0, false, false);
        app.objects.edit_value(|v| v["body"]["physical"]["weight"] = 99.into());
    }
    h.run_steps(3);
    assert!(h.state().objects.is_dirty());
    h.run_steps(10);
    h.get_by_label("Revert").click();
    h.run_steps(3);
    eprintln!("dirty after pointer click on Revert: {}", h.state().objects.is_dirty());
    assert!(!h.state().objects.is_dirty());
}
