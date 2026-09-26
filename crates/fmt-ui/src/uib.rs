//! `.uib`: a menu/HUD layout — a tree of UI elements, reusable element templates, keyframe
//! animations and named integer constants. Loaded by `FUN_005222c0`; elements are read by
//! `FUN_005212b0` (type-specific data by vtable slot 0x50 of each element class), animations
//! are played from the stored bytes by `FUN_005325e0` / `FUN_00533340`.
//!
//! ```text
//! u32 text                     // resource index of the `_scripts\[region]\…` text table (-1 none)
//! u16 width, u16 height        // design resolution (1920x1080 or 854x480)
//! u8  keyboard_navigation
//! u8  root_count, Element[root_count]
//! u8  template_count; u32 size; u8 blob[size]; template_count x { str8 name, u32 offset }
//! u8  animation_count; u32 size; u8 blob[size]; animation_count x { str8 name, u32 offset }
//! u8  constant_count; constant_count x { str8 name, u32 value }
//!
//! Element:
//!     u8 type; str8 name
//!     i32 x, y                 // 20.12 fixed, relative to the parent's centre
//!     i32 scale_x, scale_y     // 20.12 fixed
//!     i32 width, height
//!     i32 rotation             // degrees
//!     i32 base_width, base_height
//!     i32 pivot_x, pivot_y     // 20.12 fixed
//!     i32 base_rotation        // degrees
//!     u8 drawn, u8 enabled, u8 clip, u8 default_focus
//!     i32 help_text            // string index in the text table, -1 = none
//!     type-specific data (see ElementData)
//!     u8 child_count, Element[child_count]
//!
//! Template blob: the templates' element trees back to back (each offset = start of one).
//! Animation blob: back-to-back animations:
//!     u8 looping; u32 track_count
//!     track_count x { str8 element_path; u32 key_count; key_count x { u8 kind, i32 ticks, data } }
//! ```
//!
//! Seven unused layouts were saved by older versions of the menu tool (no per-element
//! `help_text`, shorter type data, or no text reference and an 8-value transform). The shipped
//! loader `FUN_005212b0` has no version switch — it always reads the current layout — so it
//! cannot read them, and none of their resource indices (0x6c99, 0x6c9b, 0x6c9d, 0x6c9e, 0x6cbb,
//! 0x6cbd, 0x6cef) appears in the code. They are recognised and kept via [`Version`] and
//! [`Element::legacy_data`].

use crate::named::{bit_flags, int_enum};
use scribble_core::{anyhow, bail, ensure, Context, Format, Hex, Reader, ResRef, Result, Writer};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------------------------
// 20.12 fixed point

pub use scribble_core::Fx12;

fn fx(r: &mut Reader) -> Result<Fx12> {
    Ok(Fx12(r.i32()?))
}

// ---------------------------------------------------------------------------------------------
// Types

/// Which variant of the layout format a file uses. Only `current` is readable by the engine
/// (`FUN_005212b0` reads every field unconditionally); the others are menu-tool leftovers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Version {
    /// The format the game reads.
    #[default]
    Current,
    /// No per-element `help_text`; type data as in `current` except buttons (2 bytes) and
    /// toggles (1 byte).
    NoHelpText,
    /// No per-element `help_text`; images without source rectangle; buttons/toggles 1 byte.
    Early,
    /// No text reference, 8-value transform, 3 flag bytes, no animations or constants.
    Earliest,
}

