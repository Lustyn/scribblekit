//! Building the full [`Context`] for an unpacked game: resource names plus the named id
//! namespaces (see [`scribble_core::ns`]) that codecs use to print ids readably.
//!
//! Everything is derived from the game's own files:
//!
//! * **Object taxonomy** (`object.category`, `object.subcategory`, `object.group`, `object`):
//!   every `.so` starts with its 4 ids and is named `<category>_<subcategory>_<group>_<object>.so`.
//!   Names may contain underscores, so for each level the name is the one split of the file name
//!   that is consistent across every file sharing that level's id (e.g. `gameplay_gameonly_editor__zone`
//!   = `gameplay` / `gameonly` / `editor` / `_zone`). The special objects `_self_self_me.so`,
//!   `_stage_stage_stageobject.so`, ... have an empty category name, printed as `_`.
//! * **Adjective taxonomy** (`adjective.category`, `adjective.group`, `adjective`): every `.sa`
//!   carries its 3 ids at byte 4; leaf names are the `.sa` file names. Category and group names
//!   come from the custom filters `asadjbycat_<category>.exf` / `asadjbysubcat_<category>__<group>.exf`,
//!   whose contents are the adjectives of that category/group. The `gameplay` filters ship empty;
//!   their ids are named from the adjectives they contain (see [`ADJECTIVE_FALLBACK`]).
//! * **Tags** (`tag`): the English tag dictionary; each id is named after the word the game
//!   displays for it (its jump-table entry).
//! * **Merits** (`merit`): the merit databases (`data\merits\*.mdb`, led by `everything.mdb`,
//!   which lists almost every merit with its global id) and the title strings of their text
//!   tables.
//!
//! Ids without a unique name stay numeric (see [`scribble_core::IdNames`]), so a missing or
//! changed file only makes the output less readable, never lossy.

use scribble_core::{ns, Context, Format, IdNames, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// Name tables saved next to `manifest.json` in a decoded tree (see [`save_names`]), where the
/// binary files the namespaces are derived from have been replaced by JSON.
pub const NAMES_FILE: &str = "names.json";

/// Load the resource names and every id namespace for an unpacked directory (the one holding
/// `manifest.json`) or a decoded tree (which also holds [`NAMES_FILE`]). For a game directory
/// (only `pmindex.xml`), just the resource names.
pub fn load_context(dir: &Path) -> Result<Context> {
    let mut ctx = scribble_pack::load_context(dir)?;
    if dir.join(NAMES_FILE).exists() {
        load_names(&mut ctx, &dir.join(NAMES_FILE))?;
    } else if dir.join(scribble_pack::manifest::MANIFEST).exists() {
        let read = |logical: &str| std::fs::read(scribble_pack::manifest::disk_path(dir, logical)).ok();
        add_namespaces(&mut ctx, &read);
    }
    Ok(ctx)
}

/// Write every namespace of `ctx` as readable JSON:
/// `{"object.category": [[16, "mammal"], ...], "object.subcategory": [[1562, "large", 16], ...]}`
/// (`[id, name]` or `[id, name, parent id]`).
pub fn save_names(ctx: &Context, path: &Path) -> Result<()> {
    let mut names: Vec<&str> = ctx.namespace_names().collect();
    names.sort();
    let mut out = String::from("{\n");
    for (i, ns_name) in names.iter().enumerate() {
        let ids = ctx.namespace(ns_name).unwrap();
        out.push_str(&format!("  {}: [\n", serde_json::to_string(ns_name)?));
        let entries = ids.entries();
        for (j, (id, name, parent)) in entries.iter().enumerate() {
            let row = match parent {
                Some(p) => serde_json::json!([id, name, p]),
                None => serde_json::json!([id, name]),
            };
            out.push_str(&format!("    {}{}\n", row, if j + 1 < entries.len() { "," } else { "" }));
        }
        out.push_str(if i + 1 < names.len() { "  ],\n" } else { "  ]\n" });
    }
    out.push_str("}\n");
    std::fs::write(path, out)?;
    Ok(())
}

fn load_names(ctx: &mut Context, path: &Path) -> Result<()> {
    let v: BTreeMap<String, Vec<serde_json::Value>> = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    for (ns_name, rows) in v {
        let mut ids = IdNames::new();
        for row in rows {
            let id = row[0].as_u64().ok_or_else(|| scribble_core::anyhow!("{ns_name}: bad id in {row}"))? as u32;
            let name = row[1].as_str().ok_or_else(|| scribble_core::anyhow!("{ns_name}: bad name in {row}"))?;
            ids.insert(id, name, row.get(2).and_then(|p| p.as_u64()).map(|p| p as u32));
        }
        ctx.set_namespace(ns_name, ids);
    }
    Ok(())
}

/// Install [`load_context`] as the loader behind `scribble_core::testing::context()`.
pub fn install_test_context() {
    scribble_core::testing::set_context_loader(load_context);
}

/// Derive every namespace from the files of `ctx`'s resources (`read` returns a resource's
/// bytes by logical path).
pub fn add_namespaces(ctx: &mut Context, read: &dyn Fn(&str) -> Option<Vec<u8>>) {
    let paths: Vec<(u32, String)> = ctx.resources().map(|(i, p)| (i, p.to_string())).collect();
    for (name, ids) in ns::OBJECT_PATH.iter().zip(object_taxonomy(&paths, read)) {
        ctx.set_namespace(*name, ids);
    }
    for (name, ids) in ns::ADJECTIVE_PATH.iter().zip(adjective_taxonomy(&paths, read)) {
        ctx.set_namespace(*name, ids);
    }
    if let Some(tags) = tags(ctx, read) {
        ctx.set_namespace(ns::TAG, tags);
    }
    if let Some(merits) = merits(ctx, read) {
        ctx.set_namespace(ns::MERIT, merits);
    }
}

fn stem<'a>(path: &'a str, dir: &str, ext: &str) -> Option<&'a str> {
    let (d, f) = path.rsplit_once('\\')?;
    if !d.eq_ignore_ascii_case(dir) {
        return None;
    }
    let (s, e) = f.rsplit_once('.')?;
    e.eq_ignore_ascii_case(ext).then_some(s)
}

