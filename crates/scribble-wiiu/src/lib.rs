//! The Wii U build of Scribblenauts Unlimited: reading its packs ([`wiiu`]) and porting its
//! exclusive content — the Nintendo easter eggs — into a PC install ([`port`]).

pub mod emulation;
pub mod port;
pub mod wiiu;

pub use port::{install, installed, uninstall, Options, Summary, BACKUP_DIR, PORT_PACK};
pub use wiiu::{content_dir, PathGuesser, WiiuGame, WiiuResource};