impl Version {
    fn has_help_text(self) -> bool {
        self == Version::Current
    }
    /// Size of the type-specific data kept as `legacy_data`, or `None` if it is decoded.
    fn legacy_size(self, kind: u8) -> Option<usize> {
        use Version::*;
        match (self, kind) {
            (Current, _) | (_, 1) => None,
            (NoHelpText, 2 | 6) => None,
            (NoHelpText, 4) => Some(2),
            (Early, 6) | (Early | Earliest, 2) => None,
            (Earliest, 6) => Some(32),
            (_, 4 | 5) => Some(1),
            _ => Some(usize::MAX),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiLayout {
    #[serde(default, skip_serializing_if = "is_current")]
    pub version: Version,
    /// Text table whose strings the text elements show and `help_text` indexes (layout +0x68,
    /// looked up by `FUN_005233f0` -> `FUN_0049a710`); `null` if none.
    pub text: Option<ResRef>,
    /// Design resolution (layout +0x44..+0x4a).
    pub width: u16,
    pub height: u16,
    /// Arrow keys / d-pad move the focus between elements (root +0xf4, read by `FUN_0052b6a0`
    /// and `FUN_0052b610`, which step the focus on inputs 0x23..0x26).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub keyboard_navigation: u8,
    pub elements: Vec<Element>,
    /// Element trees instantiated at run time (list rows, icons, …; read by `FUN_0052af80`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<Template>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<UiAnimation>,
    /// Named integers looked up by the menu code (`FUN_00521220(name)`, e.g.
    /// `"ExpandedButtonsPerPage"` in `FUN_00483b90`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constants: Vec<Constant>,
}

fn is_current(v: &Version) -> bool {
    *v == Version::Current
}
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}
fn is_neg1(v: &i32) -> bool {
    *v == -1
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn fx_pair_zero(v: &[Fx12; 2]) -> bool {
    v[0].is_zero() && v[1].is_zero()
}

/// A named integer (`FUN_005222c0` copies up to 32 name bytes + the u32 into a 0x24-byte entry;
/// `FUN_00521220` returns the value for a name).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constant {
    pub name: String,
    pub value: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub element: Element,
}

/// Element classes: the constructor chosen by the type byte in `FUN_005212b0`; each class's
/// vtable slot 0x40 returns the same number (e.g. `FUN_005255b0` returns 4, `FUN_0052bca0` 7).
/// The menu tool's XML (`fast_travel.uit`) calls types 1/2/4/6 `Layer`, `Image`, `Button`,
/// `TextBox`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    /// 1: container without own graphics (`<Layer>`; ctor `FUN_00526dd0`).
    Group,
    /// 2: textured quad (`<Image>`; data read by `FUN_00528560`).
    Image,
    /// 3: flipbook sprite (`.sfb` animation over a texture; `FUN_0052ba30`).
    Sprite,
    /// 4: button (`<Button>`; states normal/hover/pressed/disabled, `FUN_00526440`).
    Button,
    /// 5: two-state button; plays `toggle_normal`/`toggle_pressed`/… (`FUN_00531a80`).
    Toggle,
    /// 6: text label or input box (`<TextBox>`; `FUN_00530fb0`).
    Text,
    /// 7: slider whose child `Thumb` is dragged along its `track` (`FUN_0052bd50`).
    Slider,
    /// 8: nine-slice frame (`FUN_00523810` -> `FUN_0053a2d0`).
    Frame,
    #[serde(untagged)]
    Other(u8),
}

impl ElementKind {
    fn from_u8(v: u8) -> Self {
        use ElementKind::*;
        match v {
            1 => Group,
            2 => Image,
            3 => Sprite,
            4 => Button,
            5 => Toggle,
            6 => Text,
            7 => Slider,
            8 => Frame,
            v => Other(v),
        }
    }
    fn to_u8(self) -> u8 {
        use ElementKind::*;
        match self {
            Group => 1,
            Image => 2,
            Sprite => 3,
            Button => 4,
            Toggle => 5,
            Text => 6,
            Slider => 7,
            Frame => 8,
            Other(v) => v,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub kind: ElementKind,
    /// Name used in dotted paths (`FUN_00527d00` matches path segments against it).
    pub name: String,
    /// Position of the centre relative to the parent's centre.
    pub position: [Fx12; 2],
    #[serde(default = "unit_scale", skip_serializing_if = "is_unit_scale")]
    pub scale: [Fx12; 2],
    pub size: [i32; 2],
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub rotation: i32,
    /// Defaults to `size`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_size: Option<[i32; 2]>,
    #[serde(default, skip_serializing_if = "fx_pair_zero")]
    pub pivot: [Fx12; 2],
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub base_rotation: i32,
    /// Drawn (+0x88: skipped by drawing and hit testing `FUN_005278d0`; also the blink state,
    /// `FUN_00526c10`).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub drawn: bool,
    /// Enabled (`FUN_005212b0` passes the inverse to vtable slot 0x64, which stores it at +0x9d
    /// and propagates it to children; buttons then show state 3 "disabled", `FUN_00525e20`).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Clips children to the element's rectangle (+0xa8, `FUN_0052d390`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub clip: bool,
    /// Gets the focus first: `FUN_005212b0` stores the element at root +0xe8, which
    /// `FUN_0052b560` returns as the initial focus (if focusable) before searching the tree.
    #[serde(default, skip_serializing_if = "is_false")]
    pub default_focus: bool,
    /// String index (in the layout's `text` table) of the help line shown while the element is
    /// highlighted; -1 = none. Stored at +0xe0 and read only by `FUN_00528370`, which looks the
    /// string up with `FUN_005233f0`; menu code (e.g. `FUN_005729c0`) passes it to
    /// `FUN_00724a20`, which shows it in `mainmenu.InfoText`. In shipped layouts it indexes
    /// strings such as "SAVE OBJECT!" on the object editor's save button.
    #[serde(default = "neg1", skip_serializing_if = "is_neg1")]
    pub help_text: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite: Option<SpriteData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<ButtonData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle: Option<ToggleData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slider: Option<SliderData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<FrameData>,
    /// Type data of a legacy layout ([`Version`] other than `current`), kept as bytes: the
    /// engine has no reader for these layouts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_data: Option<Hex>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Element>,
}

fn unit_scale() -> [Fx12; 2] {
    [Fx12(4096); 2]
}
fn is_unit_scale(v: &[Fx12; 2]) -> bool {
    *v == unit_scale()
}
fn is_255(v: &u8) -> bool {
    *v == 255
}
fn is_one_u8(v: &u8) -> bool {
    *v == 1
}
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}
fn full_alpha() -> u8 {
    255
}
fn one_u8() -> u8 {
    1
}

fn yes() -> bool {
    true
}
fn neg1() -> i32 {
    -1
}

bit_flags! {
    /// Image texture setup. `FUN_00528560` passes bit 0 and bit 1 to vtable slot 0x94
    /// (`FUN_00528a90`) as the x/y tiling switches and stores bit 7 at +0xf4 (texture drawn with
    /// `srgbtexture.gp`). Bits 2-6 are not read.
    pub struct TextureFlags { 0 => "tile_x", 1 => "tile_y", 7 => "srgb" }
}

/// Type 2 (`FUN_00528560`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageData {
    /// Non-zero: drawn with the vector material (passed as a bool to slot 0x94;
    /// scribblematerialvector.gp, `FUN_005afa80`).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub vector: u8,
    pub texture: Option<ResRef>,
    /// Source rectangle in the texture: `[x, y, width, height]` (+0xec..+0xf2; absent in early
    /// layouts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<[u16; 4]>,
    pub texture_flags: TextureFlags,
}

/// Type 3 (`FUN_0052ba30`); unused by shipped layouts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpriteData {
    /// Texture strip: the first u32 is passed to the quad constructor `FUN_006ef1f0` ->
    /// `FUN_005af500` (the texture loader also used for images and frames).
    pub texture: Option<ResRef>,
    /// Flipbook (`.sfb`): the second u32 goes to `FUN_006efd80`, which loads it with
    /// `FUN_006ef060` (the `.sfb` reader).
    pub animation: Option<ResRef>,
    /// Skipped by the reader (`FUN_0052ba30` advances the cursor by 9 after reading 8 bytes).
    pub unused: u8,
}

