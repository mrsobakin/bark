use crate::config::PostStep;

fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\u{2014}' | '\u{2013}' => out.push('-'), // em/en dash
            '\u{2026}' => out.push_str("..."),
            '\u{2018}' | '\u{2019}' | '\u{201E}' => out.push('\''),
            '\u{201C}' | '\u{201D}' | '\u{00AB}' | '\u{00BB}' => out.push('"'),
            '\u{00A0}' | '\u{2009}' | '\u{202F}' => out.push(' '),
            '\u{200B}' | '\u{00AD}' => { /* drop */ }
            _ => out.push(ch),
        }
    }
    out
}

pub fn postprocess(text: &str, steps: &[PostStep]) -> String {
    let mut text = text.to_string();
    for step in steps {
        text = apply(&text, step);
    }
    text
}

fn apply(text: &str, step: &PostStep) -> String {
    match step {
        PostStep::Normalize => normalize_text(text),
        PostStep::Regex { re, with } => re.replace_all(text, with.as_str()).to_string(),
    }
}
