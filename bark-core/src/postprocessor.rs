//! Post-processing: typographic normalisation etc.

use crate::config::{DeEmdasherConfig, PostConfig};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Symbol translation table (generated once)
// ---------------------------------------------------------------------------

static SYMBOLS: LazyLock<HashMap<char, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // Dashes
    m.insert('\u{2014}', "-");   // —  em dash
    m.insert('\u{2013}', "-");   // –  en dash
    // Ellipsis
    m.insert('\u{2026}', "...");  // …  horizontal ellipsis
    // Quotes
    m.insert('\u{2018}', "'");   // '  left single
    m.insert('\u{2019}', "'");   // '  right single
    m.insert('\u{201C}', "\"");  // "  left double
    m.insert('\u{201D}', "\"");  // "  right double
    m.insert('\u{201E}', "\"");  // „  double low-9
    m.insert('\u{00AB}', "\"");  // «  left guillemet
    m.insert('\u{00BB}', "\"");  // »  right guillemet
    // Spaces
    m.insert('\u{00A0}', " ");   //     non-breaking space
    m.insert('\u{2009}', " ");   //    thin space
    m.insert('\u{202F}', " ");   //     narrow no-break space
    // Zero-width / soft hyphen
    m.insert('\u{200B}', "");    // ​  zero-width space
    m.insert('\u{00AD}', "");    // ­  soft hyphen
    m
});

static DOUBLEDASH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\s)-(\s)").unwrap()
});

fn translate_symbols(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if let Some(replacement) = SYMBOLS.get(&ch) {
            out.push_str(replacement);
        } else {
            out.push(ch);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// DeEmdasher
// ---------------------------------------------------------------------------

/// Unicode → ASCII normalisation + optional double-dash conversion.
///
/// Ported from the Python `speechd` DeEmdasher.
pub struct DeEmdasher {
    doubledash: bool,
}

impl DeEmdasher {
    pub fn new(config: &DeEmdasherConfig) -> Self {
        Self {
            doubledash: config.doubledash,
        }
    }

    #[allow(dead_code)]
    pub fn default() -> Self {
        Self { doubledash: false }
    }

    pub fn process(&self, text: &str) -> String {
        let mut out = translate_symbols(text);
        if self.doubledash {
            out = DOUBLEDASH_RE.replace_all(&out, "$1--$2").into_owned();
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Full post-processing pipeline
// ---------------------------------------------------------------------------

/// Apply all enabled post-processing steps and return the final text.
pub fn postprocess(text: &str, config: &PostConfig) -> String {
    let mut text = text.to_string();

    if let Some(ref cfg) = config.deemdasher {
        text = DeEmdasher::new(cfg).process(&text);
    }

    text.trim().replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deemdasher_basic() {
        let cfg = DeEmdasherConfig { doubledash: false };
        let d = DeEmdasher::new(&cfg);
        // Em dash → ASCII hyphen
        assert_eq!(d.process("hello\u{2014}world"), "hello-world");
        // Ellipsis
        assert_eq!(d.process("wait\u{2026}"), "wait...");
    }

    #[test]
    fn deemdasher_doubledash() {
        let cfg = DeEmdasherConfig { doubledash: true };
        let d = DeEmdasher::new(&cfg);
        assert_eq!(d.process("a - b"), "a -- b");
    }
}