fn u16s(b: &[u8], at: usize, n: usize) -> Option<Vec<u32>> {
    (0..n).map(|i| Some(u16::from_le_bytes(b.get(at + 2 * i..at + 2 * i + 2)?.try_into().ok()?) as u32)).collect()
}

/// The four object taxonomy levels.
fn object_taxonomy(paths: &[(u32, String)], read: &dyn Fn(&str) -> Option<Vec<u8>>) -> [IdNames; 4] {
    // (ids, file stem)
    let mut files: Vec<([u32; 4], String)> = Vec::new();
    for (_, path) in paths {
        let Some(name) = stem(path, "data\\_game\\scribbleobjects", "so") else { continue };
        let Some(ids) = read(path).and_then(|b| u16s(&b, 0, 4)) else { continue };
        files.push((ids.try_into().unwrap(), name.to_lowercase()));
    }
    // Candidate names of the first three levels: every split of the stem into 4 parts at
    // underscores, intersected over all files with the same id at that level.
    let mut candidates: [BTreeMap<u32, BTreeSet<String>>; 3] = Default::default();
    for (ids, name) in &files {
        let us: Vec<usize> = name.match_indices('_').map(|(i, _)| i).collect();
        let mut sets: [BTreeSet<String>; 3] = Default::default();
        for a in 0..us.len() {
            for b in a + 1..us.len() {
                for c in b + 1..us.len() {
                    let (a, b, c) = (us[a], us[b], us[c]);
                    sets[0].insert(name[..a].to_string());
                    sets[1].insert(name[a + 1..b].to_string());
                    sets[2].insert(name[b + 1..c].to_string());
                }
            }
        }
        for (level, set) in sets.into_iter().enumerate() {
            candidates[level].entry(ids[level]).and_modify(|s| s.retain(|x| set.contains(x))).or_insert(set);
        }
    }
    // Prefer a non-empty name without a trailing underscore (`editor` over `editor_` and ``,
    // which only arise from the `__` before names such as `_zone`).
    let mut names: [BTreeMap<u32, String>; 3] = Default::default();
    for level in 0..3 {
        for (id, set) in &candidates[level] {
            let good: Vec<&String> = set.iter().filter(|s| !s.is_empty() && !s.ends_with('_')).collect();
            let pick = match good.as_slice() {
                [one] => Some((*one).clone()),
                [] if set.len() == 1 => set.iter().next().cloned(),
                _ => None,
            };
            if let Some(p) = pick {
                names[level].insert(*id, p);
            }
        }
    }
    let mut out: [IdNames; 4] = Default::default();
    for (ids, name) in &files {
        let prefix: Option<Vec<&String>> = (0..3).map(|l| names[l].get(&ids[l])).collect();
        let Some(prefix) = prefix else { continue };
        let prefix = prefix.iter().map(|s| format!("{s}_")).collect::<String>();
        if let Some(object) = name.strip_prefix(&prefix) {
            out[3].insert(ids[3], object, Some(ids[2]));
        }
        for level in 0..3 {
            let n = &names[level][&ids[level]];
            let printed = if n.is_empty() { "_" } else { n.as_str() };
            let parent = (level > 0).then(|| ids[level - 1]);
            if out[level].name(ids[level]).is_none() {
                out[level].insert(ids[level], printed, parent);
            }
        }
    }
    out
}

/// Names of adjective categories/groups whose `asadjbycat_`/`asadjbysubcat_` filters ship
/// empty (`gameplay`): assigned from the adjectives in each group (`_nogravity`, `_bouncy` =
/// physics; `_background`, `_foreground` = placement; `_rumbleexplode`, `_survival*` = rumble;
/// `_globalmerithelper*` = globalmerit; the rest = other). `(level, id, name, parent)`.
pub const ADJECTIVE_FALLBACK: &[(usize, u32, &str, Option<u32>)] = &[
    (0, 1392, "gameplay", None),
    (1, 1393, "other", Some(1392)),
    (1, 1395, "physics", Some(1392)),
    (1, 1399, "placement", Some(1392)),
    (1, 1932, "rumble", Some(1392)),
    (1, 2007, "globalmerit", Some(1392)),
];