/// Sounds of a button or toggle, played on state changes by vtable slot 0x94 (`FUN_005259e0`,
/// shared by both classes; states: 0 normal, 1 hover, 2 pressed, 3 disabled — the animation
/// names at `0x00895e74`). Played with `FUN_0051d120(sound, 0x10)`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct StateSounds {
    /// +0x128: played on entering hover; stopped (`FUN_0051cb60`) when the element leaves hover
    /// for normal/disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover_sound: Option<ResRef>,
    /// +0x12c: played on entering hover when `hover_sound` is not set (never stopped).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover_fallback_sound: Option<ResRef>,
    /// +0x130: played on entering the pressed state (click).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press_sound: Option<ResRef>,
}

/// Type 4 (`FUN_005255d0`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ButtonData {
    /// Repeats while held (+0xf4; `FUN_00526440`, `FUN_00525cd0`), after `repeat_delay` ticks
    /// every `repeat_interval` ticks (+0x11c, +0x120).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub auto_repeat: u8,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub repeat_delay: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub repeat_interval: u32,
    /// Pixel masks for hit testing (`FUN_00526030`); absent in every shipped layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_masks: Option<HitMasks>,
    #[serde(flatten)]
    pub sounds: StateSounds,
}

/// Hit-test masks of a button (`FUN_00526030`): 1-bit bitmaps, one bit per 4x4-pixel cell,
/// row-major with a row stride of `width / 4` bits, least significant bit first. A point inside
/// the `[width, height]` box (centred on the element) hits if its cell's bit is set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HitMasks {
    /// Box size (+0xfa, +0xfc) and bits (+0x104) used in the normal and disabled states.
    pub normal_size: [u16; 2],
    pub normal_mask: Hex,
    /// Box size (+0xfe, +0x100) and bits (+0x108) used in the hover and pressed states.
    pub active_size: [u16; 2],
    pub active_mask: Hex,
}

/// Type 5 (`FUN_00531a80`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToggleData {
    /// Starts checked (+0x134): `FUN_005319e0` then plays the `toggle_normal`/`toggle_hover`/…
    /// state animations instead of `normal`/`hover`/…; set/cleared by animation keys
    /// `check`/`uncheck`.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub checked: u8,
    #[serde(flatten)]
    pub sounds: StateSounds,
}

int_enum! {
    /// Horizontal text alignment (+0x128), applied per line by `FUN_005301a0`.
    pub enum HAlign: u8 {
        /// 0: line centred in the box.
        Center = 0,
        /// 1: line starts at `-box_width / 2`.
        Left = 1,
        /// 2: line ends at `box_width / 2`.
        Right = 2,
        /// 3: left-aligned, with the spare width spread over the word gaps.
        Justify = 3,
    }
}

int_enum! {
    /// Vertical text alignment (+0x12c), applied to the block of lines by `FUN_005301a0`.
    pub enum VAlign: u8 {
        /// 0: lines centred.
        Center = 0,
        /// 1: first line at the top of the box.
        Top = 1,
        /// 2: last line at the bottom of the box.
        Bottom = 2,
    }
}

fn is_center_h(v: &HAlign) -> bool {
    *v == HAlign::Center
}
fn is_center_v(v: &VAlign) -> bool {
    *v == VAlign::Center
}

