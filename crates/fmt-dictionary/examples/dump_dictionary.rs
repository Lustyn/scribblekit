//! Print one language's high-level dictionary model as JSON:
//! `cargo run -p fmt-dictionary --example dump_dictionary -- english > english.json`
fn main() {
    let lang = std::env::args().nth(1).unwrap_or_else(|| "english".into());
    let root = scribble_core::testing::extracted_root().expect("extracted/ not found");
    let ctx = scribble_core::testing::context();
    let dict = fmt_dictionary::Dictionary::load_dir(&root, &lang, &ctx).unwrap();
    print!("{}", scribble_core::json::to_string(&serde_json::to_value(&dict).unwrap()));
}
