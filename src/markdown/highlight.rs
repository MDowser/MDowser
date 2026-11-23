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
//!   `toml`, `yaml`, C/C++, Go, Java, C#, SQL, PHP, Ruby, Swift, Kotlin, Lua,
//!   Dart, CSS, PowerShell, Zig, Nix, Haskell, Dockerfile, Makefile, Scala,
//!   Elixir and R. Anything else falls back to `Plain`.
//! - Block comments (`/* ... */`) are tracked across lines through
//!   [`State`]: [`tokenize`] scans a whole block so a comment opened on one
//!   line colors the following lines until its `*/` (or the end of the
//!   block). The scanner itself stays line-based, so strings and line
//!   comments still end with their line.
//! - Heuristics are intentionally small: keywords, strings, comments,
//!   numbers and function calls (`name(`). A single quote is a character
//!   literal only in languages that have them (`'a'`, `'\u{1F600}'`); a
//!   lifetime like `'a` stays plain instead of becoming a string.
//! - Fenced info strings accept the common Markdown spellings:
//!   ` ```rust `, ` ```{.rust} `, ` ```{.rust .numberLines} ` and rustdoc's
//!   ` ```rust,ignore ` all select the same language.

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
    Php,
    Ruby,
    Swift,
    Kotlin,
    Lua,
    Dart,
    Css,
    PowerShell,
    Zig,
    Nix,
    Haskell,
    Dockerfile,
    Makefile,
    Scala,
    Elixir,
    R,
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
    let first = info_lang(info);
    let eq = |alias: &str| first.eq_ignore_ascii_case(alias);
    if eq("rust") || eq("rs") {
        Lang::Rust
    } else if eq("python") || eq("py") {
        Lang::Python
    } else if eq("javascript") || eq("js") || eq("typescript") || eq("ts") || eq("jsx") || eq("tsx")
    {
        Lang::JavaScript
    } else if eq("bash")
        || eq("sh")
        || eq("zsh")
        || eq("shell")
        || eq("console")
        || eq("shellsession")
    {
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
    } else if eq("php") {
        Lang::Php
    } else if eq("ruby") || eq("rb") {
        Lang::Ruby
    } else if eq("swift") {
        Lang::Swift
    } else if eq("kotlin") || eq("kt") || eq("kts") {
        Lang::Kotlin
    } else if eq("lua") {
        Lang::Lua
    } else if eq("dart") {
        Lang::Dart
    } else if eq("css") {
        Lang::Css
    } else if eq("powershell") || eq("pwsh") || eq("ps1") {
        Lang::PowerShell
    } else if eq("zig") {
        Lang::Zig
    } else if eq("nix") {
        Lang::Nix
    } else if eq("haskell") || eq("hs") {
        Lang::Haskell
    } else if eq("dockerfile") || eq("docker") || eq("containerfile") {
        Lang::Dockerfile
    } else if eq("makefile") || eq("make") || eq("gnumake") || eq("mk") {
        Lang::Makefile
    } else if eq("scala") || eq("sc") {
        Lang::Scala
    } else if eq("elixir") || eq("ex") || eq("exs") {
        Lang::Elixir
    } else if eq("r") {
        Lang::R
    } else {
        Lang::Plain
    }
}

/// The language token of a fenced info string: its first whitespace-delimited
/// word (at most `MAX_LANG_CHARS` chars), with the wrappers common Markdown
/// dialects add removed (`"js title=\"app.js\""` -> `"js"`,
/// `"{.rust .numberLines}"` -> `"rust"`, `"rust,ignore"` -> `"rust"`). This is
/// what the code card shows as its chip and what [`norm_lang`] resolves, so
/// docs-generator metadata never leaks in.
pub fn info_lang(info: &str) -> &str {
    // Bound the scan: the useful tag is short, and a hostile fence info
    // string must not be walked in full on every repaint. The wider prefix
    // leaves room for indentation without letting it grow with the body.
    let prefix_end = info
        .char_indices()
        .nth(crate::config::MAX_LANG_CHARS.saturating_mul(4))
        .map_or(info.len(), |(i, _)| i);
    let trimmed = info[..prefix_end].trim_start();
    let mut end = trimmed.len();
    for (n, (i, c)) in trimmed.char_indices().enumerate() {
        if c.is_whitespace() || n >= crate::config::MAX_LANG_CHARS {
            end = i;
            break;
        }
    }
    let token = &trimmed[..end];
    // Only a `{...}` class list gets its braces/dot stripped: a bare
    // `.gitignore` or `.env` is a file name, not a Pandoc class.
    let token = match token.strip_prefix('{') {
        Some(rest) => rest.trim_start_matches('.').trim_end_matches('}'),
        None => token,
    };
    token.split(',').next().unwrap_or("")
}