/// Type 6 (`FUN_00530fb0`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextData {
    /// Index into the game's font table (+0x108 = `DAT_008a82fc + 8 + font * 0x18`).
    pub font: u8,
    #[serde(default = "center_h", skip_serializing_if = "is_center_h")]
    pub align: HAlign,
    #[serde(default = "center_v", skip_serializing_if = "is_center_v")]
    pub valign: VAlign,
    /// String index in the layout's text table (`FUN_005233f0`); -1 = set by code.
    pub string: i32,
    /// Text box size in pixels (+0x10c, +0x110), used for alignment and wrapping.
    pub box_size: [i32; 2],
    /// +0x114: newlines in the string become spaces instead of line breaks (`FUN_00530910`
    /// starts a new line on `\n` only when it is 0; `FUN_0052f710` turns `\n` into ' ').
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub newline_to_space: u8,
    /// RGB, reduced to 5 bits per channel (`FUN_005317e0`) and packed at +0x116.
    pub color: [u8; 3],
    /// Opacity 0..255, scaled to the engine's 1..31 (+0x11c).
    #[serde(default = "full_alpha", skip_serializing_if = "is_255")]
    pub alpha: u8,
    /// Non-zero: the box gets a text caret — `FUN_0052eef0` creates a child quad from the
    /// font's caret texture (font +4), keeps it at +0x14c and positions it at the cursor
    /// (`FUN_0052e700`). Set only on the word-entry boxes (`writeBox`) of the write-mode menus.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub caret: u8,
    /// Caret blink period in ticks (read only when `caret` is set; `FUN_00526c90` stores it at
    /// the caret's +0xf8, `FUN_00526c10` toggles its `drawn` flag every period).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub caret_blink: u32,
    /// +0x144: with a caret, the box takes the focus and hit tests against `box_size` (vtable
    /// slot 0x44 `FUN_0052f310` returns `+0x144 && caret`; slot 0x78 checks it before hit
    /// testing). 1 in every shipped layout.
    #[serde(default = "one_u8", skip_serializing_if = "is_one_u8")]
    pub focusable: u8,
    /// Scrolls long text automatically (+0x130, `FUN_005306b0`): pauses `scroll_pause` ticks
    /// (+0x134), moves `scroll_speed` px/s (+0x140 = speed * 4096 / 60).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub autoscroll: u8,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub scroll_pause: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub scroll_speed: u32,
}

fn center_h() -> HAlign {
    HAlign::Center
}
fn center_v() -> VAlign {
    VAlign::Center
}

/// Type 7 (`FUN_0052bcc0`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SliderData {
    /// 1 = the thumb moves along y, 0 = along x. Stored inverted at +0xe8, which
    /// `FUN_0052bd50`/`FUN_0052bed0` test to pick the x or y coordinate (the vertical
    /// `hueBar`/`scaleBar` of the object editor have 1).
    pub vertical: u8,
    /// The thumb travels ±`travel` pixels around its start position (+0xfc; `FUN_0052bd50`
    /// sets min = start - travel, max = start + travel).
    pub travel: i32,
}

/// Type 8 (`FUN_00523810`, geometry built by `FUN_0053a2d0`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrameData {
    pub texture: Option<ResRef>,
    /// Texture coordinates (fractions of the texture) of the inner grid lines:
    /// `[left, top, right, bottom]`. `FUN_0053a2d0` builds the nine-slice UVs from them and the
    /// right/bottom border ratios `(1 - right) / left`, `(1 - bottom) / top`.
    pub inner_uv: [Fx12; 4],
    /// Border width in pixels (`FUN_005397e0` offsets the inner grid lines by it).
    pub border: Fx12,
    /// Skipped by the reader (`FUN_00523810` reads 24 bytes and advances by 28).
    pub unused: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UiAnimation {
    pub name: String,
    /// Stored at player +0x2c by `FUN_005325e0`: the animation restarts when it ends.
    pub looping: u8,
    pub tracks: Vec<AnimTrack>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimTrack {
    /// Dotted element path, e.g. `menu_main.savingBar`.
    pub element: String,
    pub keys: Vec<Key>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    /// Ticks (60/s) from the previous key to this one; -1 = applied immediately when the
    /// animation starts (`FUN_00533340` applies and destroys such keys at once).
    pub ticks: i32,
    #[serde(flatten)]
    pub change: Change,
}

int_enum! {
    /// Interpolation curve of a tweened key (`FUN_00532ce0(easing, progress)`).
    pub enum Easing: u8 {
        /// 0: t.
        Linear = 0,
        /// 1: t³ (starts slow).
        EaseIn = 1,
        /// 2: 1 - (1 - t)³ (ends slow).
        EaseOut = 2,
    }
}

fn is_linear(v: &Easing) -> bool {
    *v == Easing::Linear
}
fn linear() -> Easing {
    Easing::Linear
}

/// Keyframe kinds: `FUN_00533340` switches on the kind byte to one class per kind (each reads
/// its data with vtable slot 4 and applies itself with slot 0x14).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// 0: only advances time (`FUN_005359f0`).
    Wait,
    /// 1: move to (x, y) (`FUN_00534820`; `FUN_005349f0` sets the position each tick).
    Move {
        #[serde(default = "linear", skip_serializing_if = "is_linear")]
        easing: Easing,
        x: Fx12,
        y: Fx12,
    },
    /// 2: opacity 0..31 (`FUN_00534430`).
    Alpha {
        #[serde(default = "linear", skip_serializing_if = "is_linear")]
        easing: Easing,
        alpha: u8,
    },
    /// 3: scale (`FUN_00535550`).
    Scale {
        #[serde(default = "linear", skip_serializing_if = "is_linear")]
        easing: Easing,
        x: Fx12,
        y: Fx12,
    },
    /// 4: rotate (`FUN_00535290`).
    Rotate {
        #[serde(default = "linear", skip_serializing_if = "is_linear")]
        easing: Easing,
        degrees: i32,
    },
    /// 5: enables the element (`FUN_005342c0`: vtable slot 0x64(0)).
    Enable,
    /// 6: disables the element (`FUN_00534150`: slot 0x64(1)).
    Disable,
    /// 7: checks a toggle (`FUN_00533a70`; apply `FUN_00533b60` sets +0x134 = 1 if the element
    /// is type 5).
    Check,
    /// 8: unchecks a toggle (`FUN_00535870`; `FUN_00535960` sets +0x134 = 0).
    Uncheck,
    /// 9: start another animation of this layout (`FUN_00534cf0`).
    Play { animation: String },
    /// 10: start an animation on another element (`FUN_00534fa0`).
    PlayOn { animation: String, element: String },
    /// 12: spawns a particle effect as child `name` (`FUN_00533c30`, `FUN_00533d80`).
    SpawnEffect { name: String, effect: Option<ResRef> },
    /// 13: removes child `name` (`FUN_00533f10`, `FUN_00533fd0`).
    RemoveChild { name: String },
    /// 14: plays a sound (`FUN_00534b40`; `FUN_00534be0` calls `FUN_0051d120(sound, 0x10010)`).
    Sound { sound: Option<ResRef> },
}

