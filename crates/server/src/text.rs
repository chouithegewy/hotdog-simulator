// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 David Nunes-Childs
//! What the lobby takes from people as text: names and chat lines. Nothing here is markup: the page shows what
//! arrives as text, never as HTML (lobby.js), so nothing is escaped, only cleaned.
//!
//! A name is 1 to NAME visible characters (grapheme clusters, so an emoji made of several code points is one),
//! with control and invisible characters removed and white space collapsed; a chat line the same, at most CHAT.
//! These are the rules the Node lobby had, in the same order: cut to 400 (4000 for chat) UTF-16 units, normalise
//! to NFC, collapse white space, drop what cannot be seen, collapse spaces, trim, cut to the visible length, trim.
use regex::Regex;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// A name is at most NAME visible characters; a chat line at most CHAT.
pub const NAME: usize = 16;
pub const CHAT: usize = 200;

/// JavaScript's white space (`\s`, and what `trim` removes): its WhiteSpace and LineTerminator characters.
pub fn js_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}
fn js_trim(s: &str) -> &str {
    s.trim_matches(js_space)
}
/// Control characters, format characters (the zero-width joiner aside, which emoji need), line and paragraph
/// separators and private-use characters. (Lone surrogates, which JavaScript strings can hold, cannot be in a Rust
/// string at all.)
static UNPRINTABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\p{Cc}\p{Co}\u{2028}\u{2029}]|[\p{Cf}--\u{200d}]").expect("a valid pattern"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" +").expect("a valid pattern"));

/// The first `units` UTF-16 code units of `s`, never splitting a character.
fn utf16_prefix(s: &str, units: usize) -> &str {
    let mut count = 0;
    for (at, c) in s.char_indices() {
        count += c.len_utf16();
        if count > units {
            return &s[..at];
        }
    }
    s
}
/// Cleans text as the lobby keeps it, cut to `visible` grapheme clusters; '' when nothing is left.
fn clean(value: &str, cut: usize, visible: usize) -> String {
    let normal: String = utf16_prefix(value, cut).nfc().collect();
    // White space collapsed (runs of it become one space), what cannot be seen removed, spaces collapsed again.
    let mut spaced = String::with_capacity(normal.len());
    let mut in_space = false;
    for c in normal.chars() {
        if js_space(c) {
            if !in_space {
                spaced.push(' ');
            }
            in_space = true;
        } else {
            spaced.push(c);
            in_space = false;
        }
    }
    let shown = UNPRINTABLE.replace_all(&spaced, "");
    let single = SPACES.replace_all(&shown, " ");
    let kept: String = js_trim(&single).graphemes(true).take(visible).collect();
    js_trim(&kept).to_string()
}
/// A name as the lobby keeps it: no control or invisible characters, white space collapsed and trimmed, at most
/// NAME visible characters. '' when nothing is left (or `value` is not a string).
pub fn clean_name(value: Option<&serde_json::Value>) -> String {
    match value.and_then(|v| v.as_str()) {
        Some(text) => clean(text, 400, NAME),
        None => String::new(),
    }
}
/// A chat line as it is passed on: the same cleaning, at most CHAT visible characters. '' when nothing is left.
pub fn clean_chat(value: Option<&serde_json::Value>) -> String {
    match value.and_then(|v| v.as_str()) {
        Some(text) => clean(text, 4000, CHAT),
        None => String::new(),
    }
}
/// `name`, or `name 2`, `name 3`…: the first that none of `taken` is, whatever the capitals. The suffix fits inside
/// NAME visible characters.
pub fn unique_name<'a>(name: &str, taken: impl IntoIterator<Item = &'a str>) -> String {
    let used: std::collections::HashSet<String> = taken.into_iter().map(str::to_lowercase).collect();
    if !used.contains(&name.to_lowercase()) {
        return name.to_string();
    }
    for n in 2.. {
        let suffix = format!(" {n}");
        let room = NAME.saturating_sub(suffix.chars().count());
        let base: String = name.graphemes(true).take(room).collect();
        let candidate = format!("{}{suffix}", js_trim(&base));
        if !used.contains(&candidate.to_lowercase()) {
            return candidate;
        }
    }
    unreachable!("there is always a free number")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn name(text: &str) -> String {
        clean_name(Some(&json!(text)))
    }
    fn chat(text: &str) -> String {
        clean_chat(Some(&json!(text)))
    }
    #[test]
    fn a_name_is_trimmed_1_to_16_visible_characters_and_free_of_control_and_invisible_characters() {
        assert_eq!(NAME, 16);
        assert_eq!(name("  Frank   Further  "), "Frank Further");
        assert_eq!(name("a\u{0}b\u{7}c\u{1b}[31m"), "abc[31m");
        assert_eq!(name("tab\tand\nline"), "tab and line");
        assert_eq!(name("\u{202e}evil\u{200b}\u{2028}"), "evil", "bidi overrides, zero-width spaces and line separators go");
        assert_eq!(name(&"x".repeat(40)), "x".repeat(16));
        assert_eq!(name("1234567890123456 tail"), "1234567890123456");
        assert_eq!(name(&"👩‍🍳".repeat(30)).graphemes(true).count(), 16, "sixteen visible characters, however many code points");
        for nothing in [json!(""), json!("   "), json!("\u{0}\u{1}"), json!(null), json!(7), json!({}), json!(["a"])] {
            assert_eq!(clean_name(Some(&nothing)), "");
        }
        assert_eq!(clean_name(None), "");
        assert_eq!(name("<b>Bun</b>"), "<b>Bun</b>", "markup is only text: the page never treats it as HTML");
        assert_eq!(name("e\u{301}clair"), "\u{e9}clair", "normalised to NFC");
    }
    #[test]
    fn a_taken_name_is_suffixed_whatever_its_capitals_and_the_suffix_fits_in_16_characters() {
        assert_eq!(unique_name("Bob", []), "Bob");
        assert_eq!(unique_name("Bob", ["bob"]), "Bob 2");
        assert_eq!(unique_name("BOB", ["Bob", "bob 2"]), "BOB 3");
        assert_eq!(unique_name("ABCDEFGHIJKLMNOP", ["abcdefghijklmnop"]), "ABCDEFGHIJKLMN 2");
    }
    #[test]
    fn chat_is_capped_at_200_characters_and_stripped_of_control_characters() {
        assert_eq!(CHAT, 200);
        assert_eq!(chat(&"x".repeat(500)).len(), 200);
        assert_eq!(chat("  hello\u{0}\u{7} \n\t there\u{202e}  "), "hello there");
        for nothing in [json!(""), json!("  \n "), json!("\u{1}"), json!(null), json!(42), json!({})] {
            assert_eq!(clean_chat(Some(&nothing)), "");
        }
    }
}
