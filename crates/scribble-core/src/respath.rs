//! Logical resource paths, split into the parts codec selection matches on.

/// A logical path such as `data\_game\scribbleobjects\cow.so`.
pub struct ResPath<'a> {
    pub full: &'a str,
    /// Final component, lowercased.
    pub file: String,
    /// Extension of the final component, lowercased (`""` if none).
    pub ext: String,
    /// Directory part, lowercased, backslash-separated.
    pub dir: String,
}

impl<'a> ResPath<'a> {
    pub fn new(full: &'a str) -> Self {
        let lower = full.to_lowercase();
        let (dir, file) = lower.rsplit_once('\\').map(|(d, f)| (d.to_string(), f.to_string())).unwrap_or((String::new(), lower.clone()));
        let ext = file.rsplit_once('.').map(|(_, e)| e.to_string()).unwrap_or_default();
        ResPath { full, file, ext, dir }
    }
}

/// Declare a `static` [`FormatCodec`](crate::FormatCodec) for a type and return it as `&dyn Codec`.
#[macro_export]
macro_rules! codec {
    ($t:ty) => {{
        static C: $crate::FormatCodec<$t> = $crate::FormatCodec::new();
        &C as &'static dyn $crate::Codec
    }};
}
