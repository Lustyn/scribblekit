//! Event scripts: extensionless files in `data\events\[region]\` (`…_intro`, `…_$textboxes`,
//! `…_cinematic`, …) and a few placeholders under `_menu\bitmap\_scripts\[region]\`. Each file
//! holds one or more cutscene/dialogue scripts in all 7 text languages (4 in the Wii U build; the
//! file does not say which, so the count comes from [`Context::platform`](scribble_core::Context::platform)).
//!
//! ```text
//! u16 count                          // scripts * L (L = 7 languages, 4 on Wii U; 0 in placeholders)
//! n x { u16 index, u32 offset }      // index == record number; n = (offset[0] - 2) / 6
//! u8  bytecode[..]                   // record i covers [offset[i], offset[i+1])
//! ```
//!
//! Record `script * 7 + language` is the bytecode of `script` in that language (languages in
//! [`LANGUAGES`](crate::text::LANGUAGES) order); the game reads the u32 at
//! `(script * 7 + language) * 6 + 4` (`FUN_00459f90`).
//!
//! The bytecode (interpreted by `FUN_00459d10`) is a sequence of one-letter opcodes, each
//! followed by its operands. `E` starts a *track*: the actions after it run on one actor.
//! `|` ends the first part: `FUN_00459d10` returns 1 there and its only caller `FUN_00459f90`
//! calls it again on the rest, which resets the current actor and track, so both parts build
//! the same sequencer the same way (shipped scripts keep text boxes before the bar and camera /
//! actor tracks after it, the event editor's layout). `/` ends the script (return 0) and is
//! followed by `///` padding. Other bytes hit the switch's default case and are skipped.
//!
//! | op  | operands                                   | meaning                                  |
//! |-----|--------------------------------------------|------------------------------------------|
//! | `E` | i32 actor                                  | start a track (-1 none, -2 player, …)    |
//! | `C` | i32 x, i32 y, u8 anchor, i32 duration      | pan camera to (x, y)                     |
//! | `Z` | u32 zoom (0..4096), i32 duration           | zoom camera between min and max          |
//! | `M` | i32 x, i32 y, i32 duration                 | move the actor to (x, y)                 |
//! | `T` | i32 duration                               | wait                                     |
//! | `N` | i32 duration                               | extend the script length only            |
//! | `G` | u8 wave, i32 duration                      | release a survival wave                  |
//! | `W` | u8 flags, [object filter], i32 duration    | start the survival round                 |
//! | `I` | see [`TextBox`]                            | show a text box                          |
//! | `+` | str8 function, u8 argc, argc x str8 arg     | call a named script function             |
//! | `@` | u8 type, operands, u32 (skipped)           | run an object-editor action              |
//!
//! Durations are in game ticks (60/s). `@` actions are the same action classes scribble objects
//! and scene scripts use (factory `FUN_0064e1f0`, reader = vtable slot 9), decoded with the
//! shared schema of [`fmt_object::behaviour`] ([`fmt_object::behaviour::ACTIONS`]).

use crate::named::int_enum;
use crate::text::{language_count, Localized};
use fmt_object::refs::Filter;
use scribble_core::{anyhow, bail, ensure, Context, Format, Hex, Reader, Result, Writer};
use serde::{Deserialize, Serialize};

/// A file of localized event scripts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventScripts {
    /// The stored script count when it differs from `scripts.len() * 7` (placeholder files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_count: Option<u16>,
    pub scripts: Vec<Languages<Slot>>,
}

/// The 7 language versions of a script, collapsed to one when they are identical.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Languages<T> {
    Same { all_languages: T },
    Each(Localized<T>),
}

impl<T: Clone + PartialEq> Languages<T> {
    fn new(l: Localized<T>) -> Result<Self> {
        let a = l.slots()?;
        Ok(if a.iter().all(|x| *x == a[0]) { Languages::Same { all_languages: l.english } } else { Languages::Each(l) })
    }
    /// The value for each of `langs` languages, in file order.
    pub fn each(&self, langs: usize) -> Result<Vec<&T>> {
        match self {
            Languages::Same { all_languages } => Ok(vec![all_languages; langs]),
            Languages::Each(l) => {
                let v = l.slots()?;
                ensure!(v.len() == langs, "script has {} languages, expected {langs}", v.len());
                Ok(v)
            }
        }
    }
}