// ---------------------------------------------------------------------------------------------
// Reading

/// A section's byte blob and its `(name, offset)` index.
type NamedBlob = (Vec<u8>, Vec<(String, u32)>);

struct Rd<'a> {
    r: Reader<'a>,
    ctx: &'a Context,
    v: Version,
}

fn res(i: u32, ctx: &Context) -> Option<ResRef> {
    (i != u32::MAX).then(|| ResRef::from_index(i, ctx))
}

fn read_sounds(r: &mut Reader, ctx: &Context) -> Result<StateSounds> {
    Ok(StateSounds { hover_sound: res(r.u32()?, ctx), hover_fallback_sound: res(r.u32()?, ctx), press_sound: res(r.u32()?, ctx) })
}

impl Rd<'_> {
    fn element(&mut self) -> Result<Element> {
        let r = &mut self.r;
        let kind_byte = r.u8()?;
        let kind = ElementKind::from_u8(kind_byte);
        let name = r.str_u8()?;
        let (position, scale, size, rotation, base_size, pivot, base_rotation);
        if self.v == Version::Earliest {
            position = [fx(r)?, fx(r)?];
            scale = [fx(r)?, fx(r)?];
            rotation = r.i32()?;
            size = [r.i32()?, r.i32()?];
            base_rotation = r.i32()?;
            base_size = None;
            pivot = [Fx12(0); 2];
        } else {
            position = [fx(r)?, fx(r)?];
            scale = [fx(r)?, fx(r)?];
            size = [r.i32()?, r.i32()?];
            rotation = r.i32()?;
            let bs = [r.i32()?, r.i32()?];
            base_size = (bs != size).then_some(bs);
            pivot = [fx(r)?, fx(r)?];
            base_rotation = r.i32()?;
        }
        let mut flag = || -> Result<bool> {
            match r.u8()? {
                0 => Ok(false),
                1 => Ok(true),
                v => bail!("element flag byte {v}"),
            }
        };
        let drawn = flag()?;
        let enabled = flag()?;
        let clip = flag()?;
        let default_focus = if self.v == Version::Earliest { false } else { flag()? };
        let help_text = if self.v.has_help_text() { r.i32()? } else { -1 };
        let mut e = Element {
            kind,
            name,
            position,
            scale,
            size,
            rotation,
            base_size,
            pivot,
            base_rotation,
            drawn,
            enabled,
            clip,
            default_focus,
            help_text,
            image: None,
            sprite: None,
            button: None,
            toggle: None,
            text: None,
            slider: None,
            frame: None,
            legacy_data: None,
            children: Vec::new(),
        };
        if let Some(n) = self.v.legacy_size(kind_byte) {
            ensure!(n != usize::MAX, "element type {kind_byte} in a {:?} layout", self.v);
            e.legacy_data = Some(Hex(r.bytes(n)?.to_vec()));
        } else {
            let ctx = self.ctx;
            match kind_byte {
                1 => {}
                2 => {
                    let early = matches!(self.v, Version::Early | Version::Earliest);
                    e.image = Some(ImageData {
                        vector: r.u8()?,
                        texture: res(r.u32()?, ctx),
                        rect: if early { None } else { Some([r.u16()?, r.u16()?, r.u16()?, r.u16()?]) },
                        texture_flags: TextureFlags(r.u8()?),
                    })
                }
                3 => e.sprite = Some(SpriteData { texture: res(r.u32()?, ctx), animation: res(r.u32()?, ctx), unused: r.u8()? }),
                4 => {
                    let flag = r.u8()?;
                    let a = r.u32()?;
                    let b = r.u32()?;
                    let hit_masks = match r.u8()? {
                        0 => None,
                        1 => {
                            let normal_size = [r.u16()?, r.u16()?];
                            let n = r.u32()? as usize;
                            let normal_mask = Hex(r.bytes(n)?.to_vec());
                            let active_size = [r.u16()?, r.u16()?];
                            let n = r.u32()? as usize;
                            let active_mask = Hex(r.bytes(n)?.to_vec());
                            Some(HitMasks { normal_size, normal_mask, active_size, active_mask })
                        }
                        v => bail!("button hit-mask flag {v}"),
                    };
                    let sounds = read_sounds(r, ctx)?;
                    e.button = Some(ButtonData { auto_repeat: flag, repeat_delay: a, repeat_interval: b, hit_masks, sounds });
                }
                5 => e.toggle = Some(ToggleData { checked: r.u8()?, sounds: read_sounds(r, ctx)? }),
                6 => {
                    e.text = Some(TextData {
                        font: r.u8()?,
                        align: HAlign::from_raw(r.u8()?),
                        valign: VAlign::from_raw(r.u8()?),
                        string: r.i32()?,
                        box_size: [r.i32()?, r.i32()?],
                        newline_to_space: r.u8()?,
                        color: [r.u8()?, r.u8()?, r.u8()?],
                        alpha: r.u8()?,
                        caret: r.u8()?,
                        caret_blink: r.u32()?,
                        focusable: r.u8()?,
                        autoscroll: r.u8()?,
                        scroll_pause: r.u32()?,
                        scroll_speed: r.u32()?,
                    })
                }
                7 => e.slider = Some(SliderData { vertical: r.u8()?, travel: r.i32()? }),
                8 => {
                    e.frame = Some(FrameData {
                        texture: res(r.u32()?, ctx),
                        inner_uv: [fx(r)?, fx(r)?, fx(r)?, fx(r)?],
                        border: fx(r)?,
                        unused: r.u32()?,
                    })
                }
                t => bail!("unknown element type {t}"),
            }
        }
        let n = self.r.u8()?;
        for _ in 0..n {
            let c = self.element()?;
            e.children.push(c);
        }
        Ok(e)
    }

    fn named_blob(&mut self) -> Result<NamedBlob> {
        let count = self.r.u8()?;
        let size = self.r.u32()? as usize;
        if count == 0 {
            ensure!(size == 0, "empty section with size {size}");
            return Ok((Vec::new(), Vec::new()));
        }
        let blob = self.r.bytes(size)?.to_vec();
        let names = (0..count).map(|_| Ok((self.r.str_u8()?, self.r.u32()?))).collect::<Result<_>>()?;
        Ok((blob, names))
    }
}

