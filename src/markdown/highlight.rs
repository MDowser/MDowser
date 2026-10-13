// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::highlight`: lightweight syntax highlighting for code blocks.
//!
//! - Zero new dependencies, `O(n)` per line, no regex.
//! - Line-based spans that borrow nothing: returns byte ranges so the
//!   viewer can build a single `LayoutJob` per line (one selectable widget).
//! - Supports `rust`, `python`, `javascript`/`typescript`, `bash`, `json`,
//!   `toml`, `yaml`. Anything else falls back to `Plain`.
//! - Heuristics are intentionally small: keywords, strings, comments,
//!   numbers and function calls (`name(`). Lifetimes like `'a` are NOT
//!   treated as strings (require a closing `'` on the same line).

/// Language detected from a fenced info string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    Bash,
    Json,
    Toml,
    Yaml,
    Plain,
}

/// Token kind for one span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Plain,
    Keyword,
    Str,
    Comment,
    Number,
    Func,
}

/// One highlighted span: byte range into the original line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

/// Normalizes a fenced info string (`"Rust"`, `"rs"`, `"sh"`...) to a `Lang`.
///
/// Allocation-free: the first token compares with `eq_ignore_ascii_case`
/// against the aliases instead of building a lowercased `String` (this runs
/// per code block per frame; the language chip already avoids the same
/// allocation).
pub fn norm_lang(info: &str) -> Lang {
    let first = info
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '#'))
        .next()
        .unwrap_or("");
    let eq = |alias: &str| first.eq_ignore_ascii_case(alias);
    if eq("rust") || eq("rs") {
        Lang::Rust
    } else if eq("python") || eq("py") {
        Lang::Python
    } else if eq("javascript") || eq("js") || eq("typescript") || eq("ts") || eq("jsx") || eq("tsx")
    {
        Lang::JavaScript
    } else if eq("bash") || eq("sh") || eq("zsh") || eq("shell") {
        Lang::Bash
    } else if eq("json") {
        Lang::Json
    } else if eq("toml") {
        Lang::Toml
    } else if eq("yaml") || eq("yml") {
        Lang::Yaml
    } else {
        // C-family has no keyword table here: falling back to `Plain`
        // is honest (no false JS/Rust colors on C code).
        Lang::Plain
    }
}

/// The language token of a fenced info string: its first whitespace-delimited
/// word (`"js title=\"app.js\""` -> `"js"`). This is what the code card shows
/// as its chip, so Docusaurus/Mintlify `title=` metadata never leaks into it.
pub fn info_lang(info: &str) -> &str {
    let end = info.find(char::is_whitespace).unwrap_or(info.len());
    &info[..end]
}

