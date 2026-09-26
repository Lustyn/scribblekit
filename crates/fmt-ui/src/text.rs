//! Localized text tables: extensionless files in `data\events\[region]\` (`static_worldnames`,
//! `…_$merits`, `…_$hintboxes`, …) and `[platform]\_menu\bitmap\_scripts\[region]\` (one per menu).
//!
//! Every table holds the same `n` strings in each of the game's 7 text languages (4 in the Wii U
//! build, see [`WIIU_LANGUAGES`]; the header size tells them apart). The lookup
//! `FUN_0049a710(table, i)` returns "" unless `i < n`, else the string at
//! `offset[lang * n + i]`, where `lang = *(DAT_008a82fc + 4)` is the global language index
//! (0..6). The same index selects `details_file_*` in `FUN_0073c880`: 0 english, 1 dutch,
//! 2 french, 3 german, 4 italian, 5 spanish_mexico, 6 portuguese_brazil — the order of
//! [`LANGUAGES`].
//!
//! ```text
//! u32 n
//! u32 offset[L * n]          // L = 7 languages (4 on Wii U); language-major: offset[lang * n + i]
//! char text[..]              // NUL-terminated Windows-1252 strings, in offset order
//! ```
//!
//! Strings are stored back to back in offset order. The engine reaches strings only through
//! the offsets, so bytes between strings are never read: a handful of one-entry tables carry
//! a few such stale bytes before the second string (fragments of the event editor's stub
//! `…cd ab 00 00 7c 54 01 00 00 00 42`), kept in [`Text::WithJunk`]. Likewise 91 empty tables
//! (`n = 0`: menus without strings, levels without merits) are followed by a whole stale
//! event-script stub, kept in [`TextTable::unused_trailer`].

use scribble_core::{bail, ensure, Context, Format, Hex, Platform, Reader, Result, Writer};
use serde::{Deserialize, Serialize};

/// Names of the 7 text-language slots of the PC build, in file order.
pub const LANGUAGES: [&str; 7] = ["english", "dutch", "french", "german", "italian", "spanish", "portuguese"];
/// Names of the 4 text-language slots of the Wii U (USA) build, in file order (`HELP`, `AIDE`,
/// `AJUDA`, `AYUDA` in `storefilter`).
pub const WIIU_LANGUAGES: [&str; 4] = ["english", "french", "portuguese", "spanish"];

/// Number of text-language slots in a build's tables and event scripts.
pub fn language_count(platform: Platform) -> usize {
    match platform {
        Platform::Pc => LANGUAGES.len(),
        Platform::WiiU => WIIU_LANGUAGES.len(),
    }
}

/// One value per text language: the PC build's 7 (English, Dutch, French, German, Italian,
/// Mexican Spanish, Brazilian Portuguese — the order of `details_file_*` selected by
/// `FUN_0073c880`: 0x1e25, 0x1e24, 0x1e28, 0x1e29, 0x1e2a, 0x1e2e, 0x1e2c), or the Wii U build's
/// 4, which lack Dutch, German and Italian (see [`WIIU_LANGUAGES`] for their order).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Localized<T> {
    pub english: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dutch: Option<T>,
    pub french: T,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub german: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italian: Option<T>,
    pub spanish: T,
    pub portuguese: T,
}

impl<T> Localized<T> {
    /// From the values in file order: 7 (PC) or 4 (Wii U).
    pub fn from_vec(v: Vec<T>) -> Result<Self> {
        let n = v.len();
        let mut it = v.into_iter();
        let mut next = || it.next().unwrap();
        Ok(match n {
            7 => {
                let (english, dutch, french, german, italian, spanish, portuguese) = (next(), next(), next(), next(), next(), next(), next());
                Localized { english, dutch: Some(dutch), french, german: Some(german), italian: Some(italian), spanish, portuguese }
            }
            4 => {
                let (english, french, portuguese, spanish) = (next(), next(), next(), next());
                Localized { english, dutch: None, french, german: None, italian: None, spanish, portuguese }
            }
            _ => bail!("expected 7 (PC) or 4 (Wii U) languages, got {n}"),
        })
    }

    /// The values in file order.
    pub fn slots(&self) -> Result<Vec<&T>> {
        Ok(match (&self.dutch, &self.german, &self.italian) {
            (Some(dutch), Some(german), Some(italian)) => {
                vec![&self.english, dutch, &self.french, german, italian, &self.spanish, &self.portuguese]
            }
            (None, None, None) => vec![&self.english, &self.french, &self.portuguese, &self.spanish],
            _ => bail!("dutch, german and italian must be all present (PC) or all absent (Wii U)"),
        })
    }
}