fn read_animation(r: &mut Reader, name: String, ctx: &Context) -> Result<UiAnimation> {
    let looping = r.u8()?;
    let n = r.u32()?;
    let mut tracks = Vec::new();
    for _ in 0..n {
        let element = r.str_u8()?;
        let k = r.u32()?;
        let mut keys = Vec::new();
        for _ in 0..k {
            let kind = r.u8()?;
            let ticks = r.i32()?;
            let change = match kind {
                0 => Change::Wait,
                1 => Change::Move { easing: Easing::from_raw(r.u8()?), x: fx(r)?, y: fx(r)? },
                2 => Change::Alpha { easing: Easing::from_raw(r.u8()?), alpha: r.u8()? },
                3 => Change::Scale { easing: Easing::from_raw(r.u8()?), x: fx(r)?, y: fx(r)? },
                4 => Change::Rotate { easing: Easing::from_raw(r.u8()?), degrees: r.i32()? },
                5 => Change::Enable,
                6 => Change::Disable,
                7 => Change::Check,
                8 => Change::Uncheck,
                9 => Change::Play { animation: r.str_u8()? },
                10 => Change::PlayOn { animation: r.str_u8()?, element: r.str_u8()? },
                12 => Change::SpawnEffect { name: r.str_u8()?, effect: res(r.u32()?, ctx) },
                13 => Change::RemoveChild { name: r.str_u8()? },
                14 => Change::Sound { sound: res(r.u32()?, ctx) },
                k => bail!("unknown keyframe kind {k}"),
            };
            keys.push(Key { ticks, change });
        }
        tracks.push(AnimTrack { element, keys });
    }
    Ok(UiAnimation { name, looping, tracks })
}

fn check_offsets(names: &[(String, u32)], starts: &[usize], what: &str) -> Result<()> {
    ensure!(names.len() == starts.len(), "{what}: {} names for {} entries", names.len(), starts.len());
    for ((n, o), s) in names.iter().zip(starts) {
        ensure!(*o as usize == *s, "{what} {n:?} at offset {o}, expected {s}");
    }
    Ok(())
}

fn decode_version(data: &[u8], v: Version, ctx: &Context) -> Result<UiLayout> {
    let mut d = Rd { r: Reader::new(data), ctx, v };
    let text = if v == Version::Earliest {
        None
    } else {
        res(d.r.u32()?, ctx)
    };
    let width = d.r.u16()?;
    let height = d.r.u16()?;
    let flag = d.r.u8()?;
    let n = d.r.u8()?;
    let elements = (0..n).map(|_| d.element()).collect::<Result<Vec<_>>>()?;

    let (blob, names) = d.named_blob()?;
    let mut templates = Vec::new();
    {
        let mut sub = Rd { r: Reader::new(&blob), ctx, v };
        let mut starts = Vec::new();
        let mut els = Vec::new();
        while !sub.r.at_end() {
            starts.push(sub.r.pos());
            els.push(sub.element()?);
        }
        check_offsets(&names, &starts, "template")?;
        for ((name, _), element) in names.into_iter().zip(els) {
            templates.push(Template { name, element });
        }
    }

    let mut animations = Vec::new();
    let mut constants = Vec::new();
    if v != Version::Earliest {
        let (blob, names) = d.named_blob()?;
        let mut r = Reader::new(&blob);
        let mut starts = Vec::new();
        let mut anims = Vec::new();
        while !r.at_end() {
            starts.push(r.pos());
            anims.push(read_animation(&mut r, String::new(), ctx)?);
        }
        check_offsets(&names, &starts, "animation")?;
        for ((name, _), mut a) in names.into_iter().zip(anims) {
            a.name = name;
            animations.push(a);
        }
        let n = d.r.u8()?;
        for _ in 0..n {
            constants.push(Constant { name: d.r.str_u8()?, value: d.r.u32()? });
        }
    }
    d.r.expect_end()?;
    Ok(UiLayout { version: v, text, width, height, keyboard_navigation: flag, elements, templates, animations, constants })
}