fn adjective_taxonomy(paths: &[(u32, String)], read: &dyn Fn(&str) -> Option<Vec<u8>>) -> [IdNames; 3] {
    let mut out: [IdNames; 3] = Default::default();
    let mut header: HashMap<u32, [u32; 3]> = HashMap::new();
    for (index, path) in paths {
        let Some(name) = stem(path, "data\\_game\\scribbleadjectives", "sa") else { continue };
        let Some(ids) = read(path).and_then(|b| u16s(&b, 4, 3)) else { continue };
        let ids: [u32; 3] = ids.try_into().unwrap();
        header.insert(*index, ids);
        out[2].insert(ids[2], name.to_lowercase(), Some(ids[1]));
    }
    // The ids shared by every adjective a filter lists.
    let common = |path: &str, levels: usize| -> Option<Vec<u32>> {
        let b = read(path)?;
        let n = u32::from_le_bytes(b.get(..4)?.try_into().ok()?) as usize;
        let items = u16s(&b, 4, n)?;
        let mut shared: Option<Vec<u32>> = None;
        for i in items {
            let h = header.get(&i)?[..levels].to_vec();
            match &shared {
                Some(s) if *s != h => return None,
                _ => shared = Some(h),
            }
        }
        shared
    };
    let clean = |s: &str| s.trim().replace(' ', "_");
    for (_, path) in paths {
        if let Some(cat) = stem(path, "data\\customfilters", "exf").and_then(|s| s.strip_prefix("asadjbycat_"))
            && let Some(ids) = common(path, 1)
        {
            out[0].insert(ids[0], clean(cat), None);
        }
        if let Some(rest) = stem(path, "data\\customfilters", "exf").and_then(|s| s.strip_prefix("asadjbysubcat_"))
            && let (Some((_, group)), Some(ids)) = (rest.split_once("__"), common(path, 2))
        {
            out[1].insert(ids[1], clean(group), Some(ids[0]));
        }
    }
    for &(level, id, name, parent) in ADJECTIVE_FALLBACK {
        if out[level].name(id).is_none() && header.values().any(|h| h[level] == id) {
            out[level].insert(id, name, parent);
        }
    }
    out
}

/// Tag ids named after the word the game displays for them (the English tag jump table's entry,
/// e.g. `FOOD` for the id whose words are `CHOW`, `DIET`, `FARE`, `FOOD`, `MEAL`).
fn tags(ctx: &Context, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<IdNames> {
    let dict = fmt_dictionary::Dictionary::load("english", ctx, |p: &str| Ok(read(p))).ok()?;
    let mut ids = IdNames::new();
    for (id, target) in dict.tags.as_ref()?.tags.iter() {
        if let Some(name) = target.name.as_deref().map(snake).filter(|n| !n.is_empty()) {
            ids.insert(*id as u32, name, None);
        }
    }
    Some(ids)
}

/// Merit ids named after their titles. `data\merits\everything.mdb` lists (almost) every
/// merit with its global id; the level databases use the same ids (checked: all 524 shared
/// ids have the same title) and add a few, which are named when all levels agree. Each merit
/// owns three strings of its database's text table, the title first.
fn merits(ctx: &Context, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<IdNames> {
    let mut paths: Vec<&str> = ctx.resources().map(|(_, p)| p).filter(|p| stem(p, "data\\merits", "mdb").is_some()).collect();
    paths.sort_by_key(|p| !p.ends_with("\\everything.mdb"));
    let mut titles: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    let mut primary: BTreeMap<u32, String> = BTreeMap::new();
    for path in paths {
        let Some(db) = read(path).and_then(|b| fmt_map::MeritDatabase::decode(&b, ctx).ok()) else { continue };
        let Some(table) = db.text_table.as_ref().and_then(|t| t.to_index(ctx).ok()).and_then(|i| ctx.name(i)) else { continue };
        let Some(table) = read(table).and_then(|b| fmt_ui::TextTable::decode(&b, ctx).ok()) else { continue };
        for m in &db.merits {
            let Some(title) = table.strings.get(m.text_index as usize) else { continue };
            let text = match &title.english {
                fmt_ui::text::Text::Plain(s) => s.as_str(),
                fmt_ui::text::Text::WithJunk { text, .. } => text.as_str(),
            };
            let name = snake(text);
            if path.ends_with("\\everything.mdb") {
                primary.entry(m.id as u32).or_insert(name);
            } else {
                titles.entry(m.id as u32).or_default().insert(name);
            }
        }
    }
    let mut ids = IdNames::new();
    for (id, name) in &primary {
        ids.insert(*id, name.clone(), None);
    }
    for (id, names) in titles {
        if !primary.contains_key(&id) && names.len() == 1 {
            ids.insert(id, names.into_iter().next().unwrap(), None);
        }
    }
    Some(ids)
}

/// `"FARM ANIMALS!"` -> `"farm_animals"`.
fn snake(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c == '\'' {
            continue;
        }
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}
