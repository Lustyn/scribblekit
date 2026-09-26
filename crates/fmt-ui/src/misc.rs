//! Small one-off formats: `.swc` palette, `.stl` tag list, `fasttravel.dat` world map graph,
//! `.uit` UI layout source, and the credits text `wb.txt`.

use scribble_core::bin::latin1;
use scribble_core::{ensure, Context, Format, Hex, Reader, Result, Writer};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ---------------------------------------------------------------------------------------------
// Colours

/// An 8-bit RGBA colour, written `"#rrggbbaa"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let [r, g, b, a] = self.0;
        s.serialize_str(&format!("#{r:02x}{g:02x}{b:02x}{a:02x}"))
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let h = s.strip_prefix('#').unwrap_or(&s);
        if h.len() != 8 {
            return Err(serde::de::Error::custom(format!("expected #rrggbbaa, got {s:?}")));
        }
        let mut out = [0u8; 4];
        for (i, o) in out.iter_mut().enumerate() {
            *o = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).map_err(serde::de::Error::custom)?;
        }
        Ok(Rgba(out))
    }
}

// ---------------------------------------------------------------------------------------------
// .swc

/// `.swc`: colour-slot overrides applied to the shadow world (`FUN_006e43e0`, called with the
/// palette's resource index 0x206f). Each entry replaces palette slot `slot` with `color`
/// (converted to floats /255).
///
/// ```text
/// u8  flags          // bit 0: apply the palette at all
/// i32 count          // entries the game reads
/// n x { u8 slot, u8 r, u8 g, u8 b, u8 a }   // n may exceed count: the rest is ignored
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    /// Flag bit 0: `FUN_006e43e0` applies the entries only if it is set.
    pub enabled: bool,
    /// Flag bits 1-7, never tested (`FUN_006e43e0` checks `*data & 1` only).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub unused_flag_bits: u8,
    /// The first `count` entries: the loop in `FUN_006e43e0` runs `count` (u32 at +1) times.
    pub colors: Vec<PaletteEntry>,
    /// Entries past `count`, present in the file but never read by the game.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unused_entries: Vec<PaletteEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaletteEntry {
    /// Palette slot: `FUN_006e43e0` writes the colour to `table + slot * 16`.
    pub slot: u8,
    /// RGBA, each channel stored as `byte * DAT_00829ac8` (1/255) floats.
    pub color: Rgba,
}

impl Format for Palette {
    const NAME: &'static str = "swc";
    const DESCRIPTION: &'static str = "Shadow-world palette slot overrides";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let flags = r.u8()?;
        let count = r.i32()?;
        ensure!(count >= 0, "negative palette count");
        let mut entries = Vec::new();
        while !r.at_end() {
            entries.push(PaletteEntry { slot: r.u8()?, color: Rgba(r.array()?) });
        }
        ensure!(count as usize <= entries.len(), "palette count {count} exceeds the {} entries present", entries.len());
        let unused_entries = entries.split_off(count as usize);
        Ok(Palette { enabled: flags & 1 != 0, unused_flag_bits: flags & !1, colors: entries, unused_entries })
    }

    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        ensure!(self.unused_flag_bits & 1 == 0, "bit 0 of the flags is `enabled`");
        w.u8(self.unused_flag_bits | self.enabled as u8).i32(self.colors.len() as i32);
        for e in self.colors.iter().chain(&self.unused_entries) {
            w.u8(e.slot).bytes(&e.color.0);
        }
        Ok(w.into_inner())
    }
}

// ---------------------------------------------------------------------------------------------
// .stl