/// A string of a text table.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Text {
    Plain(String),
    /// The string is preceded by stray bytes that the game never reads (`FUN_0049a710` jumps
    /// straight to the string's offset).
    WithJunk {
        #[serde(rename = "unused_before")]
        junk_before: Hex,
        text: String,
    },
}

impl Text {
    fn parts(&self) -> (&[u8], &str) {
        match self {
            Text::Plain(s) => (&[], s),
            Text::WithJunk { junk_before, text } => (&junk_before.0, text),
        }
    }
}

/// A localized string table (see the module docs for the layout).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextTable {
    /// String `i` in every language.
    pub strings: Vec<Localized<Text>>,
    /// Bytes after the `n = 0` header of an empty table: a stale event-editor stub
    /// (`u16 0`, 7 index records, `|T 1 B`). Never read: `FUN_0049a710` returns "" for every
    /// index when `n = 0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unused_trailer: Option<Hex>,
}

impl TextTable {
    /// Whether `data` looks like a text table (as opposed to an [`EventScripts`](crate::EventScripts) file).
    pub fn detect(data: &[u8]) -> bool {
        if data.len() < 8 {
            return false;
        }
        let n = u32::from_le_bytes(data[0..4].try_into().unwrap()) as u64;
        let first = u32::from_le_bytes(data[4..8].try_into().unwrap()) as u64;
        // An event-script file starts with u16 count, u16 index 0, so a zero first u32 means an
        // empty text table (an event file always has records when it has more than 2 bytes).
        n == 0 || ((first == 4 + 28 * n || first == 4 + 16 * n) && first <= data.len() as u64)
    }
}

impl Format for TextTable {
    const NAME: &'static str = "text_table";
    const DESCRIPTION: &'static str = "Localized string table (7 languages, 4 on Wii U)";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let n = r.u32()? as usize;
        if n == 0 {
            let rest = r.bytes(data.len() - 4)?;
            return Ok(TextTable { strings: Vec::new(), unused_trailer: (!rest.is_empty()).then(|| Hex(rest.to_vec())) });
        }
        let first = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        let langs = (first - 4) / (4 * n);
        ensure!(first == 4 + 4 * langs * n && (langs == 7 || langs == 4), "header of {first:#x} bytes is not 7 or 4 languages of {n} strings");
        let offsets = (0..langs * n).map(|_| r.u32().map(|o| o as usize)).collect::<Result<Vec<_>>>()?;
        let mut pos = r.pos();
        let mut texts = Vec::with_capacity(langs * n);
        for &off in &offsets {
            ensure!(off >= pos && off < data.len(), "string offset {off:#x} out of order (expected >= {pos:#x})");
            let junk = &data[pos..off];
            r.seek(off)?;
            let text = r.cstr()?;
            pos = r.pos();
            texts.push(if junk.is_empty() { Text::Plain(text) } else { Text::WithJunk { junk_before: Hex(junk.to_vec()), text } });
        }
        r.expect_end()?;
        // language-major -> per-string
        let mut per_lang: Vec<std::vec::IntoIter<Text>> = Vec::new();
        let mut it = texts.into_iter();
        for _ in 0..langs {
            per_lang.push(it.by_ref().take(n).collect::<Vec<_>>().into_iter());
        }
        let strings = (0..n)
            .map(|_| Localized::from_vec(per_lang.iter_mut().map(|l| l.next().unwrap()).collect()))
            .collect::<Result<_>>()?;
        Ok(TextTable { strings, unused_trailer: None })
    }

    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        let n = self.strings.len();
        if n == 0 {
            let mut w = Writer::new();
            w.u32(0);
            if let Some(h) = &self.unused_trailer {
                w.bytes(&h.0);
            }
            return Ok(w.into_inner());
        }
        ensure!(self.unused_trailer.is_none(), "unused_trailer is only kept for empty tables");
        let slots = self.strings.iter().map(Localized::slots).collect::<Result<Vec<_>>>()?;
        let langs = slots[0].len();
        ensure!(slots.iter().all(|s| s.len() == langs), "every string must have the same languages");
        let mut w = Writer::new();
        w.u32(n as u32);
        let table = w.pos();
        w.zeros(4 * langs * n);
        for lang in 0..langs {
            for (i, s) in slots.iter().enumerate() {
                let (junk, text) = s[lang].parts();
                w.bytes(junk);
                let off = w.pos() as u32;
                w.patch_u32(table + 4 * (lang * n + i), off);
                w.cstr(text)?;
            }
        }
        Ok(w.into_inner())
    }
}