/// One record: a script, or a record whose offset points nowhere.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Slot {
    /// A record whose offset is outside the file (the event editor writes `0xabcd` for
    /// languages it did not export); never read.
    Missing { missing_offset: u32 },
    /// Bytecode that does not re-encode exactly (no shipped script: the editor's `|T…B` stubs
    /// sit in empty text tables, see [`TextTable`](crate::TextTable)).
    Raw { raw: Hex },
    Script(Script),
}

/// A script: tracks before the `|` and tracks after it (parsed identically, see the module
/// docs).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Script {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_bar: Vec<Track>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_bar: Vec<Track>,
}

/// `E actor` followed by the actions it performs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Scene entity (`FUN_00458920`; placed-object index, or 0xFF0000nn for a group); -1 = no
    /// actor, -2 = the player, -3 = the level's starite.
    pub actor: i32,
    pub actions: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// `C`: pan the camera (`FUN_00458760` -> sequencer node `FUN_006d50a0`). Only the low 24
    /// bits of x/y are used.
    CameraPan {
        x: i32,
        y: i32,
        #[serde(default = "top_left", skip_serializing_if = "is_top_left")]
        anchor: CameraAnchor,
        duration: i32,
    },
    /// `Z`: zoom; 0 = closest, 4096 = farthest.
    CameraZoom { zoom: u32, duration: i32 },
    /// `M`: move the actor. Only the low 24 bits of x/y are used.
    MoveTo { x: i32, y: i32, duration: i32 },
    /// `T`
    Wait { duration: i32 },
    /// `N`: adds to the script's length without creating an action.
    Delay { duration: i32 },
    /// `G`: releases survival wave `wave` (an index into the scene's `survival_waves`;
    /// sequencer node type 0x20, run by `FUN_006d6420` -> `FUN_00703450`).
    SpawnWave { wave: u8, duration: i32 },
    /// `W`: starts the survival round (sequencer node type 0x1f, `FUN_0070f890`);
    /// `advance_round` = flag bit 0. Flag bit 1 means an object filter follows, which the
    /// engine parses and discards.
    StartWave {
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        advance_round: bool,
        /// Flag bits 2-7: `FUN_00459210` tests only bits 0 and 1 (zero in shipped scripts).
        #[serde(default, skip_serializing_if = "is_zero_u8")]
        unused_flag_bits: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<Filter>,
        duration: i32,
    },

    /// `I`
    TextBox(TextBox),
    /// `+`
    Call { function: String, args: Vec<String> },
    /// `@`
    Action(EventAction),
}

/// `I`: a dialogue/info box (`FUN_00459a90`).
///
/// ```text
/// u8 style ('d');     if 'c' or 'd': u8[2]
/// u8 align ('c');     if 'u': u8[2]
/// u8 title_format;    if != 0: str8 title
/// u8 extra;           if != 0: u8           // never set
/// u16 page_count
/// page_count x { u8 line_count; line_count x { u8 format, str8 text } }
/// i32 duration
/// ```
/// `FUN_00459a90` only compares the style byte with 'c'/'d' and the align byte with 'u' to
/// know how many bytes to skip, and skips the extra byte's operand and every line's format
/// byte; none of these values is used (they are the event editor's settings: style 'd',
/// align 'c', line formats 15/12, title format 14). The title (non-zero format byte = present)
/// goes to `FUN_00456f40`, the lines to `FUN_006d5fa0`, the duration extends the script.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextBox {
    #[serde(rename = "unused_style")]
    pub style: char,
    #[serde(default, rename = "unused_style_args", skip_serializing_if = "Option::is_none")]
    pub style_args: Option<[u8; 2]>,
    #[serde(rename = "unused_align")]
    pub align: char,
    #[serde(default, rename = "unused_align_args", skip_serializing_if = "Option::is_none")]
    pub align_args: Option<[u8; 2]>,
    /// Title line; its format byte doubles as the has-title flag (never 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<Line>,
    /// Non-zero flag byte and the byte after it, both skipped (never set in shipped scripts).
    #[serde(default, rename = "unused_extra", skip_serializing_if = "Option::is_none")]
    pub extra: Option<[u8; 2]>,
    /// Pages of lines. Text keeps its inline style codes (`ci…ic`).
    pub pages: Vec<Vec<Line>>,
    pub duration: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// Editor line format; skipped by `FUN_00459a90` (only a title's non-zero value matters).
    #[serde(rename = "unused_format")]
    pub format: u8,
    pub text: String,
}