/// `.stl`: a list of scribble-object tag ids (e.g. the object categories allowed in Rumble and
/// Survival). Loaded by `FUN_007122b0` (resources 0x20a3 `rumbleandsurvival`, 0x20a4
/// `rumblecategories`), which copies the ids into an array and allocates a matching bitset.
///
/// ```text
/// u8  unused         // FUN_007122b0 starts reading at byte 1; always 0
/// u8  count
/// u32 tag[count]
/// ```
///
/// `test.stl` (0x20a5, referenced nowhere in the code) is instead a `5CPF` container as written
/// by `FUN_007af2a0` — a GIGL-area serializer with no callers: magic `"5CPF\r\n\x1a\n"`, then
/// `u32 size, "DATA", data`, …, `u32 0, "FEND"`. The shipped copy had every lone LF expanded
/// to CRLF (the magic's `\x1a\n` and the size byte 0x0a became `\x1a\r\n` / `\r\n`), the
/// PNG-style damage the magic is built to detect. Its body is kept as bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TagList {
    Tags {
        #[serde(default, skip_serializing_if = "is_zero_u8")]
        unused: u8,
        /// Tag ids, printed as tag names.
        tags: Vec<scribble_core::NamedId>,
    },
    /// `"5CPF\r\n\x1a\r\n"` followed by the container body; never loaded.
    Cpf { unused_cpf_body: Hex },
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

const CPF_MAGIC: &[u8] = b"5CPF\r\n\x1a\r\n";

impl Format for TagList {
    const NAME: &'static str = "stl";
    const DESCRIPTION: &'static str = "Scribble-object tag id list";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        if let Some(body) = data.strip_prefix(CPF_MAGIC) {
            return Ok(TagList::Cpf { unused_cpf_body: Hex(body.to_vec()) });
        }
        let mut r = Reader::new(data);
        let unused = r.u8()?;
        let n = r.u8()?;
        let tags = (0..n).map(|_| Ok(scribble_core::NamedId::from_id(r.u32()?, scribble_core::ns::TAG, None, ctx))).collect::<Result<_>>()?;
        r.expect_end()?;
        Ok(TagList::Tags { unused, tags })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        match self {
            TagList::Tags { unused, tags } => {
                ensure!(tags.len() <= 255, "at most 255 tags");
                w.u8(*unused).u8(tags.len() as u8);
                for t in tags {
                    w.u32(t.to_id(scribble_core::ns::TAG, None, ctx)?);
                }
            }
            TagList::Cpf { unused_cpf_body } => {
                w.bytes(CPF_MAGIC).bytes(&unused_cpf_body.0);
            }
        }
        Ok(w.into_inner())
    }
}

// ---------------------------------------------------------------------------------------------
// fasttravel.dat

