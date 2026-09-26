/// Round-trip with the full context (resource names + taxonomy/tag/merit names).
fn check_all<T: scribble_core::Format>(pred: impl Fn(&scribble_core::ResPath) -> bool) -> usize {
    scribble_formats::install_test_context();
    scribble_core::testing::check_all::<T>(pred)
}

#[test]
fn all_so_files() {
    check_all::<fmt_object::ScribbleObject>(|p| p.ext == "so");
}

#[test]
fn all_sao_files() {
    check_all::<fmt_object::SimpleObject>(|p| p.ext == "sao");
}

#[test]
fn all_sa_files() {
    check_all::<fmt_object::Adjective>(|p| p.ext == "sa");
}

#[test]
fn scribbleobject_odt() {
    check_all::<fmt_object::ObjectDetailsTable>(|p| p.ext == "odt" && p.file == "scribbleobject.odt");
}

/// Taxonomy ids print as names and parse back.
#[test]
fn taxonomy_names() {
    scribble_formats::install_test_context();
    let ctx = scribble_core::testing::context();
    let Some(root) = scribble_core::testing::extracted_root() else { return };
    let data = std::fs::read(root.join("data/_game/scribbleobjects/mammal_large_hooved_cow.so")).unwrap();
    let cow = <fmt_object::ScribbleObject as scribble_core::Format>::decode(&data, &ctx).unwrap();
    assert_eq!(cow.id.0.as_str(), "mammal/large/hooved/cow");
    assert_eq!(cow.id.ids(&ctx).unwrap(), [Some(16), Some(1562), Some(1588), Some(1598)]);
    let any = fmt_object::refs::ObjectPath(scribble_core::NamedPath("food/nutsgrains/*/*".into()));
    assert_eq!(any.ids(&ctx).unwrap(), [Some(11), Some(943), None, None]);
}
