//! Which codec handles which resource.
//!
//! Resources are identified by their logical path (`data\_game\scribbleobjects\cow.so`), since
//! several formats have no extension and are only distinguishable by directory or name. Each
//! `fmt-*` crate exposes a `handler(path, data)` that claims the resources it understands.

pub mod context;

pub use context::{install_test_context, load_context, save_names, NAMES_FILE};
pub use scribble_core::ResPath as Path;
use scribble_core::{Codec, ResPath};

/// How a resource is represented when decoded to the human-readable tree.
pub enum Handler {
    /// A game-specific format, decoded to JSON by a [`Codec`].
    Codec(&'static dyn Codec),
    /// A standard format that is already readable with ordinary tools; exported unchanged
    /// under a conventional extension (e.g. `.[texture]` -> `.dds`).
    Standard { extension: &'static str, description: &'static str },
}

const FORMAT_CRATES: &[fn(&ResPath, &[u8]) -> Option<&'static dyn Codec>] = &[
    fmt_common::handler,
    fmt_object::handler,
    fmt_dictionary::handler,
    fmt_vec::handler,
    fmt_anim::handler,
    fmt_map::handler,
    fmt_ui::handler,
    fmt_effects::handler,
];

fn standard(p: &ResPath, data: &[u8]) -> Option<(&'static str, &'static str)> {
    Some(match p.ext.as_str() {
        "[texture]" => ("dds", "DirectDraw Surface texture"),
        "png" => ("png", "PNG image"),
        "psd" => ("psd", "Photoshop document"),
        "gp" => ("gp", "HLSL/Cg GPU program source (text)"),
        "asi" | "flt" => ("dll", "Miles Sound System plugin (Windows DLL)"),
        // 31-byte bundle header (u32 data offset, u32 count, u32 0, str8 name, u32 size) followed
        // by a Scaleform GFx (SWF v8, "GFX" signature) font library movie.
        "bff" => ("bff", "Scaleform GFx font library in a bundle header"),
        // Most audio is Bink Audio ("1FCB") despite the extension; a few are real RIFF WAVs.
        "wav" if data.starts_with(b"1FCB") => ("binka", "Bink Audio (decodable with ffmpeg)"),
        "wav" => ("wav", "RIFF WAVE audio"),
        _ => return None,
    })
}

/// Find the handler for a logical resource path. `data` lets handlers sniff magic numbers.
///
/// Only `fmt-ui` looks at the bytes (text tables and event scripts share directories); with
/// empty `data` every other resource still resolves by path alone.
pub fn handler_for(logical_path: &str, data: &[u8]) -> Option<Handler> {
    let p = ResPath::new(logical_path);
    if let Some((extension, description)) = standard(&p, data) {
        return Some(Handler::Standard { extension, description });
    }
    if let Some(c) = FORMAT_CRATES.iter().find_map(|h| h(&p, data)) {
        return Some(Handler::Codec(c));
    }
    // data\_game\dummy\dummy_NN: zero-length placeholders.
    data.is_empty().then_some(Handler::Standard { extension: "", description: "empty placeholder" })
}

/// Look a codec up by its [`Codec::name`].
pub fn codec_by_name(name: &str) -> Option<&'static dyn Codec> {
    codecs().into_iter().find(|c| c.name() == name)
}

/// Every codec any format crate can return, for listing. Crates list theirs via `codecs()`.
pub fn codecs() -> Vec<&'static dyn Codec> {
    let mut v = Vec::new();
    v.extend(fmt_common::codecs());
    v.extend(fmt_object::codecs());
    v.extend(fmt_dictionary::codecs());
    v.extend(fmt_vec::codecs());
    v.extend(fmt_anim::codecs());
    v.extend(fmt_map::codecs());
    v.extend(fmt_ui::codecs());
    v.extend(fmt_effects::codecs());
    v
}