// ---------------------------------------------------------------------------------------------
// Writing

struct Wr<'a> {
    w: Writer,
    ctx: &'a Context,
    v: Version,
}

impl Wr<'_> {
    fn res(&mut self, r: &Option<ResRef>) -> Result<()> {
        let i = match r {
            Some(r) => r.to_index(self.ctx)?,
            None => u32::MAX,
        };
        self.w.u32(i);
        Ok(())
    }

    fn sounds(&mut self, s: &StateSounds) -> Result<()> {
        self.res(&s.hover_sound)?;
        self.res(&s.hover_fallback_sound)?;
        self.res(&s.press_sound)
    }

    fn element(&mut self, e: &Element) -> Result<()> {
        let kind = e.kind.to_u8();
        self.w.u8(kind);
        self.w.str_u8(&e.name)?;
        let w = &mut self.w;
        if self.v == Version::Earliest {
            ensure!(e.base_size.is_none() && fx_pair_zero(&e.pivot), "earliest layouts have no base_size/pivot");
            w.i32(e.position[0].0).i32(e.position[1].0).i32(e.scale[0].0).i32(e.scale[1].0);
            w.i32(e.rotation).i32(e.size[0]).i32(e.size[1]).i32(e.base_rotation);
        } else {
            let bs = e.base_size.unwrap_or(e.size);
            w.i32(e.position[0].0).i32(e.position[1].0).i32(e.scale[0].0).i32(e.scale[1].0);
            w.i32(e.size[0]).i32(e.size[1]).i32(e.rotation).i32(bs[0]).i32(bs[1]);
            w.i32(e.pivot[0].0).i32(e.pivot[1].0).i32(e.base_rotation);
        }
        w.bool(e.drawn).bool(e.enabled).bool(e.clip);
        if self.v == Version::Earliest {
            ensure!(!e.default_focus, "earliest layouts have no default_focus flag");
        } else {
            w.bool(e.default_focus);
        }
        if self.v.has_help_text() {
            w.i32(e.help_text);
        }
        if let Some(h) = &e.legacy_data {
            self.w.bytes(&h.0);
        } else {
            match kind {
                1 => {}
                2 => {
                    let d = e.image.as_ref().ok_or_else(|| anyhow!("image element {:?} needs `image`", e.name))?;
                    self.w.u8(d.vector);
                    self.res(&d.texture)?;
                    let early = matches!(self.v, Version::Early | Version::Earliest);
                    ensure!(d.rect.is_some() != early, "image {:?}: rect must be present exactly in non-early layouts", e.name);
                    for v in d.rect.into_iter().flatten() {
                        self.w.u16(v);
                    }
                    self.w.u8(d.texture_flags.0);
                }
                3 => {
                    let d = e.sprite.as_ref().ok_or_else(|| anyhow!("sprite element {:?} needs `sprite`", e.name))?;
                    self.res(&d.texture)?;
                    self.res(&d.animation)?;
                    self.w.u8(d.unused);
                }
                4 => {
                    let d = e.button.as_ref().ok_or_else(|| anyhow!("button element {:?} needs `button`", e.name))?;
                    self.w.u8(d.auto_repeat).u32(d.repeat_delay).u32(d.repeat_interval);
                    match &d.hit_masks {
                        None => {
                            self.w.u8(0);
                        }
                        Some(x) => {
                            self.w.u8(1).u16(x.normal_size[0]).u16(x.normal_size[1]).u32(x.normal_mask.0.len() as u32).bytes(&x.normal_mask.0);
                            self.w.u16(x.active_size[0]).u16(x.active_size[1]).u32(x.active_mask.0.len() as u32).bytes(&x.active_mask.0);
                        }
                    }
                    self.sounds(&d.sounds)?;
                }
                5 => {
                    let d = e.toggle.as_ref().ok_or_else(|| anyhow!("toggle element {:?} needs `toggle`", e.name))?;
                    self.w.u8(d.checked);
                    self.sounds(&d.sounds)?;
                }
                6 => {
                    let d = e.text.as_ref().ok_or_else(|| anyhow!("text element {:?} needs `text`", e.name))?;
                    let w = &mut self.w;
                    w.u8(d.font).u8(d.align.to_raw()).u8(d.valign.to_raw()).i32(d.string).i32(d.box_size[0]).i32(d.box_size[1]);
                    w.u8(d.newline_to_space).bytes(&d.color).u8(d.alpha).u8(d.caret).u32(d.caret_blink);
                    w.u8(d.focusable).u8(d.autoscroll).u32(d.scroll_pause).u32(d.scroll_speed);
                }
                7 => {
                    let d = e.slider.as_ref().ok_or_else(|| anyhow!("slider element {:?} needs `slider`", e.name))?;
                    self.w.u8(d.vertical).i32(d.travel);
                }
                8 => {
                    let d = e.frame.as_ref().ok_or_else(|| anyhow!("frame element {:?} needs `frame`", e.name))?;
                    self.res(&d.texture)?;
                    for v in d.inner_uv {
                        self.w.i32(v.0);
                    }
                    self.w.i32(d.border.0);
                    self.w.u32(d.unused);
                }
                t => bail!("unknown element type {t}"),
            }
        }
        self.w.u8(e.children.len() as u8);
        for c in &e.children {
            self.element(c)?;
        }
        Ok(())
    }
}

