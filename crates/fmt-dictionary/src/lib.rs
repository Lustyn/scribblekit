//! Scribblenauts Unlimited word dictionaries: how typed words become objects, adjectives and
//! tags.
//!
//! Per-file codecs (all lossless):
//!
//! | file (`data\_game\scribbleobjects\d\`)       | type                          |
//! |-----------------------------------------------|-------------------------------|
//! | `object_dictionary_<lang>`, `tag_dictionary_<lang>` | [`WordList`]            |
//! | `object_dictionary_{danish,finnish,norwegian,swedish}` | [`LegacyObjectDictionary`] |
//! | `object_single_word_<lang>`                   | [`SingleWordIndex`]           |
//! | `*_jumptable_<lang>`, `*_word_id_table_<lang>`, `related_objects_jumptable` | [`OffsetTable`] |
//! | `related_objects_dictionary`                  | [`RelatedObjectsLists`]       |
//! | `details_file_<lang>`                         | [`Details`]                   |
//! | `<lang>.dtm`                                  | [`TranslationMap`]            |
//! | `scribbleadjectives\dictionary\adjective_dictionary_<lang>` | [`WordList`] (legacy layout) |
//! | `scribbleadjectives\dictionary\adjective_{jumptable,word_id_table}_<lang>` | [`OffsetTable`] |
//!
//! For editing, use the high-level models, which load all related files and regenerate every
//! derived table consistently: [`Dictionary`] (one language), [`RelatedObjects`] and
//! [`LegacyAdjectiveDictionary`]. Binary layouts are documented in [`wordfile`], [`tables`].

pub mod dictionary;
pub mod tables;
pub mod wordfile;
pub mod words;

pub use dictionary::{
    DICTIONARY_DIR, Dictionary, ENGINE_LANGUAGES, Entry, LANGUAGES, LEGACY_ADJECTIVE_DIR, LEGACY_LANGUAGES, LegacyAdjectiveDictionary,
    RelatedList, RelatedObjects, ResourceSource, Sense, TagDictionary, Target, Translations, WordLink, WordRef, dir_source, normalize_word,
    write_files,
};
pub use tables::{Choice, DetailEntry, Details, OffsetTable, RelatedObjectsLists, Translation, TranslationMap};
pub use wordfile::{Gender, Layout, Properties, WordKind};
pub use words::{KindSet, LegacyObjectDictionary, Meaning, SingleWord, SingleWordIndex, Word, WordList};

use scribble_core::{Codec, ResPath, codec};

fn lang_of<'a>(file: &'a str, prefix: &str) -> Option<&'a str> {
    file.strip_prefix(prefix).filter(|l| !l.is_empty() && !l.contains('.'))
}

/// The codec for a resource, if this crate handles it.
pub fn handler(path: &ResPath, _data: &[u8]) -> Option<&'static dyn Codec> {
    let f = path.file.as_str();
    match path.dir.as_str() {
        "data\\_game\\scribbleobjects\\d" => {
            if let Some(lang) = lang_of(f, "object_dictionary_") {
                return Some(if LEGACY_LANGUAGES.contains(&lang) { codec!(LegacyObjectDictionary) } else { codec!(WordList) });
            }
            if lang_of(f, "tag_dictionary_").is_some() {
                return Some(codec!(WordList));
            }
            if lang_of(f, "object_single_word_").is_some() {
                return Some(codec!(SingleWordIndex));
            }
            if lang_of(f, "details_file_").is_some() {
                return Some(codec!(Details));
            }
            if f == "related_objects_dictionary" {
                return Some(codec!(RelatedObjectsLists));
            }
            if f == "related_objects_jumptable"
                || ["object", "adjective", "tag"].iter().any(|k| {
                    lang_of(f, &format!("{k}_jumptable_")).is_some() || lang_of(f, &format!("{k}_word_id_table_")).is_some()
                })
            {
                return Some(codec!(OffsetTable));
            }
            if path.ext == "dtm" {
                return Some(codec!(TranslationMap));
            }
            None
        }
        "data\\_game\\scribbleadjectives\\dictionary" => {
            if lang_of(f, "adjective_dictionary_").is_some() {
                Some(codec!(WordList))
            } else if lang_of(f, "adjective_jumptable_").is_some() || lang_of(f, "adjective_word_id_table_").is_some() {
                Some(codec!(OffsetTable))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Every codec this crate provides.
pub fn codecs() -> Vec<&'static dyn Codec> {
    vec![
        codec!(WordList),
        codec!(LegacyObjectDictionary),
        codec!(SingleWordIndex),
        codec!(OffsetTable),
        codec!(RelatedObjectsLists),
        codec!(Details),
        codec!(TranslationMap),
    ]
}