/// `title="…"` (also unquoted) from a fenced info string, if present: the tab
/// label a `<CodeGroup>` child shows. The label is capped at
/// `MAX_COMPONENT_LABEL_CHARS`, the same bound the viewer applies to labels,
/// so a hostile title cannot inflate the normalized document.
pub fn info_title(info: &str) -> Option<String> {
    let key = "title=";
    let mut at = 0;
    while let Some(rel) = info[at..].find(key) {
        let start = at + rel;
        let starts_token = info[..start]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        if starts_token {
            let after = &info[start + key.len()..];
            let value = match after.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let body = &after[quote.len_utf8()..];
                    match body.find(quote) {
                        Some(end) => &body[..end],
                        None => body,
                    }
                }
                _ => {
                    let end = after.find(char::is_whitespace).unwrap_or(after.len());
                    &after[..end]
                }
            };
            let value = value.trim();
            if !value.is_empty() {
                return Some(
                    crate::util::truncate_chars(value, crate::config::MAX_COMPONENT_LABEL_CHARS)
                        .into_owned(),
                );
            }
        }
        at = start + key.len();
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
        Lang::Php => &[
            "abstract",
            "and",
            "array",
            "as",
            "break",
            "callable",
            "case",
            "catch",
            "class",
            "clone",
            "const",
            "continue",
            "declare",
            "default",
            "do",
            "echo",
            "else",
            "elseif",
            "empty",
            "enddeclare",
            "endfor",
            "endforeach",
            "endif",
            "endswitch",
            "endwhile",
            "enum",
            "eval",
            "exit",
            "extends",
            "false",
            "final",
            "finally",
            "fn",
            "for",
            "foreach",
            "function",
            "global",
            "goto",
            "if",
            "implements",
            "include",
            "include_once",
            "instanceof",
            "insteadof",
            "interface",
            "isset",
            "list",
            "match",
            "namespace",
            "new",
            "null",
            "or",
            "print",
            "private",
            "protected",
            "public",
            "readonly",
            "require",
            "require_once",
            "return",
            "static",
            "switch",
            "throw",
            "trait",
            "true",
            "try",
            "unset",
            "use",
            "var",
            "while",
            "xor",
            "yield",
        ],
        Lang::Ruby => &[
            "alias",
            "and",
            "attr_accessor",
            "attr_reader",
            "attr_writer",
            "begin",
            "break",
            "case",
            "class",
            "def",
            "do",
            "else",
            "elsif",
            "end",
            "ensure",
            "false",
            "for",
            "if",
            "in",
            "lambda",
            "module",
            "next",
            "nil",
            "not",
            "or",
            "private",
            "proc",
            "protected",
            "public",
            "puts",
            "raise",
            "redo",
            "require",
            "require_relative",
            "rescue",
            "retry",
            "return",
            "self",
            "super",
            "then",
            "true",
            "undef",
            "unless",
            "until",
            "when",
            "while",
            "yield",
        ],
        Lang::Swift => &[
            "Any",
            "Self",
            "actor",
            "any",
            "as",
            "associatedtype",
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "continue",
            "default",
            "defer",
            "deinit",
            "do",
            "else",
            "enum",
            "extension",
            "fallthrough",
            "false",
            "fileprivate",
            "for",
            "func",
            "guard",
            "if",
            "import",
            "in",
            "init",
            "inout",
            "internal",
            "is",
            "let",
            "nil",
            "nonisolated",
            "open",
            "operator",
            "private",
            "protocol",
            "public",
            "repeat",
            "rethrows",
            "return",
            "self",
            "some",
            "static",
            "struct",
            "subscript",
            "super",
            "switch",
            "throw",
            "throws",
            "true",
            "try",
            "typealias",
            "var",
            "where",
            "while",
        ],
        Lang::Kotlin => &[
            "abstract",
            "actual",
            "annotation",
            "as",
            "break",
            "by",
            "catch",
            "class",
            "companion",
            "const",
            "constructor",
            "continue",
            "crossinline",
            "data",
            "delegate",
            "do",
            "dynamic",
            "else",
            "enum",
            "expect",
            "external",
            "false",
            "field",
            "file",
            "final",
            "finally",
            "for",
            "fun",
            "get",
            "if",
            "import",
            "in",
            "infix",
            "init",
            "inline",
            "inner",
            "interface",
            "internal",
            "is",
            "lateinit",
            "noinline",
            "null",
            "object",
            "open",
            "operator",
            "out",
            "override",
            "package",
            "param",
            "private",
            "property",
            "protected",
            "public",
            "receiver",
            "reified",
            "return",
            "sealed",
            "set",
            "setparam",
            "super",
            "suspend",
            "tailrec",
            "this",
            "throw",
            "true",
            "try",
            "typealias",
            "typeof",
            "val",
            "value",
            "var",
            "vararg",
            "when",
            "where",
            "while",
        ],
        Lang::Lua => &[
            "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto",
            "if", "in", "local", "nil", "not", "or", "repeat", "require", "return", "self", "then",
            "true", "until", "while",
        ],
        Lang::Dart => &[
            "Function",
            "abstract",
            "as",
            "assert",
            "async",
            "await",
            "break",
            "case",
            "catch",
            "class",
            "const",
            "continue",
            "covariant",
            "default",
            "deferred",
            "do",
            "dynamic",
            "else",
            "enum",
            "export",
            "extends",
            "extension",
            "external",
            "factory",
            "false",
            "final",
            "finally",
            "for",
            "get",
            "hide",
            "if",
            "implements",
            "import",
            "in",
            "interface",
            "is",
            "late",
            "library",
            "mixin",
            "new",
            "null",
            "on",
            "operator",
            "part",
            "required",
            "rethrow",
            "return",
            "sealed",
            "set",
            "show",
            "static",
            "super",
            "switch",
            "sync",
            "this",
            "throw",
            "true",
            "try",
            "typedef",
            "var",
            "void",
            "when",
            "while",
            "with",
            "yield",
        ],
        Lang::Css => &[
            "charset",
            "container",
            "counter-style",
            "document",
            "font-face",
            "font-feature-values",
            "important",
            "import",
            "keyframes",
            "layer",
            "media",
            "namespace",
            "page",
            "property",
            "scope",
            "starting-style",
            "supports",
            "view-transition",
        ],
        Lang::PowerShell => &[
            "begin",
            "break",
            "catch",
            "class",
            "configuration",
            "continue",
            "data",
            "define",
            "do",
            "dynamicparam",
            "else",
            "elseif",
            "end",
            "enum",
            "exit",
            "filter",
            "finally",
            "for",
            "foreach",
            "from",
            "function",
            "hidden",
            "if",
            "in",
            "inlinescript",
            "param",
            "parallel",
            "process",
            "return",
            "sequence",
            "static",
            "switch",
            "throw",
            "trap",
            "try",
            "until",
            "using",
            "var",
            "while",
            "workflow",
        ],
        Lang::Zig => &[
            "addrspace",
            "align",
            "allowzero",
            "and",
            "anyframe",
            "anytype",
            "asm",
            "async",
            "await",
            "break",
            "callconv",
            "catch",
            "comptime",
            "const",
            "continue",
            "defer",
            "else",
            "enum",
            "errdefer",
            "error",
            "export",
            "extern",
            "fn",
            "for",
            "if",
            "inline",
            "linksection",
            "noalias",
            "noinline",
            "nosuspend",
            "opaque",
            "or",
            "orelse",
            "packed",
            "pub",
            "resume",
            "return",
            "struct",
            "switch",
            "test",
            "threadlocal",
            "try",
            "union",
            "unreachable",
            "usingnamespace",
            "var",
            "volatile",
            "while",
        ],
        Lang::Nix => &[
            "assert", "builtins", "else", "if", "import", "in", "inherit", "let", "map", "or",
            "rec", "then", "toString", "with",
        ],
        Lang::Haskell => &[
            "as",
            "case",
            "class",
            "data",
            "default",
            "deriving",
            "do",
            "else",
            "export",
            "foreign",
            "hiding",
            "if",
            "import",
            "in",
            "infix",
            "infixl",
            "infixr",
            "instance",
            "let",
            "mdo",
            "module",
            "newtype",
            "of",
            "proc",
            "qualified",
            "rec",
            "safe",
            "then",
            "type",
            "unsafe",
            "where",
        ],
        Lang::Dockerfile => &[
            "add",
            "arg",
            "as",
            "cmd",
            "copy",
            "entrypoint",
            "env",
            "expose",
            "from",
            "healthcheck",
            "label",
            "maintainer",
            "onbuild",
            "run",
            "shell",
            "stopsignal",
            "user",
            "volume",
            "workdir",
        ],
        Lang::Makefile => &[
            "addprefix",
            "addsuffix",
            "basename",
            "call",
            "define",
            "dir",
            "else",
            "endef",
            "endif",
            "error",
            "eval",
            "export",
            "filter",
            "findstring",
            "firstword",
            "flavor",
            "foreach",
            "ifdef",
            "ifeq",
            "ifndef",
            "ifneq",
            "include",
            "info",
            "join",
            "lastword",
            "notdir",
            "origin",
            "override",
            "patsubst",
            "private",
            "shell",
            "sort",
            "strip",
            "subst",
            "suffix",
            "undefine",
            "unexport",
            "vpath",
            "warning",
            "wildcard",
            "word",
            "words",
        ],
        Lang::Scala => &[
            "abstract",
            "case",
            "catch",
            "class",
            "def",
            "derives",
            "do",
            "else",
            "end",
            "enum",
            "export",
            "extension",
            "extends",
            "false",
            "final",
            "finally",
            "for",
            "forSome",
            "given",
            "if",
            "implicit",
            "import",
            "inline",
            "lazy",
            "match",
            "new",
            "null",
            "object",
            "opaque",
            "open",
            "override",
            "package",
            "private",
            "protected",
            "return",
            "sealed",
            "super",
            "then",
            "this",
            "throw",
            "trait",
            "transparent",
            "true",
            "try",
            "type",
            "using",
            "val",
            "var",
            "while",
            "with",
            "yield",
        ],
        Lang::Elixir => &[
            "after",
            "alias",
            "and",
            "case",
            "catch",
            "cond",
            "def",
            "defdelegate",
            "defguard",
            "defguardp",
            "defimpl",
            "defmacro",
            "defmodule",
            "defp",
            "defprotocol",
            "defstruct",
            "do",
            "else",
            "end",
            "false",
            "fn",
            "for",
            "if",
            "import",
            "in",
            "nil",
            "not",
            "or",
            "quote",
            "receive",
            "require",
            "rescue",
            "super",
            "true",
            "try",
            "unless",
            "unquote",
            "use",
            "when",
        ],
        // Reserved words and constants only: base-R functions such as
        // `mean(` are recognized by the function-call heuristic, and dotted
        // names (`data.frame`) are not a single identifier for this scanner.
        Lang::R => &[
            "FALSE",
            "Inf",
            "NA",
            "NA_character_",
            "NA_integer_",
            "NA_real_",
            "NaN",
            "NULL",
            "TRUE",
            "break",
            "else",
            "for",
            "function",
            "if",
            "in",
            "next",
            "repeat",
            "return",
            "while",
        ],
        Lang::Plain => &[],
    }
}

