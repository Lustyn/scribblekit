/// Round-trip with the full context (resource names + taxonomy/tag/merit names).
fn check_all<T: scribble_core::Format>(pred: impl Fn(&scribble_core::ResPath) -> bool) -> usize {
    scribble_formats::install_test_context();
    scribble_core::testing::check_all::<T>(pred)
}
use scribble_core::ResPath;

fn text_dir(p: &ResPath) -> bool {
    p.ext.is_empty() && fmt_ui::is_text_dir(&p.dir)
}

fn data_of(p: &ResPath) -> Vec<u8> {
    let root = scribble_core::testing::extracted_root().unwrap();
    std::fs::read(root.join(p.full.replace('\\', "/"))).unwrap()
}

#[test]
fn text_tables() {
    check_all::<fmt_ui::TextTable>(|p| text_dir(p) && fmt_ui::TextTable::detect(&data_of(p)));
}

#[test]
fn event_scripts() {
    check_all::<fmt_ui::EventScripts>(|p| text_dir(p) && !fmt_ui::TextTable::detect(&data_of(p)));
}

#[test]
fn uib_files() {
    check_all::<fmt_ui::UiLayout>(|p| p.ext == "uib");
}

#[test]
fn sfb_files() {
    check_all::<fmt_ui::SpriteFrames>(|p| p.ext == "sfb");
}

#[test]
fn uit_files() {
    check_all::<fmt_ui::LayoutSource>(|p| p.ext == "uit");
}

#[test]
fn swc_files() {
    check_all::<fmt_ui::Palette>(|p| p.ext == "swc");
}

#[test]
fn stl_files() {
    check_all::<fmt_ui::TagList>(|p| p.ext == "stl");
}

#[test]
fn fasttravel_dat() {
    check_all::<fmt_ui::FastTravelMap>(|p| p.file == "fasttravel.dat");
}

#[test]
fn credits_txt() {
    check_all::<fmt_ui::CreditsText>(|p| p.ext == "txt" && p.dir == "data\\creditsdata");
}

/// Every extensionless file in the text directories is claimed by one of the two codecs, and
/// every event script decodes (no raw fallback).
#[test]
fn text_dir_coverage() {
    scribble_formats::install_test_context();
    let ctx = scribble_core::testing::context();
    let mut raw = 0;
    for (logical, path) in scribble_core::testing::resources(text_dir) {
        let data = std::fs::read(&path).unwrap();
        let p = ResPath::new(&logical);
        assert!(fmt_ui::handler(&p, &data).is_some(), "{logical} not handled");
        if !fmt_ui::TextTable::detect(&data) {
            let s = <fmt_ui::EventScripts as scribble_core::Format>::decode(&data, &ctx).unwrap();
            let langs = fmt_ui::text::language_count(ctx.platform());
            for l in &s.scripts {
                for slot in l.each(langs).unwrap() {
                    if let fmt_ui::event::Slot::Raw { raw: r } = slot {
                        eprintln!("{logical}: undecoded script ({} bytes)", r.0.len());
                        raw += 1;
                    }
                }
            }
        }
    }
    assert_eq!(raw, 0, "{raw} scripts kept raw");
}
