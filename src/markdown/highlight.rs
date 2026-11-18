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
//!   `toml`, `yaml`, C/C++, Go, Java, C# and SQL. Anything else falls back
//!   to `Plain`.
//! - Block comments (`/* ... */`) are tracked across lines through
//!   [`State`]: [`tokenize`] scans a whole block so a comment opened on one
//!   line colors the following lines until its `*/` (or the end of the
//!   block). The scanner itself stays line-based, so strings and line
//!   comments still end with their line.
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
    C,
    Go,
    Java,
    CSharp,
    Sql,
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

/// Scanner state carried from one line to the next.
///
/// Only `/* ... */` block comments outlive a line; strings and `//`/`#`/`--`
/// comments end with it. [`tokenize`] carries this state across a whole
/// fenced block, while [`spans`] starts from a fresh one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct State {
    /// A `/*` opened on an earlier line has not seen its `*/` yet.
    pub in_block_comment: bool,
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
    } else if eq("c")
        || eq("h")
        || eq("cc")
        || eq("cpp")
        || eq("cxx")
        || eq("c++")
        || eq("hpp")
        || eq("hxx")
    {
        // C and C++ share one keyword/comment scanner; the common subset is
        // small enough that a separate table would only add maintenance.
        Lang::C
    } else if eq("go") || eq("golang") {
        Lang::Go
    } else if eq("java") {
        Lang::Java
    } else if eq("cs") || eq("csharp") || eq("c#") || eq("dotnet") {
        Lang::CSharp
    } else if eq("sql")
        || eq("postgres")
        || eq("postgresql")
        || eq("mysql")
        || eq("sqlite")
        || eq("plsql")
        || eq("tsql")
    {
        Lang::Sql
    } else {
        Lang::Plain
    }
}

/// The language token of a fenced info string: its first whitespace-delimited
/// word (`"js title=\"app.js\""` -> `"js"`). This is what the code card shows
/// as its chip, so docs-generator `title=` metadata never leaks into it.
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
        Lang::C => &[
            "alignas",
            "alignof",
            "asm",
            "auto",
            "bool",
            "break",
            "case",
            "catch",
            "char",
            "char16_t",
            "char32_t",
            "char8_t",
            "class",
            "concept",
            "const",
            "const_cast",
            "consteval",
            "constexpr",
            "constinit",
            "continue",
            "co_await",
            "co_return",
            "co_yield",
            "decltype",
            "default",
            "delete",
            "do",
            "double",
            "dynamic_cast",
            "else",
            "enum",
            "explicit",
            "export",
            "extern",
            "false",
            "final",
            "float",
            "for",
            "friend",
            "goto",
            "if",
            "import",
            "inline",
            "int",
            "long",
            "module",
            "mutable",
            "namespace",
            "new",
            "noexcept",
            "nullptr",
            "operator",
            "override",
            "private",
            "protected",
            "public",
            "register",
            "reinterpret_cast",
            "requires",
            "return",
            "short",
            "signed",
            "sizeof",
            "static",
            "static_assert",
            "static_cast",
            "struct",
            "switch",
            "template",
            "this",
            "thread_local",
            "throw",
            "true",
            "try",
            "typedef",
            "typeid",
            "typename",
            "union",
            "unsigned",
            "using",
            "virtual",
            "void",
            "volatile",
            "wchar_t",
            "while",
        ],
        Lang::Go => &[
            "break",
            "case",
            "chan",
            "const",
            "continue",
            "default",
            "defer",
            "else",
            "fallthrough",
            "false",
            "for",
            "func",
            "go",
            "goto",
            "if",
            "import",
            "interface",
            "iota",
            "map",
            "nil",
            "package",
            "range",
            "return",
            "select",
            "struct",
            "switch",
            "true",
            "type",
            "var",
        ],
        Lang::Java => &[
            "abstract",
            "assert",
            "boolean",
            "break",
            "byte",
            "case",
            "catch",
            "char",
            "class",
            "const",
            "continue",
            "default",
            "do",
            "double",
            "else",
            "enum",
            "extends",
            "false",
            "final",
            "finally",
            "float",
            "for",
            "goto",
            "if",
            "implements",
            "import",
            "instanceof",
            "int",
            "interface",
            "long",
            "native",
            "new",
            "null",
            "package",
            "private",
            "protected",
            "public",
            "record",
            "return",
            "sealed",
            "short",
            "static",
            "strictfp",
            "super",
            "switch",
            "synchronized",
            "this",
            "throw",
            "throws",
            "transient",
            "true",
            "try",
            "var",
            "void",
            "volatile",
            "while",
            "yield",
        ],
        Lang::CSharp => &[
            "abstract",
            "as",
            "async",
            "await",
            "base",
            "bool",
            "break",
            "byte",
            "case",
            "catch",
            "char",
            "checked",
            "class",
            "const",
            "continue",
            "decimal",
            "default",
            "delegate",
            "do",
            "double",
            "else",
            "enum",
            "event",
            "explicit",
            "extern",
            "false",
            "finally",
            "fixed",
            "float",
            "for",
            "foreach",
            "get",
            "goto",
            "if",
            "implicit",
            "in",
            "init",
            "int",
            "interface",
            "internal",
            "is",
            "lock",
            "long",
            "namespace",
            "new",
            "null",
            "object",
            "operator",
            "out",
            "override",
            "params",
            "private",
            "protected",
            "public",
            "readonly",
            "record",
            "ref",
            "return",
            "sbyte",
            "sealed",
            "set",
            "short",
            "sizeof",
            "stackalloc",
            "static",
            "string",
            "struct",
            "switch",
            "this",
            "throw",
            "true",
            "try",
            "typeof",
            "uint",
            "ulong",
            "unchecked",
            "unsafe",
            "ushort",
            "using",
            "var",
            "virtual",
            "void",
            "volatile",
            "while",
        ],
        Lang::Sql => &[
            "add",
            "all",
            "alter",
            "and",
            "as",
            "asc",
            "begin",
            "between",
            "by",
            "case",
            "column",
            "commit",
            "create",
            "delete",
            "desc",
            "distinct",
            "drop",
            "else",
            "end",
            "exists",
            "foreign",
            "from",
            "full",
            "group",
            "having",
            "in",
            "index",
            "inner",
            "insert",
            "into",
            "is",
            "join",
            "key",
            "left",
            "like",
            "limit",
            "not",
            "null",
            "offset",
            "on",
            "or",
            "order",
            "outer",
            "primary",
            "references",
            "recursive",
            "returning",
            "right",
            "rollback",
            "select",
            "set",
            "table",
            "then",
            "transaction",
            "union",
            "update",
            "values",
            "view",
            "when",
            "where",
            "with",
        ],
        Lang::Plain => &[],
    }
}