fn is_kw(lang: Lang, word: &str) -> bool {
    match lang {
        // SQL and Dockerfile write their keywords in either case; every other
        // language here is case-sensitive, so `Class` must stay plain in
        // JavaScript.
        Lang::Sql | Lang::Dockerfile => keywords(lang)
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

/// How `#` starts a line comment in a language.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HashComment {
    /// `#` has no comment meaning (CSS IDs, Haskell operators).
    None,
    /// Anywhere outside a string (`# note`).
    Anywhere,
    /// Only at the start of a word (Bash `a#b`, YAML `value#suffix`).
    WordStart,
    /// Only at the start of a line, after indentation (Dockerfile).
    LineStart,
}

/// What a single quote means in a language.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SingleQuote {
    /// A single quote is not a delimiter (JSON, Swift, Nix).
    None,
    /// `'...'` is a string (Python, Ruby, SQL, ...).
    String,
    /// `'a'` is a character literal (Rust, C, Go, ...).
    Char,
}

/// Lexical rules that vary by language. Keeping them in one place means the
/// scanner has no long `matches!` chains per token and a new language only
/// has to fill this table plus its keyword list.
#[derive(Clone, Copy)]
struct Syntax {
    hash_comment: HashComment,
    /// `//` starts a line comment.
    slash_comment: bool,
    /// `/* ... */` marks a block comment (state carried across lines).
    block_comment: bool,
    /// `--` starts a line comment.
    dash_comment: bool,
    single_quote: SingleQuote,
    /// A backtick starts a string (JS templates, Go raw strings, Bash
    /// command substitution, Ruby command execution).
    backtick_string: bool,
    /// A backtick escapes the next char inside a double-quoted string
    /// (PowerShell), rather than closing it.
    backtick_escape: bool,
    /// PHP 8 attributes (`#[...]`) are syntax, not `#` comments.
    php_attribute: bool,
}

