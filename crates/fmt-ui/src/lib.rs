//! UI, text and event-script formats.
//!
//! * extensionless files in `data\events\[region]\` and `[platform]\_menu\bitmap\_scripts\[region]\`:
//!   either a [`TextTable`] (localized strings) or [`EventScripts`] (cutscene/dialogue bytecode),
//!   told apart by their header.
//! * `.uib` — [`UiLayout`]: menu layout (element tree, templates, animations, constants).
//! * `.sfb` — [`SpriteFrames`]: flipbook cells/frames/loops over a texture strip.
//! * `.uit` — [`LayoutSource`]: XML source of a UI layout.
//! * `.swc` — [`Palette`], `.stl` — [`TagList`], `fasttravel.dat` — [`FastTravelMap`],
//!   `creditsdata\wb.txt` — [`CreditsText`].
//!
//! Not handled here: `[platform]\flash\gfxfontlib.bff` is a 31-byte bundle header
//! (`u32 data_offset = 31, u32 file_count = 1, u32 0, str8 "gfxfontlib.gfx", u32 size`)
//! followed by one Scaleform GFx movie (SWF v8 with the `GFX` signature) — a standard format.

pub mod event;
pub mod misc;
mod named;
pub mod sfb;
pub mod text;
pub mod uib;

pub use event::EventScripts;
pub use misc::{CreditsText, FastTravelMap, LayoutSource, Palette, TagList};
pub use sfb::SpriteFrames;
pub use text::TextTable;
pub use uib::UiLayout;

use scribble_core::{codec, Codec, ResPath};

/// Directories whose extensionless files are text tables or event scripts.
pub fn is_text_dir(dir: &str) -> bool {
    dir == "data\\events\\[region]" || dir == "[platform]\\_menu\\bitmap\\_scripts\\[region]"
}

/// The codec for a resource, if this crate handles it.
pub fn handler(path: &ResPath, data: &[u8]) -> Option<&'static dyn Codec> {
    match path.ext.as_str() {
        "" if is_text_dir(&path.dir) => {
            if TextTable::detect(data) {
                Some(codec!(TextTable))
            } else if EventScripts::detect(data) {
                Some(codec!(EventScripts))
            } else {
                None
            }
        }
        "uib" => Some(codec!(UiLayout)),
        "sfb" => Some(codec!(SpriteFrames)),
        "uit" => Some(codec!(LayoutSource)),
        "swc" => Some(codec!(Palette)),
        "stl" => Some(codec!(TagList)),
        "dat" if path.file == "fasttravel.dat" => Some(codec!(FastTravelMap)),
        "txt" if path.dir == "data\\creditsdata" => Some(codec!(CreditsText)),
        _ => None,
    }
}

/// Every codec this crate provides.
pub fn codecs() -> Vec<&'static dyn Codec> {
    vec![
        codec!(TextTable),
        codec!(EventScripts),
        codec!(UiLayout),
        codec!(SpriteFrames),
        codec!(LayoutSource),
        codec!(Palette),
        codec!(TagList),
        codec!(FastTravelMap),
        codec!(CreditsText),
    ]
}