fn is_kw(lang: Lang, word: &str) -> bool {
    match lang {
        // SQL is conventionally written in either case; every other language
        // here is case-sensitive, so `Class` must stay plain in JavaScript.
        Lang::Sql => keywords(lang)
            .iter()
            .any(|keyword| word.eq_ignore_ascii_case(keyword)),
        _ => keywords(lang).contains(&word),
    }
}

fn push_span(spans: &mut Vec<Span>, kind: TokenKind, start: usize, end: usize) {
    if start >= end {
        return;
    }
    debug_assert!(
        spans.is_empty() || start >= spans.last().map_or(0, |s| s.end),
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
    line[i..].chars().next().map_or(1, |c| c.len_utf8())
}

/// Lexical rules that vary by language. Keeping them in one place means the
/// scanner has no long `matches!` chains per token and a new language only
/// has to fill this table plus its keyword list.
#[derive(Clone, Copy)]
struct Syntax {
    /// `#` starts a line comment.
    hash_comment: bool,
    /// `//` starts a line comment.
    slash_comment: bool,
    /// `/* ... */` marks a block comment (state carried across lines).
    block_comment: bool,
    /// `--` starts a line comment.
    sql_line_comment: bool,
    /// A single quote names a short char literal, not a string.
    char_literal: bool,
}

impl Syntax {
    fn for_lang(lang: Lang) -> Self {
        Self {
            hash_comment: matches!(lang, Lang::Python | Lang::Bash | Lang::Toml | Lang::Yaml),
            slash_comment: matches!(
                lang,
                Lang::Rust | Lang::JavaScript | Lang::C | Lang::Go | Lang::Java | Lang::CSharp
            ),
            block_comment: matches!(
                lang,
                Lang::Rust
                    | Lang::JavaScript
                    | Lang::C
                    | Lang::Go
                    | Lang::Java
                    | Lang::CSharp
                    | Lang::Sql
            ),
            sql_line_comment: matches!(lang, Lang::Sql),
            char_literal: matches!(
                lang,
                Lang::Rust | Lang::C | Lang::Go | Lang::Java | Lang::CSharp
            ),
        }
    }
}

/// Highlights one line from a fresh scanner state. Returned ranges always
/// cover `0..line.len()` with no gaps when the line is non-empty.
///
/// Use [`spans_with_state`] to continue an open block comment, or
/// [`tokenize`] for a whole fenced block (the normal caller).
pub fn spans(lang: Lang, line: &str) -> Vec<Span> {
    spans_with_state(lang, line, State::default()).0
}

/// Tokenizes a whole block, carrying `/* ... */` comment state from line to
/// line. The result has one span list per line, in `body.split('\n')` order.
pub fn tokenize(lang: Lang, body: &str) -> Vec<Vec<Span>> {
    // Plain blocks have no tokens and therefore no state to carry.
    if lang == Lang::Plain {
        return body.split('\n').map(|line| spans(lang, line)).collect();
    }
    let mut state = State::default();
    body.split('\n')
        .map(|line| {
            let (spans, next) = spans_with_state(lang, line, state);
            state = next;
            spans
        })
        .collect()
}

/// Highlights one line, resuming from (and reporting) the scanner state at
/// the previous line's end.
///
/// Safety: every `start`/`end` is a UTF-8 char boundary, so
/// `&line[start..end]` never panics. The scanner advances by whole chars
/// for non-ASCII bytes (never `i += 1` mid-char); ASCII fast paths (`//`,
/// digits, idents) stay byte-wise because ASCII bytes are always boundaries.
pub fn spans_with_state(lang: Lang, line: &str, state: State) -> (Vec<Span>, State) {
    let mut state = state;
    let syntax = Syntax::for_lang(lang);
    // Only the block-comment languages can resume: clear a stale state when
    // a caller hands one to a language that has no `/* ... */` syntax.
    if !syntax.block_comment {
        state.in_block_comment = false;
    }
    let mut out: Vec<Span> = Vec::new();
    if line.is_empty() {
        return (out, state);
    }
    if lang == Lang::Plain {
        push_span(&mut out, TokenKind::Plain, 0, line.len());
        return (out, state);
    }
    let bytes = line.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    // Continue a block comment opened on an earlier line: everything up to
    // its closing `*/` is comment, and scanning resumes after it.
    if state.in_block_comment {
        match line.find("*/") {
            Some(at) => {
                let end = at + 2;
                push_span(&mut out, TokenKind::Comment, 0, end);
                i = end;
                state.in_block_comment = false;
            }
            None => {
                push_span(&mut out, TokenKind::Comment, 0, n);
                return (out, state);
            }
        }
    }
    let mut plain_start: Option<usize> = None;
    let flush_plain = |out: &mut Vec<Span>, plain_start: &mut Option<usize>, end: usize| {
        if let Some(s) = plain_start.take() {
            push_span(out, TokenKind::Plain, s, end);
        }
    };

    while i < n {
        let b = bytes[i];
        // `/* ... */` block comment (carried across lines; see `State`).
        if syntax.block_comment && b == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            flush_plain(&mut out, &mut plain_start, i);
            match line[i + 2..].find("*/") {
                Some(rel) => {
                    let end = i + 2 + rel + 2;
                    push_span(&mut out, TokenKind::Comment, i, end);
                    i = end;
                }
                None => {
                    push_span(&mut out, TokenKind::Comment, i, n);
                    state.in_block_comment = true;
                    i = n;
                }
            }
            continue;
        }
        // `//` line comment (Rust/JS/C/C++/Go/Java/C#).
        if syntax.slash_comment && b == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            flush_plain(&mut out, &mut plain_start, i);
            push_span(&mut out, TokenKind::Comment, i, n);
            break;
        }
        // `#` line comment (python/bash/toml/yaml).
        if syntax.hash_comment && b == b'#' {
            flush_plain(&mut out, &mut plain_start, i);
            push_span(&mut out, TokenKind::Comment, i, n);
            break;
        }
        // `--` line comment (SQL).
        if syntax.sql_line_comment && b == b'-' && i + 1 < n && bytes[i + 1] == b'-' {
            flush_plain(&mut out, &mut plain_start, i);
            push_span(&mut out, TokenKind::Comment, i, n);
            break;
        }
        // Strings: `"..."`, `` `...` ``, and `'...'` (per-language).
        if b == b'"' || b == b'`' || b == b'\'' {
            let quote = bytes[i];
            // `'`: Rust, C, Go, Java and C# use char literals (`'a'`), while
            // Python/JS/Bash/SQL use `'...'` strings of any length.
            // Keep the char-literal languages strict (short literal only),
            // others like `"`.
            if quote == b'\'' && syntax.char_literal {
                let mut j = i + 1;
                // Optional escape: `'\n'`, `'\\'`.
                if j < n && bytes[j] == b'\\' {
                    j += 1;
                    if j < n {
                        j += 1;
                    }
                } else {
                    // One plain char, which may be multi-byte (`'é'`,
                    // `'😀'`). Advance to the next CHAR boundary, not to the
                    // next byte: landing on a UTF-8 continuation byte made
                    // the closing-quote check fail, so valid non-ASCII char
                    // literals were never colored.
                    if j < n {
                        j += char_len_at(line, j);
                    }
                }
                // `j - i` spans the opening quote + the char (up to 4 bytes)
                // + the closing quote, so 6 still excludes long strings.
                let ok = j < n && bytes[j] == b'\'' && j - i <= 6;
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
    (out, state)
}

#[cfg(test)]
mod tests {
    use super::{norm_lang, spans, spans_with_state, tokenize, Lang, State, TokenKind};

    #[test]
    fn normalizes_aliases() {
        assert_eq!(norm_lang("Rust"), Lang::Rust);
        assert_eq!(norm_lang("rs"), Lang::Rust);
        assert_eq!(norm_lang("py"), Lang::Python);
        assert_eq!(norm_lang("ts"), Lang::JavaScript);
        assert_eq!(norm_lang("sh"), Lang::Bash);
        assert_eq!(norm_lang("yml"), Lang::Yaml);
        assert_eq!(norm_lang("unknown"), Lang::Plain);
        // C/C++, Go, Java, C# and SQL have their own keyword tables.
        assert_eq!(norm_lang("c"), Lang::C);
        assert_eq!(norm_lang("cpp"), Lang::C);
        assert_eq!(norm_lang("c++"), Lang::C);
        assert_eq!(norm_lang("go"), Lang::Go);
        assert_eq!(norm_lang("java"), Lang::Java);
        assert_eq!(norm_lang("cs"), Lang::CSharp);
        assert_eq!(norm_lang("sql"), Lang::Sql);
    }

    #[test]
    fn c_family_block_comment_char_and_strings() {
        let line = "const char c = 'x'; /* note */ char *s = \"hi\";";
        let sp = spans(Lang::C, line);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &line[s.start..s.end] == "const"));
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Str && &line[s.start..s.end] == "'x'"));
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Comment && &line[s.start..s.end] == "/* note */"));
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Str && &line[s.start..s.end] == "\"hi\""));
        assert_eq!(sp.first().map(|s| s.start), Some(0));
        assert_eq!(sp.last().map(|s| s.end), Some(line.len()));
    }

    #[test]
    fn block_comment_carries_across_lines() {
        let body = "int a; /* start\nstill \"quoted\"\nend */ int b;";
        let lines: Vec<&str> = body.split('\n').collect();
        let sp = tokenize(Lang::C, body);
        assert_eq!(sp.len(), lines.len());
        // Opener: plain code first, then the comment runs to the line end.
        assert!(sp[0]
            .iter()
            .any(|s| s.kind == TokenKind::Comment && &lines[0][s.start..s.end] == "/* start"));
        // A line wholly inside the comment is one comment span: its quotes
        // and words are comment text, not tokens.
        assert_eq!(sp[1].len(), 1);
        assert_eq!(sp[1][0].kind, TokenKind::Comment);
        assert_eq!((sp[1][0].start, sp[1][0].end), (0, lines[1].len()));
        // The closing `*/` ends the comment; tokens after it highlight again.
        assert!(sp[2]
            .iter()
            .any(|s| s.kind == TokenKind::Comment && &lines[2][s.start..s.end] == "end */"));
        assert!(sp[2]
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &lines[2][s.start..s.end] == "int"));
    }

    #[test]
    fn blank_line_inside_block_comment_keeps_state() {
        let body = "/* open\n\nclose */\nlet x = 1;";
        let sp = tokenize(Lang::Rust, body);
        assert!(sp[1].is_empty(), "a blank line has no spans");
        assert!(sp[2]
            .iter()
            .any(|s| s.kind == TokenKind::Comment && &"close */"[s.start..s.end] == "close */"));
        assert!(sp[3]
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &"let x = 1;"[s.start..s.end] == "let"));
    }

    #[test]
    fn unclosed_block_comment_runs_to_end_of_block() {
        let body = "let a = 1;\n/* open\nstill open";
        let sp = tokenize(Lang::Rust, body);
        assert!(sp[0]
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &"let a = 1;"[s.start..s.end] == "let"));
        for (line, spans) in [("/* open", &sp[1]), ("still open", &sp[2])] {
            assert_eq!(spans.len(), 1);
            assert_eq!(spans[0].kind, TokenKind::Comment);
            assert_eq!((spans[0].start, spans[0].end), (0, line.len()));
        }
        // The open state belongs to one block only: a fresh scan starts
        // outside the comment.
        let fresh = tokenize(Lang::Rust, "let b = 2;");
        assert!(fresh[0]
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &"let b = 2;"[s.start..s.end] == "let"));
    }

    #[test]
    fn sql_block_comment_carries_across_lines() {
        let body = "SELECT 1;\n/* note\nstill note */ SELECT 2;";
        let sp = tokenize(Lang::Sql, body);
        assert_eq!(sp[1].len(), 1);
        assert_eq!(sp[1][0].kind, TokenKind::Comment);
        assert!(sp[2]
            .iter()
            .any(|s| s.kind == TokenKind::Comment
                && &"still note */"[s.start..s.end] == "still note */"));
        assert!(sp[2].iter().any(|s| {
            s.kind == TokenKind::Keyword && &"still note */ SELECT 2;"[s.start..s.end] == "SELECT"
        }));
    }

    #[test]
    fn spans_with_state_round_trips_an_open_comment() {
        let (first, open) = spans_with_state(Lang::C, "x /* open", State::default());
        assert!(open.in_block_comment);
        assert!(first
            .iter()
            .any(|s| s.kind == TokenKind::Comment && &"x /* open"[s.start..s.end] == "/* open"));
        let line = "close */ y";
        let (second, closed) = spans_with_state(Lang::C, line, open);
        assert!(!closed.in_block_comment);
        assert!(second
            .iter()
            .any(|s| s.kind == TokenKind::Comment && &line[s.start..s.end] == "close */"));
    }

    #[test]
    fn block_comment_state_does_not_leak_into_other_languages() {
        let (_, open) = spans_with_state(Lang::C, "/*", State::default());
        assert!(open.in_block_comment);
        let (sp, state) = spans_with_state(Lang::Python, "x = 1", open);
        assert!(!state.in_block_comment);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Number && &"x = 1"[s.start..s.end] == "1"));
    }

    #[test]
    fn go_java_csharp_and_sql_tokens() {
        let go = "func main() { s := `raw` }";
        let sp = spans(Lang::Go, go);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &go[s.start..s.end] == "func"));
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Str && &go[s.start..s.end] == "`raw`"));

        let java = "public final class A { int value; }";
        let sp = spans(Lang::Java, java);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &java[s.start..s.end] == "class"));

        let cs = "public string Name { get; set; }";
        let sp = spans(Lang::CSharp, cs);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &cs[s.start..s.end] == "string"));
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &cs[s.start..s.end] == "get"));

        // SQL keywords are case-insensitive, unlike identifiers in the other
        // highlighted languages.
        let sql = "SELECT id FROM t -- note";
        let sp = spans(Lang::Sql, sql);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &sql[s.start..s.end] == "SELECT"));
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &sql[s.start..s.end] == "FROM"));
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Comment && &sql[s.start..s.end] == "-- note"));
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
    fn multibyte_char_literals_are_strings() {
        // A non-ASCII char literal must advance by whole chars: stopping on a
        // UTF-8 continuation byte used to hide the closing quote and emit the
        // literal as plain text.
        for (lang, line) in [
            (Lang::Rust, "let c = 'é';"),
            (Lang::C, "char c = 'é';"),
            (Lang::Rust, "let c = '😀';"),
        ] {
            let sp = spans(lang, line);
            assert!(
                sp.iter().any(|s| {
                    s.kind == TokenKind::Str && line[s.start..s.end].starts_with('\'')
                }),
                "{line}: no string span in {sp:?}"
            );
        }
        // Escapes and lifetimes still behave as before.
        let escaped = "let c = '\\n';";
        assert!(spans(Lang::Rust, escaped)
            .iter()
            .any(|s| s.kind == TokenKind::Str && &escaped[s.start..s.end] == "'\\n'"));
        let line = "fn f<'a>(x: &'a str)";
        assert!(!spans(Lang::Rust, line)
            .iter()
            .any(|s| s.kind == TokenKind::Str));
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