impl Syntax {
    fn for_lang(lang: Lang) -> Self {
        use HashComment::{Anywhere, LineStart, None as NoHash, WordStart};
        use SingleQuote::{Char, None as NoQuote, String as StrQuote};
        Self {
            hash_comment: match lang {
                Lang::Bash | Lang::Yaml => WordStart,
                Lang::Python
                | Lang::Toml
                | Lang::Php
                | Lang::Ruby
                | Lang::PowerShell
                | Lang::Nix
                | Lang::Makefile
                | Lang::Elixir
                | Lang::R => Anywhere,
                Lang::Dockerfile => LineStart,
                _ => NoHash,
            },
            slash_comment: matches!(
                lang,
                Lang::Rust
                    | Lang::JavaScript
                    | Lang::C
                    | Lang::Go
                    | Lang::Java
                    | Lang::CSharp
                    | Lang::Php
                    | Lang::Swift
                    | Lang::Kotlin
                    | Lang::Dart
                    | Lang::Scala
                    | Lang::Zig
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
                    | Lang::Php
                    | Lang::Swift
                    | Lang::Kotlin
                    | Lang::Dart
                    | Lang::Css
                    | Lang::Nix
                    | Lang::Scala
            ),
            dash_comment: matches!(lang, Lang::Sql | Lang::Lua | Lang::Haskell),
            single_quote: match lang {
                Lang::Json | Lang::Swift | Lang::Nix => NoQuote,
                Lang::Rust
                | Lang::C
                | Lang::Go
                | Lang::Java
                | Lang::CSharp
                | Lang::Kotlin
                | Lang::Zig
                | Lang::Scala
                | Lang::Haskell => Char,
                _ => StrQuote,
            },
            backtick_string: matches!(lang, Lang::JavaScript | Lang::Go | Lang::Bash | Lang::Ruby),
            backtick_escape: lang == Lang::PowerShell,
            php_attribute: lang == Lang::Php,
        }
    }
}

