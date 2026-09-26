//! `scribble`: unpack, decode, edit, encode and repack Scribblenauts Unlimited assets.
//!
//! Typical round trip:
//! ```text
//! scribble unpack  game/      extracted/     # .p packs -> loose binary files + manifest.json
//! scribble decode-all extracted/ decoded/    # binary -> readable JSON (+ standard files)
//! ...edit decoded/...
//! scribble encode-all decoded/ extracted/    # JSON -> binary
//! scribble pack    extracted/ out/           # loose files -> .p packs, index.bin, ...
//! ```

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rayon::prelude::*;
use scribble_core::Codec;
use scribble_formats::{handler_for, Handler};
use scribble_pack::{manifest::disk_path, Manifest};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Extract all resources from a game directory's .p packs.
    Unpack { game: PathBuf, out: PathBuf },
    /// Extract all resources of the Wii U build (a decrypted dump or its `content` folder) into
    /// an unpacked tree whose manifest is marked `"platform": "wiiu"`.
    UnpackWiiu {
        wiiu: PathBuf,
        out: PathBuf,
        /// The unpacked PC tree, whose paths name the resources both builds have.
        #[arg(long)]
        pc: PathBuf,
    },
    /// Build .p packs, index.bin, pmindex*.xml and 1s from an unpacked directory.
    Pack { unpacked: PathBuf, out: PathBuf },
    /// Decode one resource to JSON on stdout (or --out).
    Decode {
        file: PathBuf,
        /// Logical path used to pick the codec; defaults to the path relative to --root.
        #[arg(long)]
        logical: Option<String>,
        /// Unpacked directory (for resource names and inferring the logical path).
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Encode one JSON file back to the game's binary format.
    Encode {
        json: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
        /// Logical path of the resource (inferred from the JSON's path under --root if absent).
        #[arg(long)]
        logical: Option<String>,
        /// Codec name (see `scribble formats`); inferred from the path if absent.
        #[arg(long)]
        format: Option<String>,
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Decode every resource of an unpacked directory into a readable tree.
    DecodeAll { unpacked: PathBuf, out: PathBuf },
    /// Encode a decoded tree back into an unpacked directory (manifest is copied along).
    EncodeAll { decoded: PathBuf, out: PathBuf },
    /// Verify decode -> JSON -> encode reproduces every resource byte-for-byte.
    Check {
        unpacked: PathBuf,
        /// Only check resources whose logical path contains this substring.
        #[arg(long)]
        filter: Option<String>,
        /// Print every failure instead of a few per codec.
        #[arg(long)]
        verbose: bool,
    },
    /// List known formats.
    Formats,
    /// List the named id namespaces derived from an unpacked directory (taxonomy levels, tags,
    /// merits), or the ids of one namespace as `id<TAB>printed name<TAB>parent id`.
    Names {
        unpacked: PathBuf,
        /// Namespace to list (e.g. `object.category`); all namespace sizes if absent.
        namespace: Option<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Unpack { game, out } => {
            let m = scribble_pack::unpack(&game, &out, |i, n| progress("unpack", i, n))?;
            eprintln!("\nunpacked {} resources", m.files.len());
        }
        Cmd::UnpackWiiu { wiiu, out, pc } => {
            let pc = scribble_pack::Manifest::load(&pc)?;
            let mut game = scribble_wiiu::WiiuGame::open(&wiiu, &scribble_wiiu::wiiu::pc_paths_from_manifest(&pc))?;
            let m = game.unpack(&out, Some(&pc), |i, n| progress("unpack", i, n))?;
            let only = game.resources.iter().filter(|r| !r.on_pc).count();
            eprintln!("\nunpacked {} resources ({only} not in the PC build)", m.files.len());
        }
        Cmd::Pack { unpacked, out } => {
            scribble_pack::pack(&unpacked, &out, |i, n| progress("pack", i, n))?;
            eprintln!("\npacked into {}", out.display());
        }
        Cmd::Decode { file, logical, root, out } => {
            let data = fs::read(&file)?;
            let logical = logical_path(&file, logical, root.as_deref())?;
            let ctx = context(root.as_deref())?;
            let Some(Handler::Codec(c)) = handler_for(&logical, &data) else { bail!("no codec for {logical}") };
            let text = c.decode_text(&data, &ctx)?;
            match out {
                Some(o) => fs::write(o, text)?,
                None => print!("{text}"),
            }
        }
        Cmd::Encode { json, out, logical, format, root } => {
            let (inferred, suffix_codec) = match &logical {
                Some(l) => (l.clone(), None),
                None => logical_from_decoded(&json, root.as_deref())?,
            };
            let codec = match format.as_deref().or(suffix_codec) {
                Some(name) => scribble_formats::codec_by_name(name).with_context(|| format!("unknown format {name}"))?,
                None => match handler_for(&inferred, &[]) {
                    Some(Handler::Codec(c)) => c,
                    _ => bail!("can't tell the format of {inferred}; pass --format"),
                },
            };
            let ctx = context(root.as_deref())?;
            fs::write(out, codec.encode_text(&fs::read_to_string(&json)?, &ctx)?)?;
        }
        Cmd::DecodeAll { unpacked, out } => decode_all(&unpacked, &out)?,
        Cmd::EncodeAll { decoded, out } => encode_all(&decoded, &out)?,
        Cmd::Check { unpacked, filter, verbose } => check(&unpacked, filter.as_deref(), verbose)?,
        Cmd::Names { unpacked, namespace } => {
            let ctx = scribble_formats::load_context(&unpacked)?;
            match namespace {
                None => {
                    let mut names: Vec<&str> = ctx.namespace_names().collect();
                    names.sort();
                    for n in names {
                        println!("{n:20} {:6} names", ctx.namespace(n).unwrap().len());
                    }
                }
                Some(n) => {
                    let ids = ctx.namespace(&n).with_context(|| format!("no namespace {n:?}"))?;
                    let mut all: Vec<(u32, &str)> = ids.iter().collect();
                    all.sort();
                    for (id, _) in all {
                        let parent = ids.parent(id);
                        let p = parent.map(|p| p.to_string()).unwrap_or_default();
                        println!("{id}\t{}\t{p}", ids.display(id, parent));
                    }
                }
            }
        }
        Cmd::Formats => {
            for c in scribble_formats::codecs() {
                println!("{:8} {}", c.name(), c.description());
            }
        }
    }
    Ok(())
}

fn progress(what: &str, i: usize, n: usize) {
    if i.is_multiple_of(1000) {
        eprint!("\r{what}: {i}/{n}");
    }
}

fn context(root: Option<&Path>) -> Result<scribble_core::Context> {
    match root {
        Some(r) => scribble_formats::load_context(r),
        None => Ok(scribble_core::Context::empty()),
    }
}

fn logical_path(file: &Path, logical: Option<String>, root: Option<&Path>) -> Result<String> {
    if let Some(l) = logical {
        return Ok(l);
    }
    let root = root.context("pass --logical or --root to identify the resource")?;
    let rel = file.strip_prefix(root).with_context(|| format!("{} is not under {}", file.display(), root.display()))?;
    Ok(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("\\"))
}

/// Decoded JSON files are named `<resource file>.json` or `<resource file>.<codec>.json`;
/// returns the logical path and the codec named in the file name, if any.
fn logical_from_decoded(json: &Path, root: Option<&Path>) -> Result<(String, Option<&'static str>)> {
    let l = logical_path(json, None, root)?;
    let l = l.strip_suffix(".json").unwrap_or(&l).to_string();
    for c in scribble_formats::codecs() {
        if let Some(base) = l.strip_suffix(&format!(".{}", c.name()))
            && !matches!(handler_for(&l, &[]), Some(Handler::Codec(_)))
        {
            return Ok((base.to_string(), Some(c.name())));
        }
    }
    Ok((l, None))
}

/// Where a resource lands in the decoded tree: `<file>.json` for game formats (or
/// `<file>.<codec>.json` when the codec can't be told from the path alone), standard formats
/// under their usual extension, anything else unchanged.
fn decoded_path(root: &Path, logical: &str, handler: Option<&Handler>) -> PathBuf {
    let base = disk_path(root, logical);
    let name = base.file_name().unwrap().to_string_lossy().to_string();
    let file = match handler {
        Some(Handler::Codec(c)) => match handler_for(logical, &[]) {
            Some(Handler::Codec(by_path)) if by_path.name() == c.name() => format!("{name}.json"),
            _ => format!("{name}.{}.json", c.name()),
        },
        Some(Handler::Standard { extension, .. }) => {
            if extension.is_empty() || name.to_lowercase().ends_with(&format!(".{extension}")) {
                name
            } else {
                format!("{name}.{extension}")
            }
        }
        None => name,
    };
    base.with_file_name(file)
}

/// The JSON file and codec for a resource in a decoded tree, if it was decoded to JSON.
fn find_decoded_json(root: &Path, logical: &str) -> Option<(PathBuf, &'static dyn Codec)> {
    let base = disk_path(root, logical);
    if let Some(Handler::Codec(c)) = handler_for(logical, &[]) {
        let p = base.with_extension_appended("json");
        if p.exists() {
            return Some((p, c));
        }
    }
    scribble_formats::codecs().into_iter().find_map(|c| {
        let p = base.with_extension_appended(&format!("{}.json", c.name()));
        p.exists().then_some((p, c))
    })
}

fn decode_all(unpacked: &Path, out: &Path) -> Result<()> {
    let m = Manifest::load(unpacked)?;
    let ctx = scribble_formats::load_context(unpacked)?;
    fs::create_dir_all(out)?;
    fs::copy(unpacked.join(scribble_pack::manifest::MANIFEST), out.join(scribble_pack::manifest::MANIFEST))?;
    // The id names are derived from binary files that the decoded tree replaces with JSON.
    scribble_formats::save_names(&ctx, &out.join(scribble_formats::NAMES_FILE))?;
    let errors = Mutex::new(Vec::new());
    m.files.par_iter().filter(|f| f.pack.is_some()).for_each(|f| {
        let r = (|| -> Result<()> {
            let data = fs::read(f.disk_path(unpacked))?;
            let h = handler_for(&f.path, &data);
            let dst = decoded_path(out, &f.path, h.as_ref());
            fs::create_dir_all(dst.parent().unwrap())?;
            match h {
                Some(Handler::Codec(c)) => fs::write(dst, c.decode_text(&data, &ctx)?)?,
                _ => fs::write(dst, &data)?,
            }
            Ok(())
        })();
        if let Err(e) = r {
            errors.lock().unwrap().push(format!("{}: {e:#}", f.path));
        }
    });
    report_errors(errors.into_inner().unwrap())
}

fn encode_all(decoded: &Path, out: &Path) -> Result<()> {
    let m = Manifest::load(decoded)?;
    let ctx = scribble_formats::load_context(decoded)?;
    fs::create_dir_all(out)?;
    fs::copy(decoded.join(scribble_pack::manifest::MANIFEST), out.join(scribble_pack::manifest::MANIFEST))?;
    let errors = Mutex::new(Vec::new());
    m.files.par_iter().filter(|f| f.pack.is_some()).for_each(|f| {
        let r = (|| -> Result<()> {
            let dst = f.disk_path(out);
            fs::create_dir_all(dst.parent().unwrap())?;
            if let Some((json_path, c)) = find_decoded_json(decoded, &f.path) {
                fs::write(dst, c.encode_text(&fs::read_to_string(&json_path)?, &ctx)?).with_context(|| format!("encoding {}", json_path.display()))?;
                return Ok(());
            }
            for candidate in standard_candidates(decoded, &f.path) {
                if candidate.exists() {
                    fs::copy(candidate, dst)?;
                    return Ok(());
                }
            }
            bail!("missing from decoded tree")
        })();
        if let Err(e) = r {
            errors.lock().unwrap().push(format!("{}: {e:#}", f.path));
        }
    });
    report_errors(errors.into_inner().unwrap())
}

fn standard_candidates(root: &Path, logical: &str) -> Vec<PathBuf> {
    let base = disk_path(root, logical);
    let mut v = vec![base.clone()];
    for ext in ["dds", "binka", "wav", "png", "psd", "gp", "dll"] {
        v.push(base.with_extension_appended(ext));
    }
    v
}

trait AppendExt {
    fn with_extension_appended(&self, ext: &str) -> PathBuf;
}
impl AppendExt for PathBuf {
    fn with_extension_appended(&self, ext: &str) -> PathBuf {
        let mut s = self.as_os_str().to_owned();
        s.push(".");
        s.push(ext);
        PathBuf::from(s)
    }
}

fn report_errors(errors: Vec<String>) -> Result<()> {
    if errors.is_empty() {
        eprintln!("done");
        return Ok(());
    }
    for e in errors.iter().take(50) {
        eprintln!("error: {e}");
    }
    bail!("{} resources failed", errors.len())
}

fn check(unpacked: &Path, filter: Option<&str>, verbose: bool) -> Result<()> {
    let m = Manifest::load(unpacked)?;
    let ctx = scribble_formats::load_context(unpacked)?;
    #[derive(Default)]
    struct Tally {
        ok: usize,
        fail: Vec<String>,
    }
    let tallies: Mutex<BTreeMap<String, Tally>> = Mutex::default();
    let unhandled: Mutex<BTreeMap<String, usize>> = Mutex::default();
    m.files.par_iter().filter(|f| f.pack.is_some() && filter.is_none_or(|s| f.path.contains(s))).for_each(|f| {
        let data = match fs::read(f.disk_path(unpacked)) {
            Ok(d) => d,
            Err(e) => {
                unhandled.lock().unwrap().insert(format!("unreadable: {e}"), 1);
                return;
            }
        };
        match handler_for(&f.path, &data) {
            Some(Handler::Codec(c)) => {
                let r = c.roundtrip(&data, &ctx);
                let mut t = tallies.lock().unwrap();
                let t = t.entry(c.name().to_string()).or_default();
                match r {
                    Ok(()) => t.ok += 1,
                    Err(e) => t.fail.push(format!("{}: {e:#}", f.path)),
                }
            }
            Some(Handler::Standard { description, .. }) => *unhandled.lock().unwrap().entry(format!("standard: {description}")).or_default() += 1,
            None => {
                let p = scribble_formats::Path::new(&f.path);
                let key = if p.ext.is_empty() { format!("no codec: (no ext) in {}", p.dir) } else { format!("no codec: .{}", p.ext) };
                *unhandled.lock().unwrap().entry(key).or_default() += 1
            }
        }
    });
    let tallies = tallies.into_inner().unwrap();
    let mut failed = 0;
    for (name, t) in &tallies {
        println!("{name:10} {:6} ok {:6} failed", t.ok, t.fail.len());
        for e in t.fail.iter().take(if verbose { usize::MAX } else { 3 }) {
            println!("    {e}");
        }
        failed += t.fail.len();
    }
    for (k, n) in unhandled.into_inner().unwrap() {
        println!("{n:6}  {k}");
    }
    if failed > 0 {
        bail!("{failed} resources failed to round-trip");
    }
    Ok(())
}
