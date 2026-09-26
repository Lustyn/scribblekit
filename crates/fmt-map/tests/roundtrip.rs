/// Round-trip with the full context (resource names + taxonomy/tag/merit names).
fn check_all<T: scribble_core::Format>(pred: impl Fn(&scribble_core::ResPath) -> bool) -> usize {
    scribble_formats::install_test_context();
    scribble_core::testing::check_all::<T>(pred)
}

#[test]
fn all_lvls_files() {
    check_all::<fmt_map::LevelTable>(|p| p.ext == "lvls");
}

#[test]
fn all_tle_files() {
    check_all::<fmt_map::TileMap>(|p| p.ext == "tle");
}

#[test]
fn all_stp_files() {
    check_all::<fmt_map::LevelSetup>(|p| p.ext == "stp");
}

#[test]
fn all_mdb_files() {
    check_all::<fmt_map::MeritDatabase>(|p| p.ext == "mdb");
}

#[test]
fn all_plf_files() {
    check_all::<fmt_map::ParallaxLayers>(|p| p.ext == "plf");
}

#[test]
fn all_nbtc_files() {
    check_all::<fmt_map::CollisionTileset>(|p| p.ext == "nbtc");
}

#[test]
fn all_dpd_files() {
    check_all::<fmt_map::PreloadList>(|p| p.ext == "dpd");
}

#[test]
fn all_sod_files() {
    check_all::<fmt_map::Scene>(|p| p.ext == "sod");
}
