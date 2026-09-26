//! Readable JSON output: like `serde_json::to_string_pretty`, but short arrays/objects stay on
//! one line and long runs of numbers are packed into lines, so geometry and tables stay compact.
//!
//! Layout works on JSON *text* (numbers are kept exactly as serde wrote them), so values
//! serialized straight from Rust types keep `f32`s in their shortest form (`0.1`, not
//! `0.10000000149011612` as a detour through `serde_json::Value` would produce).

use serde::Serialize;
use serde_json::Value;

const WIDTH: usize = 100;

/// Serialize a Rust value to readable JSON.
pub fn to_string_of<T: Serialize + ?Sized>(v: &T) -> crate::Result<String> {
    Ok(format(&serde_json::to_string(v)?))
}

/// Readable JSON for a `serde_json::Value`.
pub fn to_string(v: &Value) -> String {
    format(&serde_json::to_string(v).unwrap())
}

/// Re-layout compact JSON text. Panics if `text` is not valid JSON.
pub fn format(text: &str) -> String {
    let node = Parser { s: text.as_bytes(), i: 0 }.value();
    let mut out = String::new();
    write(&mut out, &node, 0);
    out.push('\n');
    out
}

enum Node {
    /// A number, string, bool or null, as its exact source text.
    Scalar(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn string(&mut self) -> String {
        let start = self.i;
        self.i += 1;
        while self.s[self.i] != b'"' {
            if self.s[self.i] == b'\\' {
                self.i += 1;
            }
            self.i += 1;
        }
        self.i += 1;
        String::from_utf8(self.s[start..self.i].to_vec()).unwrap()
    }
    fn value(&mut self) -> Node {
        self.ws();
        match self.s[self.i] {
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.s[self.i] == b']' {
                        self.i += 1;
                        return Node::Array(items);
                    }
                    items.push(self.value());
                    self.ws();
                    if self.s[self.i] == b',' {
                        self.i += 1;
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.s[self.i] == b'}' {
                        self.i += 1;
                        return Node::Object(items);
                    }
                    let k = self.string();
                    self.ws();
                    self.i += 1; // ':'
                    let v = self.value();
                    items.push((k, v));
                    self.ws();
                    if self.s[self.i] == b',' {
                        self.i += 1;
                    }
                }
            }
            b'"' => Node::Scalar(self.string()),
            _ => {
                let start = self.i;
                while self.i < self.s.len() && !matches!(self.s[self.i], b',' | b']' | b'}') && !self.s[self.i].is_ascii_whitespace() {
                    self.i += 1;
                }
                Node::Scalar(String::from_utf8(self.s[start..self.i].to_vec()).unwrap())
            }
        }
    }
}

/// Single-line rendering with a space after `:` and `,` (`{"a": 1, "b": [1, 2]}`).
fn compact(n: &Node) -> String {
    match n {
        Node::Scalar(s) => s.clone(),
        Node::Array(items) => format!("[{}]", items.iter().map(compact).collect::<Vec<_>>().join(", ")),
        Node::Object(items) => format!("{{{}}}", items.iter().map(|(k, v)| format!("{k}: {}", compact(v))).collect::<Vec<_>>().join(", ")),
    }
}

fn is_scalar(n: &Node) -> bool {
    matches!(n, Node::Scalar(_))
}

fn write(out: &mut String, n: &Node, indent: usize) {
    let c = compact(n);
    if is_scalar(n) || indent * 2 + c.len() <= WIDTH {
        out.push_str(&c);
        return;
    }
    let pad = "  ".repeat(indent + 1);
    match n {
        Node::Array(items) if items.iter().all(is_scalar) => {
            // Pack scalars into lines of at most WIDTH characters.
            out.push_str("[\n");
            let mut line = String::new();
            for (i, item) in items.iter().enumerate() {
                let s = compact(item) + if i + 1 < items.len() { "," } else { "" };
                if !line.is_empty() && pad.len() + line.len() + 1 + s.len() > WIDTH {
                    out.push_str(&pad);
                    out.push_str(&line);
                    out.push('\n');
                    line.clear();
                }
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&s);
            }
            out.push_str(&pad);
            out.push_str(&line);
            out.push('\n');
            out.push_str(&"  ".repeat(indent));
            out.push(']');
        }
        Node::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&pad);
                write(out, item, indent + 1);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(indent));
            out.push(']');
        }
        Node::Object(items) => {
            out.push_str("{\n");
            for (i, (k, v)) in items.iter().enumerate() {
                out.push_str(&pad);
                out.push_str(k);
                out.push_str(": ");
                write(out, v, indent + 1);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(indent));
            out.push('}');
        }
        Node::Scalar(_) => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stays_valid_json() {
        let v: Value = serde_json::json!({"a": [1, 2, 3], "b": {"c": (0..200).collect::<Vec<_>>()}, "d": [{"x": 1}, {"y": [1.5, -2]}], "e": "q\"\\"});
        let s = to_string(&v);
        assert_eq!(serde_json::from_str::<Value>(&s).unwrap(), v);
    }
    #[test]
    fn f32_stays_short() {
        #[derive(Serialize)]
        struct S {
            x: f32,
        }
        assert_eq!(to_string_of(&S { x: 0.1 }).unwrap(), "{\"x\": 0.1}\n");
    }
}