/// Does `#` at `i` start a line comment for this language? `bytes` is the
/// current line, so the decision can look at what precedes the marker.
fn hash_starts_comment(kind: HashComment, bytes: &[u8], i: usize) -> bool {
    match kind {
        HashComment::None => false,
        HashComment::Anywhere => true,
        HashComment::WordStart => {
            i == 0
                || bytes[i - 1].is_ascii_whitespace()
                || matches!(bytes[i - 1], b';' | b'|' | b'&' | b'(' | b')' | b'<' | b'>')
        }
        HashComment::LineStart => bytes[..i].iter().all(|b| b.is_ascii_whitespace()),
    }
}

/// End (one past the closing quote) of a character literal starting at
/// `start`, or `None` when this is not one (`'a` lifetime, `'ab'`, an
/// unterminated quote).
///
/// Handles the escape forms char literals actually use: one escaped char
/// (`'\n'`, `'\''`), `\xNN` (C/Go/PHP), 4-digit `\uNNNN` (Java/C++/Go),
/// `\U00000000` (Go/C++) and Rust's `\u{...}`. Longer escapes were the reason
/// `'\u{1F600}'` used to render as plain text.
fn char_literal_end(line: &str, start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let n = bytes.len();
    let mut j = start + 1;
    if j >= n || bytes[j] == b'\'' {
        return None;
    }
    if bytes[j] == b'\\' {
        j += 1;
        if j >= n {
            return None;
        }
        match bytes[j] {
            b'u' => {
                j += 1;
                if j < n && bytes[j] == b'{' {
                    // `\u{...}`: at most 8 hex digits plus the closing brace.
                    let limit = (j + 10).min(n);
                    let mut k = j + 1;
                    while k < limit && bytes[k] != b'}' {
                        k += 1;
                    }
                    if k >= limit || bytes[k] != b'}' {
                        return None;
                    }
                    j = k + 1;
                } else {
                    for _ in 0..4 {
                        if j >= n || !bytes[j].is_ascii_hexdigit() {
                            return None;
                        }
                        j += 1;
                    }
                }
            }
            b'U' => {
                j += 1;
                for _ in 0..8 {
                    if j >= n || !bytes[j].is_ascii_hexdigit() {
                        return None;
                    }
                    j += 1;
                }
            }
            b'x' => {
                // C/C++ consume as many hex digits as are present (at least
                // one); Go and PHP require two. Accepting one or two keeps
                // `'\x4'` valid without rejecting the stricter forms.
                j += 1;
                let mut digits = 0;
                while digits < 2 && j < n && bytes[j].is_ascii_hexdigit() {
                    j += 1;
                    digits += 1;
                }
                if digits == 0 {
                    return None;
                }
            }
            _ => j += char_len_at(line, j),
        }
    } else {
        // One plain char, which may be multi-byte (`'é'`, `'😀'`).
        j += char_len_at(line, j);
    }
    (j < n && bytes[j] == b'\'').then_some(j + 1)
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
        // `#` line comment (placement varies: Python/Ruby anywhere, Bash at
        // a word start, Dockerfile at line start). PHP 8's `#[...]` attribute
        // syntax is not a comment.
        if b == b'#'
            && hash_starts_comment(syntax.hash_comment, bytes, i)
            && !(syntax.php_attribute && i + 1 < n && bytes[i + 1] == b'[')
        {
            flush_plain(&mut out, &mut plain_start, i);
            push_span(&mut out, TokenKind::Comment, i, n);
            break;
        }
        // `--` line comment (SQL, Lua, Haskell).
        if syntax.dash_comment && b == b'-' && i + 1 < n && bytes[i + 1] == b'-' {
            flush_plain(&mut out, &mut plain_start, i);
            push_span(&mut out, TokenKind::Comment, i, n);
            break;
        }
        // Strings: `"..."`, `` `...` ``, and `'...'` (per-language).
        let quote_is_open = b == b'"'
            || (b == b'\'' && syntax.single_quote != SingleQuote::None)
            || (b == b'`' && syntax.backtick_string);
        if quote_is_open {
            // Character literals (`'a'`, `'\u{1F600}'`) exist only in some
            // languages. A failed check is a lifetime (`'a`, `'static`) or an
            // invalid literal: keep the quote and the identifier it
            // introduces plain, so `'static` cannot turn `static` into a
            // keyword.
            if b == b'\'' && syntax.single_quote == SingleQuote::Char {
                if let Some(end) = char_literal_end(line, i) {
                    flush_plain(&mut out, &mut plain_start, i);
                    push_span(&mut out, TokenKind::Str, i, end);
                    i = end;
                    continue;
                }
                let mut j = i + 1;
                while j < n && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                flush_plain(&mut out, &mut plain_start, i);
                push_span(&mut out, TokenKind::Plain, i, j.max(i + 1));
                i = j.max(i + 1);
                continue;
            }
            let quote = bytes[i];
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
                // PowerShell escapes a double quote with a backtick, not a
                // backslash (`"a `"quoted`" b"`).
                if syntax.backtick_escape && quote == b'"' && bytes[j] == b'`' && j + 1 < n {
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
            // Unclosed string: highlight to end of line.
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
    use super::{
        info_lang, info_title, norm_lang, spans, spans_with_state, tokenize, Lang, State, TokenKind,
    };

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

    #[test]
    fn normalizes_expanded_language_aliases() {
        for (alias, lang) in [
            ("php", Lang::Php),
            ("rb", Lang::Ruby),
            ("swift", Lang::Swift),
            ("kt", Lang::Kotlin),
            ("lua", Lang::Lua),
            ("dart", Lang::Dart),
            ("css", Lang::Css),
            ("pwsh", Lang::PowerShell),
            ("zig", Lang::Zig),
            ("nix", Lang::Nix),
            ("hs", Lang::Haskell),
            ("dockerfile", Lang::Dockerfile),
            ("make", Lang::Makefile),
            ("scala", Lang::Scala),
            ("exs", Lang::Elixir),
            ("r", Lang::R),
        ] {
            assert_eq!(norm_lang(alias), lang, "{alias}");
        }
        // Pandoc/Quarto class wrappers and rustdoc attributes resolve to the
        // same language as a bare fence tag.
        assert_eq!(norm_lang("{.rust}"), Lang::Rust);
        assert_eq!(norm_lang("{.rust .numberLines}"), Lang::Rust);
        assert_eq!(norm_lang("rust,ignore"), Lang::Rust);
        assert_eq!(norm_lang("{.c++}"), Lang::C);
        assert_eq!(info_lang("{.rust .numberLines}"), "rust");
        assert_eq!(info_lang("rust,ignore"), "rust");
        assert_eq!(info_lang("js title=\"app.js\""), "js");
        // A quoted fence title keeps its spaces; an unquoted one stops at the
        // first whitespace, and `subtitle=` is not a title.
        assert_eq!(
            info_title("js title=\"My Example\"").as_deref(),
            Some("My Example")
        );
        assert_eq!(
            info_title("js title='My Example'").as_deref(),
            Some("My Example")
        );
        assert_eq!(info_title("js title=app.js").as_deref(), Some("app.js"));
        assert_eq!(info_title("js subtitle=app.js"), None);
        // Bare dotfiles are file names, not class wrappers.
        assert_eq!(info_lang(".gitignore"), ".gitignore");
        assert_eq!(norm_lang(".gitignore"), Lang::Plain);
        // The chip token is bounded, and leading whitespace is ignored.
        assert_eq!(info_lang("  rust"), "rust");
        // A pathological leading gap is bounded too (it fails closed rather
        // than scanning the whole body on every repaint).
        let padded = format!("{}rust", " ".repeat(300));
        assert_eq!(info_lang(&padded), "");
        let long = "a".repeat(4_000);
        assert_eq!(
            info_lang(&long).chars().count(),
            crate::config::MAX_LANG_CHARS
        );
        // The cap lands on a char boundary for multi-byte tags too.
        let unicode = "é".repeat(100);
        assert_eq!(
            info_lang(&unicode).chars().count(),
            crate::config::MAX_LANG_CHARS
        );
        // A fence title is bounded at the component-label cap.
        let long_title = format!("js title=\"{}\"", "x".repeat(4_000));
        assert_eq!(
            info_title(&long_title).unwrap().chars().count(),
            crate::config::MAX_COMPONENT_LABEL_CHARS
        );
    }

    #[test]
    fn new_language_tokens() {
        fn has(lang: Lang, line: &str, kind: TokenKind, text: &str) -> bool {
            spans(lang, line)
                .iter()
                .any(|s| s.kind == kind && &line[s.start..s.end] == text)
        }

        let php = "function greet($name) { return \"hi\"; } // note";
        assert!(has(Lang::Php, php, TokenKind::Keyword, "function"));
        assert!(has(Lang::Php, php, TokenKind::Str, "\"hi\""));
        assert!(has(Lang::Php, php, TokenKind::Comment, "// note"));
        // PHP 8 attributes are not `#` comments.
        assert!(!spans(Lang::Php, "#[Attribute] class A {}")
            .iter()
            .any(|s| s.kind == TokenKind::Comment));

        let ruby = "def greet(name) # note";
        assert!(has(Lang::Ruby, ruby, TokenKind::Keyword, "def"));
        assert!(has(Lang::Ruby, ruby, TokenKind::Comment, "# note"));

        let swift = "func greet() -> String { return \"hi\" }";
        assert!(has(Lang::Swift, swift, TokenKind::Keyword, "func"));
        assert!(has(Lang::Swift, swift, TokenKind::Keyword, "return"));
        assert!(has(Lang::Swift, swift, TokenKind::Str, "\"hi\""));

        let kotlin = "fun main() { val x = 'a' }";
        assert!(has(Lang::Kotlin, kotlin, TokenKind::Keyword, "fun"));
        assert!(has(Lang::Kotlin, kotlin, TokenKind::Keyword, "val"));
        assert!(has(Lang::Kotlin, kotlin, TokenKind::Str, "'a'"));

        let lua = "local x = 42 -- note";
        assert!(has(Lang::Lua, lua, TokenKind::Keyword, "local"));
        assert!(has(Lang::Lua, lua, TokenKind::Number, "42"));
        assert!(has(Lang::Lua, lua, TokenKind::Comment, "-- note"));

        let dart = "void main() { final x = 1; }";
        assert!(has(Lang::Dart, dart, TokenKind::Keyword, "void"));
        assert!(has(Lang::Dart, dart, TokenKind::Keyword, "final"));

        let css = "@media (min-width: 600px) { .a { color: #fff; } } /* note */";
        assert!(has(Lang::Css, css, TokenKind::Keyword, "media"));
        assert!(has(Lang::Css, css, TokenKind::Comment, "/* note */"));
        // `#fff` is a color, not a `#` comment.
        assert!(!spans(Lang::Css, css)
            .iter()
            .any(|s| s.kind == TokenKind::Comment && css[s.start..s.end].contains("#fff")));

        let ps = "Write-Host \"hi\" # note";
        assert!(has(Lang::PowerShell, ps, TokenKind::Str, "\"hi\""));
        assert!(has(Lang::PowerShell, ps, TokenKind::Comment, "# note"));
        // A PowerShell backtick escapes the next char; it does not open a
        // string.
        assert!(!spans(Lang::PowerShell, "Write-Host `n")
            .iter()
            .any(|s| s.kind == TokenKind::Str));
        // Inside double quotes, a backtick escapes the quote instead of
        // ending the string early.
        let ps_escaped = r#"Write-Host "a `"quoted`" b""#;
        assert!(has(
            Lang::PowerShell,
            ps_escaped,
            TokenKind::Str,
            r#""a `"quoted`" b""#
        ));

        let zig = "const x = 'a'; // note";
        assert!(has(Lang::Zig, zig, TokenKind::Keyword, "const"));
        assert!(has(Lang::Zig, zig, TokenKind::Str, "'a'"));
        assert!(has(Lang::Zig, zig, TokenKind::Comment, "// note"));

        let nix = "let x = 1; # note";
        assert!(has(Lang::Nix, nix, TokenKind::Keyword, "let"));
        assert!(has(Lang::Nix, nix, TokenKind::Comment, "# note"));

        let haskell = "main = let x = 1 in x -- note";
        assert!(has(Lang::Haskell, haskell, TokenKind::Keyword, "let"));
        assert!(has(Lang::Haskell, haskell, TokenKind::Keyword, "in"));
        assert!(has(Lang::Haskell, haskell, TokenKind::Comment, "-- note"));

        let docker = "FROM rust:1 AS build";
        assert!(has(Lang::Dockerfile, docker, TokenKind::Keyword, "FROM"));
        assert!(has(Lang::Dockerfile, docker, TokenKind::Keyword, "AS"));
        assert!(has(
            Lang::Dockerfile,
            "# note",
            TokenKind::Comment,
            "# note"
        ));
        // A `#` after an instruction is shell input, not a Dockerfile
        // comment.
        assert!(!has(
            Lang::Dockerfile,
            "RUN echo # shell",
            TokenKind::Comment,
            "# shell"
        ));

        let make = "all: build # note";
        assert!(has(Lang::Makefile, make, TokenKind::Comment, "# note"));

        let scala = "object Main extends App { val x = 1 }";
        assert!(has(Lang::Scala, scala, TokenKind::Keyword, "object"));
        assert!(has(Lang::Scala, scala, TokenKind::Keyword, "extends"));
        assert!(has(Lang::Scala, scala, TokenKind::Keyword, "val"));

        let elixir = "defmodule Foo do # note";
        assert!(has(Lang::Elixir, elixir, TokenKind::Keyword, "defmodule"));
        assert!(has(Lang::Elixir, elixir, TokenKind::Comment, "# note"));

        let r = "f <- function(x) { if (x > 0) mean(x) else NA }";
        assert!(has(Lang::R, r, TokenKind::Keyword, "function"));
        assert!(has(Lang::R, r, TokenKind::Keyword, "if"));
        assert!(has(Lang::R, r, TokenKind::Keyword, "NA"));
        // Base-R functions are calls, not reserved words.
        assert!(has(Lang::R, r, TokenKind::Func, "mean"));
    }

    #[test]
    fn hash_comments_respect_language_placement() {
        // Bash and YAML only start a comment at a word boundary.
        assert!(!spans(Lang::Bash, "echo a#b")
            .iter()
            .any(|s| s.kind == TokenKind::Comment));
        assert!(spans(Lang::Bash, "echo a #b")
            .iter()
            .any(|s| s.kind == TokenKind::Comment));
        let yaml = "url: https://example.com/#fragment";
        assert!(!spans(Lang::Yaml, yaml)
            .iter()
            .any(|s| s.kind == TokenKind::Comment));
        let yaml_note = "key: value # note";
        assert!(spans(Lang::Yaml, yaml_note)
            .iter()
            .any(|s| s.kind == TokenKind::Comment));

        // TOML follows its spec: `#` starts a comment anywhere outside a
        // string.
        let toml = "url = \"https://example.com/#fragment\" # note";
        let sp = spans(Lang::Toml, toml);
        assert!(sp
            .iter()
            .any(|s| s.kind == TokenKind::Comment && &toml[s.start..s.end] == "# note"));
        let toml_inline = "key = x#y";
        assert!(spans(Lang::Toml, toml_inline)
            .iter()
            .any(|s| s.kind == TokenKind::Comment && &toml_inline[s.start..s.end] == "#y"));
    }

    #[test]
    fn char_literal_escapes_and_lifetimes() {
        // Rust's brace escape is longer than the old byte cap.
        let rust = "let c = '\\u{1F600}';";
        assert!(spans(Lang::Rust, rust)
            .iter()
            .any(|s| { s.kind == TokenKind::Str && &rust[s.start..s.end] == "'\\u{1F600}'" }));
        // C/Go hex escapes.
        let c = "char c = '\\x41';";
        assert!(spans(Lang::C, c)
            .iter()
            .any(|s| s.kind == TokenKind::Str && &c[s.start..s.end] == "'\\x41'"));
        // An invalid multi-char literal is not colored as a string.
        assert!(!spans(Lang::C, "'ab'")
            .iter()
            .any(|s| s.kind == TokenKind::Str));
        // A lifetime stays plain and does not expose its keyword.
        let lifetime = "fn f(x: &'static str) {}";
        let sp = spans(Lang::Rust, lifetime);
        assert!(!sp.iter().any(|s| s.kind == TokenKind::Str));
        assert!(!sp
            .iter()
            .any(|s| s.kind == TokenKind::Keyword && &lifetime[s.start..s.end] == "static"));
    }

    #[test]
    fn quote_modes_match_the_language() {
        // JSON has no single-quoted strings.
        assert!(spans(Lang::Json, "\"ok\"")
            .iter()
            .any(|s| s.kind == TokenKind::Str));
        assert!(!spans(Lang::Json, "'no'")
            .iter()
            .any(|s| s.kind == TokenKind::Str));
        // Swift and Nix do not use single quotes for strings.
        for lang in [Lang::Swift, Lang::Nix] {
            assert!(!spans(lang, "'no'").iter().any(|s| s.kind == TokenKind::Str));
        }
        // Ruby does, and Bash backticks still mark command substitution.
        assert!(spans(Lang::Ruby, "'ok'")
            .iter()
            .any(|s| s.kind == TokenKind::Str));
        assert!(spans(Lang::Bash, "echo `date`")
            .iter()
            .any(|s| s.kind == TokenKind::Str));
    }

    #[test]
    fn every_language_covers_the_line() {
        let langs = [
            Lang::Rust,
            Lang::Python,
            Lang::JavaScript,
            Lang::Bash,
            Lang::Json,
            Lang::Toml,
            Lang::Yaml,
            Lang::C,
            Lang::Go,
            Lang::Java,
            Lang::CSharp,
            Lang::Sql,
            Lang::Php,
            Lang::Ruby,
            Lang::Swift,
            Lang::Kotlin,
            Lang::Lua,
            Lang::Dart,
            Lang::Css,
            Lang::PowerShell,
            Lang::Zig,
            Lang::Nix,
            Lang::Haskell,
            Lang::Dockerfile,
            Lang::Makefile,
            Lang::Scala,
            Lang::Elixir,
            Lang::R,
            Lang::Plain,
        ];
        let line = "if x == \"s\" 'c' # h // s -- d /* b */ `t` 123 caf\u{e9}";
        for lang in langs {
            let sp = spans(lang, line);
            assert_eq!(sp.first().map(|s| s.start), Some(0), "{lang:?}");
            assert_eq!(sp.last().map(|s| s.end), Some(line.len()), "{lang:?}");
            for s in &sp {
                assert!(
                    line.is_char_boundary(s.start) && line.is_char_boundary(s.end),
                    "{lang:?} split a char boundary: {s:?}"
                );
                let _ = &line[s.start..s.end];
            }
        }
    }
}
