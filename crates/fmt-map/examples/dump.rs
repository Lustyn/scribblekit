//! Print the decoded JSON of one resource: `cargo run -p fmt-map --example dump -- <logical path>`.
fn main() {
    let path = std::env::args().nth(1).expect("usage: dump <logical path>");
    let root = scribble_core::testing::extracted_root().expect("extracted/ not found");
    let data = std::fs::read(root.join(path.replace('\\', "/"))).unwrap();
    let ctx = scribble_core::testing::context();
    let codec = fmt_map::handler(&scribble_core::ResPath::new(&path), &data).expect("not a map format");
    print!("{}", codec.decode_text(&data, &ctx).unwrap());
}