/// `data\_menu\fasttravel\fasttravel.dat`: the fast-travel world map — one node per level with
/// the stick directions that lead to neighbouring levels. Loaded by `FUN_00489910`
/// (resource index 0x24d3).
///
/// ```text
/// u32 node_count
/// node_count x {
///     u8  name_len, char name[name_len]     // level text id, e.g. "S_SEAFRONT"
///     u16 level
///     i16 link_count
///     link_count x {
///         i8  kind
///         i16 dx, i16 dy                    // direction on the map
///         kind 0: u16 level                 // neighbouring level
///         kind 2: u8 len, char name[len]
///     }
/// }
/// ```
///
/// `FUN_00489910` copies each link into a 0x50-byte record `{i32 dx, i32 dy, i32 kind,
/// u16 level, char name[64]}` (`FUN_00488b60`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FastTravelMap {
    pub nodes: Vec<FastTravelNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FastTravelNode {
    /// Level text id; the fast-travel menu compares it with its `S_*` constants (`FUN_00489be0`,
    /// `FUN_00486f50`).
    pub name: String,
    pub level: u16,
    pub links: Vec<FastTravelLink>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FastTravelLink {
    /// 0 = link to a level (`FUN_00486f50`/`FUN_004872b0` count kind-0 links and test the
    /// target level's bit in the save profile's unlock bitset, `FUN_00646ff0() + 0x3b`);
    /// 2 = named target (the loader reads a name; only once, empty, in `S_PYRAMID`); other
    /// kinds carry no target.
    pub kind: i8,
    /// Stick direction that selects the link (link +0, +4).
    pub direction: [i16; 2],
    /// Target level (kind 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u16>,
    /// Target name (kind 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl Format for FastTravelMap {
    const NAME: &'static str = "fasttravel";
    const DESCRIPTION: &'static str = "Fast-travel world map: level nodes and their links";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let n = r.u32()?;
        let mut nodes = Vec::new();
        for _ in 0..n {
            let name = r.str_u8()?;
            let level = r.u16()?;
            let count = r.i16()?;
            ensure!(count >= 0, "negative link count");
            let mut links = Vec::new();
            for _ in 0..count {
                let kind = r.i8()?;
                let direction = [r.i16()?, r.i16()?];
                let (level, name) = match kind {
                    0 => (Some(r.u16()?), None),
                    2 => (None, Some(r.str_u8()?)),
                    _ => (None, None),
                };
                links.push(FastTravelLink { kind, direction, level, name });
            }
            nodes.push(FastTravelNode { name, level, links });
        }
        r.expect_end()?;
        Ok(FastTravelMap { nodes })
    }

    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        let mut w = Writer::new();
        w.u32(self.nodes.len() as u32);
        for n in &self.nodes {
            w.str_u8(&n.name)?;
            w.u16(n.level).i16(n.links.len() as i16);
            for l in &n.links {
                w.i8(l.kind).i16(l.direction[0]).i16(l.direction[1]);
                match l.kind {
                    0 => {
                        w.u16(l.level.ok_or_else(|| scribble_core::anyhow!("kind 0 link needs a level"))?);
                    }
                    2 => {
                        w.str_u8(l.name.as_deref().ok_or_else(|| scribble_core::anyhow!("kind 2 link needs a name"))?)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(w.into_inner())
    }
}

// ---------------------------------------------------------------------------------------------
// Text files split into lines

fn split_lines(text: &str) -> Option<Vec<String>> {
    // Lossless only if every line break is CRLF.
    let bare = text.replace("\r\n", "");
    if bare.contains('\r') || bare.contains('\n') {
        return None;
    }
    Some(text.split("\r\n").map(str::to_string).collect())
}

/// `.uit`: the XML source of a UI layout (`<UI_Layout>` with `<Layer>`, `<Image>`, `<Button>`,
/// `<TextBox>` elements), as exported by the menu tool. The file is ASCII/UTF-8 text with CRLF
/// line breaks, preceded by a stray UTF-16 byte-order mark (`ff fe`). Its only instance
/// (`fast_travel.uit`, 0x64e8) is referenced nowhere in the code: the game loads the compiled
/// `.uib`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayoutSource {
    /// Whether the text is preceded by the (wrong, UTF-16) byte-order mark `ff fe`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub utf16_bom: bool,
    /// The text, one entry per CRLF-terminated line.
    pub lines: Vec<String>,
}

impl Format for LayoutSource {
    const NAME: &'static str = "uit";
    const DESCRIPTION: &'static str = "UI layout XML source (text)";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let (utf16_bom, body) = match data.strip_prefix(b"\xff\xfe") {
            Some(b) => (true, b),
            None => (false, data),
        };
        let text = std::str::from_utf8(body).map_err(|e| scribble_core::anyhow!("layout source is not UTF-8: {e}"))?;
        let lines = split_lines(text).ok_or_else(|| scribble_core::anyhow!("mixed line endings"))?;
        Ok(LayoutSource { utf16_bom, lines })
    }

    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        let mut out = if self.utf16_bom { b"\xff\xfe".to_vec() } else { Vec::new() };
        out.extend_from_slice(self.lines.join("\r\n").as_bytes());
        Ok(out)
    }
}

/// `data\creditsdata\wb.txt`: Windows-1252 credits text, one CRLF-terminated line per credit.
/// Each line starts with style codes (`c`, `l`, `b`, …) followed by the text. Its resource index
/// (0x1e8a) appears nowhere in the code or data section, so no reader is known; the style
/// codes are read off the data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreditsText {
    pub lines: Vec<String>,
}

impl Format for CreditsText {
    const NAME: &'static str = "credits";
    const DESCRIPTION: &'static str = "Credits roll text (lines with style prefixes)";

    fn decode(data: &[u8], _ctx: &Context) -> Result<Self> {
        let lines = split_lines(&latin1(data)).ok_or_else(|| scribble_core::anyhow!("mixed line endings"))?;
        Ok(CreditsText { lines })
    }

    fn encode(&self, _ctx: &Context) -> Result<Vec<u8>> {
        scribble_core::bin::to_latin1(&self.lines.join("\r\n"))
    }
}
