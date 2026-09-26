//! Every object whose art is only the gift placeholder gets a resolution (see
//! docs/evidence/placeholders.md), and resolving never fails.

use scribble_core::Format;
use scribble_studio::object_resolve::{self, Body, WordIndex};
use scribble_studio::workspace::Workspace;
use std::path::PathBuf;

#[test]
fn every_gift_placeholder_is_explained() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    if !root.join("manifest.json").exists() {
        return;
    }
    let ws = Workspace::open(&root).unwrap();
    let dict = fmt_dictionary::Dictionary::load_dir(&ws.root, "english", &ws.ctx).unwrap();
    let words = WordIndex::from_dictionary(&dict);
    let (mut gift, mut avatars, mut hidden, mut ropes) = (Vec::new(), 0, 0, 0);
    for f in ws.manifest.files.iter().filter(|f| f.path.ends_with(".so") && f.pack.is_some()) {
        let data = ws.read_logical(&f.path).unwrap();
        let o = fmt_object::ScribbleObject::decode(&data, &ws.ctx).unwrap();
        let r = object_resolve::resolve(&f.path, &o, &ws, Some(&words));
        if object_resolve::is_gift_placeholder(&o.body.root, &ws) && !f.path.ends_with("misc_paper_thick_gift.so") {
            assert!(r.note.is_some(), "{}: gift placeholder without a note", f.path);
            gift.push(f.path.clone());
        }
        match r.body {
            Body::Object { .. } if f.path.contains("human_player_avatars__") => avatars += 1,
            Body::Nothing => hidden += 1,
            _ => {}
        }
        ropes += usize::from(!r.ropes.is_empty());
    }
    // 48 avatars, 5 rope pieces, 5 engine stand-ins (`_self_self_myobject` has no vector at all)
    // and 9 unreferenced leftovers.
    assert_eq!(gift.len(), 67, "{gift:#?}");
    assert_eq!(avatars, 48);
    assert!(hidden >= 6, "{hidden}");
    assert!(ropes >= 60, "{ropes}");
}
