//! Installs the Wii U-exclusive content of Scribblenauts Unlimited into the PC (Steam) release,
//! built from the user's own Wii U game files. Run with no arguments for prompts, or:
//!
//! ```text
//! wiiu-port-installer --game <PC game folder> --wiiu <Wii U dump folder> [--no-emulation]
//! wiiu-port-installer --game <PC game folder> --uninstall
//! ```
//!
//! `--no-emulation` leaves out the data stand-ins for the Wii U-only item physics (the bouncing
//! super star and fireballs; see `scribble_wiiu::emulation`).
//!
//! ```text
//! ```

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

const STEAM_DIRS: [&str; 2] = [
    "C:\\Program Files (x86)\\Steam\\steamapps\\common\\Scribblenauts Unlimited",
    "C:\\Program Files\\Steam\\steamapps\\common\\Scribblenauts Unlimited",
];

fn main() {
    let interactive = std::env::args().len() == 1 && std::io::stdin().is_terminal();
    let code = match run(interactive) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("\nError: {e:#}");
            1
        }
    };
    if interactive {
        prompt("\nPress Enter to close.");
    }
    std::process::exit(code);
}

fn run(interactive: bool) -> anyhow::Result<()> {
    println!("Scribblenauts Unlimited: Wii U content port\n");
    let mut game: Option<PathBuf> = None;
    let mut wiiu: Option<PathBuf> = None;
    let mut uninstall = false;
    let mut options = scribble_wiiu::Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--game" => game = args.next().map(PathBuf::from),
            "--wiiu" => wiiu = args.next().map(PathBuf::from),
            "--uninstall" => uninstall = true,
            "--no-emulation" => options.emulate_engine = false,
            "-h" | "--help" => {
                println!("usage: wiiu-port-installer --game <PC game folder> --wiiu <Wii U dump folder> [--no-emulation]\n       wiiu-port-installer --game <PC game folder> --uninstall");
                return Ok(());
            }
            _ => anyhow::bail!("unknown argument {a:?} (see --help)"),
        }
    }

    let game = match game {
        Some(g) => g,
        None if interactive => {
            let found = STEAM_DIRS.iter().map(Path::new).find(|d| d.join("Scribble.exe").is_file());
            let hint = found.map(|d| format!(" [{}]", d.display())).unwrap_or_default();
            let answer = prompt(&format!("PC game folder (the one with Scribble.exe){hint}: "));
            if answer.is_empty() { found.map(Path::to_path_buf).ok_or_else(|| anyhow::anyhow!("no game folder given"))? } else { PathBuf::from(answer) }
        }
        None => anyhow::bail!("--game is required (see --help)"),
    };

    if interactive && scribble_wiiu::installed(&game)? {
        uninstall = prompt("The Wii U content is installed. Type U to uninstall it, or Enter to reinstall: ").eq_ignore_ascii_case("u");
    }
    if uninstall {
        if scribble_wiiu::uninstall(&game)? {
            println!("Uninstalled: the original index files are restored and {} is deleted.", scribble_wiiu::PORT_PACK);
        } else {
            println!("The Wii U content is not installed in {}.", game.display());
        }
        return Ok(());
    }

    let wiiu = match wiiu {
        Some(w) => w,
        None if interactive => PathBuf::from(prompt(
            "Wii U game folder: your decrypted dump of Scribblenauts Unlimited (the folder with code, content and meta): ",
        )),
        None => anyhow::bail!("--wiiu is required (see --help)"),
    };

    let s = scribble_wiiu::install(&game, &wiiu, &options, &mut |m| println!("{m}"))?;
    println!("\nInstalled into {}:", game.display());
    println!("  new resources: {}", s.added.iter().map(|(k, v)| format!("{v} .{k}")).collect::<Vec<_>>().join(", "));
    println!("  updated: {} shared resources, {} object details, {} sound settings", s.replaced.len(), s.object_details, s.sounds);
    if !s.emulated.is_empty() {
        println!("  bounce physics emulated for: {}", s.emulated.iter().map(|p| p.rsplit('\\').next().unwrap_or(p)).collect::<Vec<_>>().join(", "));
    }
    println!("  words: {}", s.words.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
    println!("  {} ({:.1} MB); originals saved in {}", scribble_wiiu::PORT_PACK, s.pack_bytes as f64 / 1e6, scribble_wiiu::BACKUP_DIR);
    println!("\nTry writing MARIO, LINK or YOSHI in game. Steam's \"verify integrity\" undoes the install; run this again after.");
    Ok(())
}

fn prompt(text: &str) -> String {
    print!("{text}");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok();
    line.trim().trim_matches('"').to_string()
}