fn write_named_blob(w: &mut Writer, blob: &[u8], names: &[(&str, usize)]) -> Result<()> {
    w.u8(names.len() as u8).u32(blob.len() as u32).bytes(blob);
    for (n, o) in names {
        w.str_u8(n)?.u32(*o as u32);
    }
    Ok(())
}

fn write_animation(w: &mut Writer, a: &UiAnimation, ctx: &Context) -> Result<()> {
    w.u8(a.looping).u32(a.tracks.len() as u32);
    for t in &a.tracks {
        w.str_u8(&t.element)?.u32(t.keys.len() as u32);
        for k in &t.keys {
            let kind = match &k.change {
                Change::Wait => 0,
                Change::Move { .. } => 1,
                Change::Alpha { .. } => 2,
                Change::Scale { .. } => 3,
                Change::Rotate { .. } => 4,
                Change::Enable => 5,
                Change::Disable => 6,
                Change::Check => 7,
                Change::Uncheck => 8,
                Change::Play { .. } => 9,
                Change::PlayOn { .. } => 10,
                Change::SpawnEffect { .. } => 12,
                Change::RemoveChild { .. } => 13,
                Change::Sound { .. } => 14,
            };
            w.u8(kind).i32(k.ticks);
            match &k.change {
                Change::Move { easing, x, y } | Change::Scale { easing, x, y } => {
                    w.u8(easing.to_raw()).i32(x.0).i32(y.0);
                }
                Change::Alpha { easing, alpha } => {
                    w.u8(easing.to_raw()).u8(*alpha);
                }
                Change::Rotate { easing, degrees } => {
                    w.u8(easing.to_raw()).i32(*degrees);
                }
                Change::Play { animation } => {
                    w.str_u8(animation)?;
                }
                Change::PlayOn { animation, element } => {
                    w.str_u8(animation)?.str_u8(element)?;
                }
                Change::SpawnEffect { name, effect } => {
                    w.str_u8(name)?.u32(match effect {
                        Some(r) => r.to_index(ctx)?,
                        None => u32::MAX,
                    });
                }
                Change::RemoveChild { name } => {
                    w.str_u8(name)?;
                }
                Change::Sound { sound } => {
                    w.u32(match sound {
                        Some(r) => r.to_index(ctx)?,
                        None => u32::MAX,
                    });
                }
                Change::Wait | Change::Enable | Change::Disable | Change::Check | Change::Uncheck => {}
            }
        }
    }
    Ok(())
}

impl Format for UiLayout {
    const NAME: &'static str = "uib";
    const DESCRIPTION: &'static str = "UI layout: element tree, templates, animations, constants";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut first_err = None;
        for v in [Version::Current, Version::NoHelpText, Version::Early, Version::Earliest] {
            match decode_version(data, v, ctx) {
                Ok(l) => return Ok(l),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        Err(first_err.unwrap())
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let v = self.version;
        let mut wr = Wr { w: Writer::new(), ctx, v };
        if v != Version::Earliest {
            let t = match &self.text {
                Some(r) => r.to_index(ctx)?,
                None => u32::MAX,
            };
            wr.w.u32(t);
        } else {
            ensure!(self.text.is_none(), "earliest layouts have no text reference");
        }
        wr.w.u16(self.width).u16(self.height).u8(self.keyboard_navigation).u8(self.elements.len() as u8);
        for e in &self.elements {
            wr.element(e)?;
        }

        let mut sub = Wr { w: Writer::new(), ctx, v };
        let mut names = Vec::new();
        for t in &self.templates {
            names.push((t.name.as_str(), sub.w.pos()));
            sub.element(&t.element)?;
        }
        write_named_blob(&mut wr.w, &sub.w.buf, &names)?;

        if v == Version::Earliest {
            ensure!(self.animations.is_empty() && self.constants.is_empty(), "earliest layouts have no animations or constants");
        } else {
            let mut blob = Writer::new();
            let mut names = Vec::new();
            for a in &self.animations {
                names.push((a.name.as_str(), blob.pos()));
                write_animation(&mut blob, a, ctx)?;
            }
            write_named_blob(&mut wr.w, &blob.buf, &names)?;
            wr.w.u8(self.constants.len() as u8);
            for c in &self.constants {
                wr.w.str_u8(&c.name)?.u32(c.value);
            }
        }
        Ok(wr.w.into_inner())
    }
}
