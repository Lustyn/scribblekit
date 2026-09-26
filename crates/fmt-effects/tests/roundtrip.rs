use scribble_core::testing::check_all;

#[test]
fn all_gps_files() {
    check_all::<fmt_effects::ParticleSystem>(|p| p.ext == "gps");
}

#[test]
fn all_gec_files() {
    check_all::<fmt_effects::EmitterCollection>(|p| p.ext == "gec");
}

#[test]
fn all_trns_files() {
    check_all::<fmt_effects::Transition>(|p| p.ext == "trns");
}

#[test]
fn all_exf_files() {
    check_all::<fmt_effects::CustomFilter>(|p| p.ext == "exf");
}

#[test]
fn all_aaf_files() {
    check_all::<fmt_effects::AudioMetadata>(|p| p.ext == "aaf");
}
