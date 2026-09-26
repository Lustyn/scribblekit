use fmt_anim::{Angle, Animation, Channel, EventKind, Fx12, PartTransform};
use scribble_core::{Context, Format};

#[test]
fn all_anim_files() {
    let n = scribble_core::testing::check_all::<Animation>(|p| p.ext == "anim");
    if n > 0 {
        assert_eq!(n, 5688);
    }
}

fn load(rel: &str) -> Option<Animation> {
    let root = scribble_core::testing::extracted_root()?;
    let data = std::fs::read(root.join(rel)).ok()?;
    Some(Animation::decode(&data, &Context::empty()).unwrap())
}

#[test]
fn walk_cycle_decodes_and_samples() {
    let Some(a) = load("data/meshanim/plants/maneatingplant/maneatingplant_walk.anim") else { return };
    assert!(a.looping);
    assert_eq!(a.part_count, 4);
    assert_eq!(a.tracks.len(), 5);
    assert_eq!(a.tracks[0].part, 0);
    let Channel::Rotation(keys) = &a.tracks[0].channel else { panic!("expected rotation") };
    assert_eq!(keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [0, 15, 40, 50, 60]);
    assert_eq!(keys[0].degrees, Angle(-6493));
    assert!(matches!(a.tracks[1].channel, Channel::Translation(_)));
    assert_eq!(a.duration_frames(), 60.0);

    // Exact keys and engine-exact interpolation (k = round(4096/15) = 273, slope = 415/frame).
    assert_eq!(a.sample(0.0).part(0).rotation, Some(Angle(-6493)));
    assert_eq!(a.sample(15.0).part(0).rotation, Some(Angle(-263)));
    assert_eq!(a.sample(7.5).part(0).rotation, Some(Angle(-3381)));
    // Looping wraps: frame 75 == frame 15.
    assert_eq!(a.sample(75.0), a.sample(15.0));
    // Part 0 also has a translation track; parts 1..3 only rotate.
    assert_eq!(a.sample(0.0).part(0).translation, Some((Fx12(0), Fx12(-6519))));
    assert_eq!(a.sample(0.0).part(1).translation, None);

    let rest = vec![PartTransform { x: Fx12(100), y: Fx12(200), angle: Angle(10) }; 4];
    let posed = a.sample(0.0).apply(&rest);
    assert_eq!(posed[0].angle, Angle((10 - 6493) & 0xffff));
    assert_eq!(posed[0].y, Fx12(200 - 6519));
}

#[test]
fn one_shot_clamps_and_has_events() {
    let Some(a) = load("data/meshanim/plants/maneatingplant/maneatingplant_attack.anim") else { return };
    assert!(!a.looping);
    assert!(a.event_frame(EventKind::Action).is_some());
    let end = a.duration_frames();
    assert_eq!(a.sample(end + 100.0), a.sample(end));
}

#[test]
fn json_is_readable() {
    let Some(a) = load("data/meshanim/plants/maneatingplant/maneatingplant_idle.anim") else { return };
    let text = scribble_core::json::to_string_of(&a).unwrap();
    assert!(text.contains("\"looping\": true"));
    assert!(text.contains("\"rotation\""));
    assert!(text.contains("\"degrees\""));
    let back: Animation = serde_json::from_str(&text).unwrap();
    assert_eq!(back, a);
}

#[test]
fn slot_names() {
    assert_eq!(fmt_anim::slot_name(14), Some("idle"));
    assert_eq!(fmt_anim::slot_name(33), Some("walk"));
    assert_eq!(fmt_anim::slot_name(4), Some("climb_ladder"));
    assert_eq!(fmt_anim::slot_for_file("cow_climbladder.anim"), Some(4));
    assert_eq!(fmt_anim::slot_for_file("man_useobjectthrow.anim"), Some(12));
    assert_eq!(fmt_anim::slot_for_file("data\\meshanim\\x\\cow\\cow_walk.anim"), Some(33));
    assert_eq!(fmt_anim::slot_for_file("maneatingplant_swi.anim"), Some(30));
}

#[test]
fn background_spin_rule() {
    let Some(a) = load("data/meshanim/_mapanim/ferriswheeldowntown/ferriswheeldowntown_spin.anim") else { return };
    assert_eq!(a.duration_frames(), 600.0);
    // FUN_006ecdd0: +-(i16)trunc(t / 2457600 * 65535), absolute, sign of the interpolated offset.
    let p = a.sample_background(150.0);
    assert_eq!(p.part(1).rotation, Some(Angle(16383)));
    assert!(p.part(1).rotation_is_absolute);
    assert_eq!(p.part(2).rotation, Some(Angle(-16383)));
    // Past half a turn the value wraps through the i16 cast.
    assert_eq!(a.sample_background(450.0).part(1).rotation, Some(Angle(49151 - 65536)));
    // The blending evaluator used for .so objects has no such rule.
    assert!(!a.sample(150.0).part(1).rotation_is_absolute);
}

#[test]
fn aim_event_kinds() {
    // bipedanimation_shoot: arm hanging down at frame 0 (kind 1), level at 10, raised at 20.
    let Some(a) = load("data/meshanim/human/bipedanimation/bipedanimation_shoot.anim") else { return };
    assert_eq!(a.event_frame(EventKind::AimDown), Some(0));
    assert_eq!(a.event_frame(EventKind::Action), Some(10));
    assert_eq!(a.event_frame(EventKind::AimUp), Some(20));
}
