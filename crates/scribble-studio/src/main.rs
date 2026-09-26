fn main() -> eframe::Result {
    let root = std::env::args_os().nth(1).map(std::path::PathBuf::from).unwrap_or_else(|| "extracted".into());
    scribble_studio::run(root)
}