int_enum! {
    /// Which screen point a `C` pan puts at (x, y): `FUN_006d50a0` picks one of five
    /// target functions (`FUN_006d4c20`, `FUN_006d4cc0`, `FUN_006d4ce0`, `FUN_006d4d20`,
    /// `FUN_006d4d70`) that subtract none, half or all of the zoomed screen width/height.
    pub enum CameraAnchor: u8 {
        /// 0: (x, y) becomes the screen centre.
        Center = 0,
        /// 1 (and any unknown value): the top-left corner.
        TopLeft = 1,
        /// 2: the top-right corner.
        TopRight = 2,
        /// 3: the bottom-left corner.
        BottomLeft = 3,
        /// 4: the bottom-right corner.
        BottomRight = 4,
    }
}

fn top_left() -> CameraAnchor {
    CameraAnchor::TopLeft
}
fn is_top_left(v: &CameraAnchor) -> bool {
    *v == CameraAnchor::TopLeft
}

/// `@`: an object-editor action — the same classes (and JSON) as the actions of scribble
/// objects and scene scripts ([`fmt_object::Action`], factory `FUN_0064e1f0`), followed by a
/// u32 the interpreter skips.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventAction {
    #[serde(flatten)]
    pub action: fmt_object::Action,
    /// The u32 after the action: `FUN_004589e0` parses the action (slot 9) and then just adds 4
    /// to the cursor (1 in almost all scripts, 0 or -1 in a few).
    #[serde(default = "one", rename = "unused_trailer", skip_serializing_if = "is_one")]
    pub trailing: u32,
}

fn one() -> u32 {
    1
}
fn is_one(v: &u32) -> bool {
    *v == 1
}

fn read_action(r: &mut Reader, ctx: &Context) -> Result<EventAction> {
    let action = fmt_object::Action::read(r, ctx)?;
    Ok(EventAction { action, trailing: r.u32()? })
}

