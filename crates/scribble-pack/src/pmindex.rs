//! `pmindex.xml`: the resource manifest the game's packager emitted. A fixed, line-oriented
//! layout (UTF-8 BOM, CRLF, one `<file .../>` element per line), so it is parsed and written by
//! hand to reproduce the original bytes exactly.

use scribble_core::{bail, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PmFile {
    pub name: String,
    pub index: u32,
    pub guid: String,
    pub chunky: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PmIndex {
    pub files: Vec<PmFile>,
}

const BOM: &str = "\u{feff}";

fn attr<'a>(line: &'a str, key: &str) -> Result<&'a str> {
    let pat = format!(" {key}=\"");
    let Some(start) = line.find(&pat).map(|i| i + pat.len()) else { bail!("missing {key} in {line:?}") };
    let end = start + line[start..].find('"').unwrap_or(0);
    Ok(&line[start..end])
}

impl PmIndex {
    pub fn read(data: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(data)?;
        let text = text.strip_prefix(BOM).unwrap_or(text);
        let mut files = Vec::new();
        for line in text.split("\r\n") {
            let line = line.trim();
            if !line.starts_with("<file ") {
                continue;
            }
            files.push(PmFile {
                name: attr(line, "name")?.to_string(),
                index: attr(line, "index")?.parse()?,
                guid: attr(line, "guid")?.to_string(),
                chunky: match attr(line, "chunky")? {
                    "True" => true,
                    "False" => false,
                    v => bail!("bad chunky value {v:?}"),
                },
            });
        }
        Ok(PmIndex { files })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut s = String::from(BOM);
        s.push_str("<packager>\r\n");
        for f in &self.files {
            s.push_str(&format!(
                "  <file name=\"{}\" index=\"{}\" guid=\"{}\" chunky=\"{}\" />\r\n",
                f.name,
                f.index,
                f.guid,
                if f.chunky { "True" } else { "False" }
            ));
        }
        s.push_str("</packager>");
        s.into_bytes()
    }
}
