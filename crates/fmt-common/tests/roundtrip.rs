#[test]
fn all_dps_files() {
    scribble_core::testing::check_all::<fmt_common::Dependencies>(|p| p.ext == "dps");
}