/// `title="…"` (also unquoted) from a fenced info string, if present: the tab
/// label a `<CodeGroup>` child shows.
pub fn info_title(info: &str) -> Option<String> {
    for tok in info.split_whitespace() {
        if let Some(v) = tok.strip_prefix("title=") {
            let v = v.trim_matches(|c| c == '"' || c == '\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Returns `true` when the language gets real token colors.
pub fn is_supported(lang: Lang) -> bool {
    !matches!(lang, Lang::Plain)
}

fn keywords(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Rust => &[
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
            "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
            "trait", "true", "type", "unsafe", "use", "where", "while", "Some", "None", "Ok",
            "Err",
        ],
        Lang::Python => &[
            "def", "class", "import", "from", "as", "if", "elif", "else", "for", "while", "return",
            "break", "continue", "pass", "in", "is", "not", "and", "or", "lambda", "with", "try",
            "except", "finally", "raise", "yield", "async", "await", "True", "False", "None",
            "self",
        ],
        Lang::JavaScript => &[
            "function",
            "const",
            "let",
            "var",
            "if",
            "else",
            "for",
            "while",
            "return",
            "break",
            "continue",
            "class",
            "extends",
            "new",
            "this",
            "typeof",
            "instanceof",
            "import",
            "export",
            "from",
            "default",
            "async",
            "await",
            "try",
            "catch",
            "finally",
            "throw",
            "switch",
            "case",
            "true",
            "false",
            "null",
            "undefined",
            "interface",
            "type",
            "enum",
        ],
        Lang::Bash => &[
            "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac",
            "function", "return", "exit", "echo", "in", "true", "false",
        ],
        Lang::Json => &["true", "false", "null"],
        Lang::Toml => &["true", "false"],
        Lang::Yaml => &["true", "false", "null"],
        Lang::Plain => &[],
    }
}

fn is_kw(lang: Lang, word: &str) -> bool {
    keywords(lang).contains(&word)
}

fn push_span(spans: &mut Vec<Span>, kind: TokenKind, start: usize, end: usize) {
    if start >= end {
        return;
    }
    debug_assert!(
        spans.is_empty() || start >= spans.last().map(|s| s.end).unwrap_or(0),
        "spans must be pushed in order"
    );
    if let Some(last) = spans.last_mut() {
        if last.kind == kind && last.end == start {
            last.end = end;
            return;
        }
    }
    spans.push(Span { kind, start, end });
}

/// Byte width of the UTF-8 char starting at `line[i]`. Callers guarantee `i < len`
/// and `i` is a char boundary; falls back to 1 so the scanner always advances.
fn char_len_at(line: &str, i: usize) -> usize {
    line[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1)
}

/// Highlights one line. Returned ranges always cover `0..line.len()`
/// with no gaps when the line is non-empty.
///
/// Safety: every `start`/`end` is a UTF-8 char boundary, so
/// `&line[start..end]` never panics. The scanner advances by whole chars
/// for non-ASCII bytes (never `i += 1` mid-char); ASCII fast paths (`//`,
/// digits, idents) stay byte-wise because ASCII bytes are always boundaries.
pub fn spans(lang: Lang, line: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    if line.is_empty() {
        return out;
    }
    if lang == Lang::Plain {
        push_span(&mut out, TokenKind::Plain, 0, line.len());
        return out;
    }
    let bytes = line.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut plain_start: Option<usize> = None;
    let flush_plain = |out: &mut Vec<Span>, plain_start: &mut Option<usize>, end: usize| {
        if let Some(s) = plain_start.take() {
            push_span(out, TokenKind::Plain, s, end);
        }
    };

    let hash_comment = matches!(lang, Lang::Python | Lang::Bash | Lang::Toml | Lang::Yaml);
    let slash_comment = matches!(lang, Lang::Rust | Lang::JavaScript);

    while i < n {
        let b = bytes[i];
        // `//` line comment (rust/js).
        if slash_comment && b == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            flush_plain(&mut out, &mut plain_start, i);
            push_span(&mut out, TokenKind::Comment, i, n);
            break;
        }
        // `#` line comment (python/bash/toml/yaml).
        if hash_comment && b == b'#' {
            flush_plain(&mut out, &mut plain_start, i);
            push_span(&mut out, TokenKind::Comment, i, n);
            break;
        }
        // Strings: `"..."`, `` `...` ``, and `'...'` (per-language).
        if b == b'"' || b == b'`' || b == b'\'' {
            let quote = bytes[i];
            // `'`: Rust uses char literals (`'a'`) and lifetimes (`'a`);
            // Python/JS/Bash use `'...'` strings of any length.
            // Keep Rust strict (short literal only), others like `"`.
            if quote == b'\'' && lang == Lang::Rust {
                let mut j = i + 1;
                // Optional escape: `'\n'`, `'\\'`.
                if j < n && bytes[j] == b'\\' {
                    j += 1;
                    if j < n {
                        j += 1;
                    }
                } else {
                    // One plain char (ASCII safe: lifetimes/idents are ASCII).
                    if j < n {
                        j += 1;
                    }
                }
                let ok = j < n && bytes[j] == b'\'' && j - i <= 4;
                if ok {
                    flush_plain(&mut out, &mut plain_start, i);
                    push_span(&mut out, TokenKind::Str, i, j + 1);
                    i = j + 1;
                } else {
                    if plain_start.is_none() {
                        plain_start = Some(i);
                    }
                    i += 1;
                }
                continue;
            }
            let mut j = i + 1;
            while j < n {
                if bytes[j] == b'\\' && j + 1 < n {
                    // Skip the escaped char wholly (it may be multi-byte).
                    j += 1;
                    j += char_len_at(line, j);
                    if j > n {
                        j = n;
                    }
                    continue;
                }
                if bytes[j] == quote {
                    j += 1;
                    break;
                }
                // Strings never span lines here (per-line input).
                // Step by whole chars so `j` stays on boundaries.
                if bytes[j] < 0x80 {
                    j += 1;
                } else {
                    j += char_len_at(line, j);
                }
            }
            // Unclosed `"`/backtick: highlight to end of line.
            flush_plain(&mut out, &mut plain_start, i);
            push_span(&mut out, TokenKind::Str, i, j.min(n).max(i + 1));
            i = j.min(n);
            continue;
        }
        // Numbers: digit starts `[0-9][0-9a-zA-Z_.]*`.
        if b.is_ascii_digit() {
            flush_plain(&mut out, &mut plain_start, i);
            let mut j = i + 1;
            while j < n {
                let d = bytes[j];
                if d.is_ascii_alphanumeric() || d == b'_' || d == b'.' {
                    j += 1;
                } else {
                    break;
                }
            }
            push_span(&mut out, TokenKind::Number, i, j);
            i = j;
            continue;
        }
        // Identifiers / keywords / function calls.
        if b.is_ascii_alphabetic() || b == b'_' {
            let mut j = i + 1;
            while j < n {
                let d = bytes[j];
                if d.is_ascii_alphanumeric() || d == b'_' {
                    j += 1;
                } else {
                    break;
                }
            }
            let word = &line[i..j];
            flush_plain(&mut out, &mut plain_start, i);
            if is_kw(lang, word) {
                push_span(&mut out, TokenKind::Keyword, i, j);
            } else {
                // Function call when the next non-space char is `(`.
                // Rust macros allow `name!(...)`: skip one `!`.
                let mut k = j;
                while k < n && (bytes[k] == b' ' || bytes[k] == b'\t') {
                    k += 1;
                }
                if k < n && bytes[k] == b'!' {
                    let mut m = k + 1;
                    while m < n && (bytes[m] == b' ' || bytes[m] == b'\t') {
                        m += 1;
                    }
                    k = m;
                }
                if k < n && bytes[k] == b'(' {
                    push_span(&mut out, TokenKind::Func, i, j);
                } else {
                    push_span(&mut out, TokenKind::Plain, i, j);
                }
            }
            i = j;
            continue;
        }
        // Anything else accumulates as plain (merged later).
        // Step by whole UTF-8 chars: `i += 1` would split multi-byte
        // sequences and leave intermediate non-boundary indices.
        if plain_start.is_none() {
            plain_start = Some(i);
        }
        i += char_len_at(line, i);
    }
    flush_plain(&mut out, &mut plain_start, n);
    debug_assert!(
        out.iter()
            .all(|s| line.is_char_boundary(s.start) && line.is_char_boundary(s.end)),
        "highlighter spans must stay on char boundaries"
    );
    out
}

#[cfg(test)]
mod tests {
    use super::{norm_lang, spans, Lang, TokenKind};

    #[test]
    fn normalizes_aliases() {
        assert_eq!(norm_lang("Rust"), Lang::Rust);
        assert_eq!(norm_lang("rs"), Lang::Rust);
        assert_eq!(norm_lang("py"), Lang::Python);
        assert_eq!(norm_lang("ts"), Lang::JavaScript);
        assert_eq!(norm_lang("sh"), Lang::Bash);
        assert_eq!(norm_lang("yml"), Lang::Yaml);
        assert_eq!(norm_lang("unknown"), Lang::Plain);
        // C-family has no table: honest Plain instead of false colors.
        assert_eq!(norm_lang("c"), Lang::Plain);
        assert_eq!(norm_lang("cpp"), Lang::Plain);
    }

    #[test]
    fn rust_keywords_strings_comments() {
        let line = "fn main() { // hello";
        let sp = spans(Lang::Rust, line);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &line[s.start..s.end] == "fn"));
        assert!(sp.iter().any(|s| s.kind == TokenKind::Comment));
        let line2 = "let s = \"hi\";";
        let sp2 = spans(Lang::Rust, line2);
        assert!(sp2
            .iter()
            .any(|s| s.kind == TokenKind::Str && &line2[s.start..s.end] == "\"hi\""));
    }

    #[test]
    fn lifetime_is_not_a_string() {
        let line = "fn f<'a>(x: &'a str)";
        let sp = spans(Lang::Rust, line);
        assert!(!sp.iter().any(|s| s.kind == TokenKind::Str));
    }

    #[test]
    fn python_hash_comment_and_number() {
        let line = "x = 42 # note";
        let sp = spans(Lang::Python, line);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Number && &line[s.start..s.end] == "42"));
        assert!(sp.iter().any(|s| s.kind == TokenKind::Comment));
    }

    #[test]
    fn function_call_detected() {
        let line = "println!(x)";
        let sp = spans(Lang::Rust, line);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Func && &line[s.start..s.end] == "println"));
    }

    #[test]
    fn ranges_cover_line() {
        let line = "let x = 1;";
        let sp = spans(Lang::Rust, line);
        assert_eq!(sp.first().map(|s| s.start), Some(0));
        assert_eq!(sp.last().map(|s| s.end), Some(line.len()));
    }

    #[test]
    fn unicode_never_panics_or_splits() {
        // Non-ASCII text must survive byte-wise scanning: spans keep
        // char boundaries so `&line[start..end]` never panics.
        let line = "let caf\u{e9} = \"h\u{e9}llo\" // adi\u{f3}s \u{1f600}";
        let sp = spans(Lang::Rust, line);
        assert_eq!(sp.first().map(|s| s.start), Some(0));
        assert_eq!(sp.last().map(|s| s.end), Some(line.len()));
        for s in &sp {
            assert!(line.is_char_boundary(s.start));
            assert!(line.is_char_boundary(s.end));
        }
    }

    #[test]
    fn unicode_strings_and_escapes_stay_on_boundaries() {
        // Multibyte inside strings + escaped multibyte must not split.
        for line in [
            "x = \"caf\u{e9} \u{1f600}\"",
            "s = \"a\\\"\u{e9}b\"",
            "let s = \"h\u{e9}llo \\n w\u{f6}rld\";",
            "caf\u{e9} = 1 # \u{1f600} note",
        ] {
            let sp = spans(Lang::Rust, line);
            assert_eq!(sp.first().map(|s| s.start), Some(0));
            assert_eq!(sp.last().map(|s| s.end), Some(line.len()));
            for s in &sp {
                assert!(
                    line.is_char_boundary(s.start) && line.is_char_boundary(s.end),
                    "split boundary in {line:?}: {s:?}"
                );
                let _ = &line[s.start..s.end];
            }
        }
    }
}