fn write_action(w: &mut Writer, a: &EventAction, ctx: &Context) -> Result<()> {
    a.action.write(w, ctx)?;
    w.u32(a.trailing);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Bytecode

fn read_text_box(r: &mut Reader) -> Result<TextBox> {
    let style = r.u8()? as char;
    let style_args = if matches!(style, 'c' | 'd') { Some([r.u8()?, r.u8()?]) } else { None };
    let align = r.u8()? as char;
    let align_args = if align == 'u' { Some([r.u8()?, r.u8()?]) } else { None };
    let title = match r.u8()? {
        0 => None,
        format => Some(Line { format, text: r.str_u8()? }),
    };
    let extra = match r.u8()? {
        0 => None,
        v => Some([v, r.u8()?]),
    };
    let pages = (0..r.u16()?)
        .map(|_| {
            let n = r.u8()?;
            (0..n).map(|_| Ok(Line { format: r.u8()?, text: r.str_u8()? })).collect::<Result<Vec<_>>>()
        })
        .collect::<Result<_>>()?;
    let duration = r.i32()?;
    Ok(TextBox { style, style_args, align, align_args, title, extra, pages, duration })
}

fn write_text_box(w: &mut Writer, t: &TextBox) -> Result<()> {
    w.u8(char_byte(t.style)?);
    if matches!(t.style, 'c' | 'd') {
        let s = t.style_args.ok_or_else(|| anyhow!("unused_style_args required for style {:?}", t.style))?;
        w.u8(s[0]).u8(s[1]);
    }
    w.u8(char_byte(t.align)?);
    if t.align == 'u' {
        let a = t.align_args.ok_or_else(|| anyhow!("unused_align_args required for align 'u'"))?;
        w.u8(a[0]).u8(a[1]);
    }
    match &t.title {
        None => {
            w.u8(0);
        }
        Some(l) => {
            ensure!(l.format != 0, "title format must be non-zero");
            w.u8(l.format).str_u8(&l.text)?;
        }
    }
    match t.extra {
        None => {
            w.u8(0);
        }
        Some([a, b]) => {
            ensure!(a != 0, "extra[0] must be non-zero");
            w.u8(a).u8(b);
        }
    }
    w.u16(t.pages.len() as u16);
    for p in &t.pages {
        w.u8(p.len() as u8);
        for l in p {
            w.u8(l.format).str_u8(&l.text)?;
        }
    }
    w.i32(t.duration);
    Ok(())
}

fn char_byte(c: char) -> Result<u8> {
    u8::try_from(c as u32).map_err(|_| anyhow!("{c:?} is not a byte"))
}

fn read_op(r: &mut Reader, code: u8, ctx: &Context) -> Result<Op> {
    Ok(match code {
        b'C' => Op::CameraPan { x: r.i32()?, y: r.i32()?, anchor: CameraAnchor::from_raw(r.u8()?), duration: r.i32()? },
        b'Z' => Op::CameraZoom { zoom: r.u32()?, duration: r.i32()? },
        b'M' => Op::MoveTo { x: r.i32()?, y: r.i32()?, duration: r.i32()? },
        b'T' => Op::Wait { duration: r.i32()? },
        b'N' => Op::Delay { duration: r.i32()? },
        b'G' => Op::SpawnWave { wave: r.u8()?, duration: r.i32()? },
        b'W' => {
            let flags = r.u8()?;
            let filter = if flags & 2 != 0 { Some(Filter::read(r, ctx)?) } else { None };
            Op::StartWave { advance_round: flags & 1 != 0, unused_flag_bits: flags & !3, filter, duration: r.i32()? }
        }
        b'I' => Op::TextBox(read_text_box(r)?),
        b'+' => {
            let function = r.str_u8()?;
            let n = r.u8()?;
            Op::Call { function, args: (0..n).map(|_| r.str_u8()).collect::<Result<_>>()? }
        }
        b'@' => Op::Action(read_action(r, ctx)?),
        c => bail!("unknown opcode {:?}", c as char),
    })
}

fn write_op(w: &mut Writer, op: &Op, ctx: &Context) -> Result<()> {
    match op {
        Op::CameraPan { x, y, anchor, duration } => {
            w.u8(b'C').i32(*x).i32(*y).u8(anchor.to_raw()).i32(*duration);
        }
        Op::CameraZoom { zoom, duration } => {
            w.u8(b'Z').u32(*zoom).i32(*duration);
        }
        Op::MoveTo { x, y, duration } => {
            w.u8(b'M').i32(*x).i32(*y).i32(*duration);
        }
        Op::Wait { duration } => {
            w.u8(b'T').i32(*duration);
        }
        Op::Delay { duration } => {
            w.u8(b'N').i32(*duration);
        }
        Op::SpawnWave { wave: value, duration } => {
            w.u8(b'G').u8(*value).i32(*duration);
        }
        Op::StartWave { advance_round, unused_flag_bits, filter, duration } => {
            ensure!(unused_flag_bits & 3 == 0, "unused_flag_bits bits 0-1 are advance_round and the filter");
            w.u8(b'W').u8(unused_flag_bits | *advance_round as u8 | (filter.is_some() as u8) << 1);
            if let Some(o) = filter {
                o.write(w, ctx)?;
            }
            w.i32(*duration);
        }
        Op::TextBox(t) => {
            w.u8(b'I');
            write_text_box(w, t)?;
        }
        Op::Call { function, args } => {
            w.u8(b'+').str_u8(function)?.u8(args.len() as u8);
            for a in args {
                w.str_u8(a)?;
            }
        }
        Op::Action(a) => {
            w.u8(b'@');
            write_action(w, a, ctx)?;
        }
    }
    Ok(())
}

const END: &[u8] = b"////";

fn read_script(data: &[u8], ctx: &Context) -> Result<Script> {
    let mut r = Reader::new(data);
    let mut parts: [Vec<Track>; 2] = Default::default();
    let mut part = 0;
    loop {
        let code = r.u8()?;
        match code {
            b'/' => break,
            b'|' => {
                ensure!(part == 0, "second '|'");
                part = 1;
            }
            b'E' => parts[part].push(Track { actor: r.i32()?, actions: Vec::new() }),
            _ => {
                let op = read_op(&mut r, code, ctx)?;
                parts[part].last_mut().ok_or_else(|| anyhow!("action before the first 'E'"))?.actions.push(op);
            }
        }
    }
    ensure!(part == 1, "script without '|'");
    ensure!(r.rest() == &END[1..], "script does not end with '////'");
    let [before_bar, after_bar] = parts;
    Ok(Script { before_bar, after_bar })
}

fn write_script(w: &mut Writer, s: &Script, ctx: &Context) -> Result<()> {
    for (i, part) in [&s.before_bar, &s.after_bar].into_iter().enumerate() {
        if i == 1 {
            w.u8(b'|');
        }
        for t in part {
            w.u8(b'E').i32(t.actor);
            for op in &t.actions {
                write_op(w, op, ctx)?;
            }
        }
    }
    w.bytes(END);
    Ok(())
}

/// Decode one record's bytecode, falling back to raw bytes if it does not re-encode exactly.
fn decode_slot(data: &[u8], ctx: &Context) -> Slot {
    if let Ok(s) = read_script(data, ctx) {
        let mut w = Writer::new();
        if write_script(&mut w, &s, ctx).is_ok() && w.buf == data {
            return Slot::Script(s);
        }
    }
    Slot::Raw { raw: Hex(data.to_vec()) }
}

impl EventScripts {
    /// Whether `data` has the event-script layout (a u16 count, then `(index, offset)` records).
    pub fn detect(data: &[u8]) -> bool {
        if data.len() == 2 {
            return data == [0, 0];
        }
        if data.len() < 8 || data[2..4] != [0, 0] {
            return false;
        }
        let first = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        first >= 8 && (first - 2).is_multiple_of(6) && first <= data.len()
    }
}

impl Format for EventScripts {
    const NAME: &'static str = "event_script";
    const DESCRIPTION: &'static str = "Localized event/cutscene scripts (bytecode)";

    fn decode(data: &[u8], ctx: &Context) -> Result<Self> {
        let mut r = Reader::new(data);
        let count = r.u16()?;
        let records = if r.at_end() {
            0
        } else {
            let first = u32::from_le_bytes(r.clone().bytes(6)?[2..6].try_into().unwrap()) as usize;
            ensure!(first >= 2 && (first - 2).is_multiple_of(6), "bad first offset {first:#x}");
            (first - 2) / 6
        };
        let langs = language_count(ctx.platform());
        ensure!(records % langs == 0, "{records} records is not a multiple of {langs} languages");
        let mut offsets = Vec::with_capacity(records);
        for i in 0..records {
            let idx = r.u16()?;
            ensure!(idx as usize == i, "record {i} has index {idx}");
            offsets.push(r.u32()? as usize);
        }
        // Valid records are those whose offsets increase through the file.
        let mut valid = vec![false; records];
        let mut last = r.pos();
        for (i, &o) in offsets.iter().enumerate() {
            if o >= last && o < data.len() {
                valid[i] = true;
                last = o;
            }
        }
        let mut slots = Vec::with_capacity(records);
        for i in 0..records {
            if !valid[i] {
                slots.push(Slot::Missing { missing_offset: offsets[i] as u32 });
                continue;
            }
            let end = (i + 1..records).find(|&j| valid[j]).map(|j| offsets[j]).unwrap_or(data.len());
            slots.push(decode_slot(&data[offsets[i]..end], ctx));
        }
        if let Some(first) = (0..records).find(|&i| valid[i]) {
            ensure!(offsets[first] == 2 + 6 * records, "gap between header and first script");
        }
        let mut it = slots.into_iter();
        let scripts = (0..records / langs)
            .map(|_| Localized::from_vec(it.by_ref().take(langs).collect()).and_then(Languages::new))
            .collect::<Result<_>>()?;
        let declared_count = (count as usize != records).then_some(count);
        Ok(EventScripts { declared_count, scripts })
    }

    fn encode(&self, ctx: &Context) -> Result<Vec<u8>> {
        let langs = language_count(ctx.platform());
        let records = self.scripts.len() * langs;
        let mut w = Writer::new();
        w.u16(self.declared_count.unwrap_or(records as u16));
        let table = w.pos();
        for i in 0..records {
            w.u16(i as u16).u32(0);
        }
        let mut i = 0;
        for s in &self.scripts {
            for slot in s.each(langs)? {
                let at = table + 6 * i + 2;
                match slot {
                    Slot::Missing { missing_offset } => w.patch_u32(at, *missing_offset),
                    Slot::Raw { raw } => {
                        let o = w.pos() as u32;
                        w.patch_u32(at, o);
                        w.bytes(&raw.0);
                    }
                    Slot::Script(sc) => {
                        let o = w.pos() as u32;
                        w.patch_u32(at, o);
                        write_script(&mut w, sc, ctx)?;
                    }
                }
                i += 1;
            }
        }
        Ok(w.into_inner())
    }
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
