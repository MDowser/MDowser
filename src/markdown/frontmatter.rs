// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `markdown::frontmatter`: optional YAML block at the top of the file.
//!
//! SHAPE:
//! ```text
//! ---
//! title: Hello
//! ---
//! # Body...
//! ```
//! - Opening delimiter: first line exactly `---` (surrounding whitespace
//!   tolerated; `\r\n` line endings tolerated).
//! - Closing: a `---` or `...` line at column 0 (indented delimiters inside
//!   block scalars/quoted text stay YAML content).
//! - No opener means no frontmatter (everything is body).
//! - **No closer means no frontmatter**: a leading `---` is a CommonMark
//!   thematic break and the whole text stays content (never an error).
//! - Closer present and the block parses as YAML but is not a mapping
//!   (a sequence or scalar): the block is treated as content too, so
//!   nothing is silently hidden. The viewer parses bodies with YAML
//!   metadata blocks DISABLED for exactly this reason (see
//!   `markdown::document`).
//! - Closer present and the block does not parse as YAML at all (malformed)
//!   => `Err`, which forces trying the next route.

/// Document metadata. All optional.
///
/// - Unknown fields are ignored (tolerance toward varied servers).
/// - Only `title` is surfaced today (OS window title); the rest of the
///   frontmatter is intentionally not displayed (minimal UI).
/// - Parsed exclusively through [`Root`]/[`TitleField`] (never through a
///   direct `Meta` deserialize), so the alias-safe path is the only path.
#[derive(Debug, Clone, Default)]
pub struct Meta {
    /// Optional title for the tab / window header.
    /// Lenient: `title: 123` / `true` coerce to `"123"` / `"true"` and
    /// maps/sequences coerce to `None` instead of rejecting the whole fetch.
    pub title: Option<String>,
}

/// Deserializes the YAML root as a provisional [`Meta`] mapping.
///
/// This replaces the old `serde_norway::Value` root check, which expanded
/// YAML aliases while materializing the tree: a 64 KiB alias fan-out could
/// allocate gigabytes. Unknown fields (including alias bombs) are skipped
/// with `IgnoredAny`, whose `ignore_any` never resolves an alias, while any
/// non-mapping root is reported as `Other` so it stays CommonMark content.
enum Root {
    Mapping(Meta),
    Other,
}

impl<'de> serde::Deserialize<'de> for Root {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Root;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a YAML mapping or any other root")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut meta = Meta::default();
                while let Some(key) = map.next_key::<LenientKey>()? {
                    if key.0.as_deref() == Some("title") {
                        meta.title = map.next_value::<TitleField>()?.0;
                    } else {
                        // `IgnoredAny` skips the value WITHOUT resolving
                        // aliases: the alias bomb stops here.
                        map.next_value::<serde::de::IgnoredAny>()?;
                    }
                }
                Ok(Root::Mapping(meta))
            }

            // Any non-mapping root (sequence, scalar, null, bool...) is not
            // frontmatter: the caller keeps it as content.
            fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
                Ok(Root::Other)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
                Ok(Root::Other)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
                Ok(Root::Other)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
                Ok(Root::Other)
            }
            fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
                Ok(Root::Other)
            }
            fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
                Ok(Root::Other)
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(Root::Other)
            }
            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(Root::Other)
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                // Drain without copying: a sequence root stays content.
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(Root::Other)
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// Newtype so `title` reuses the lenient visitor inside [`Root`].
#[derive(serde::Deserialize)]
struct TitleField(#[serde(deserialize_with = "opt_string_lenient")] Option<String>);

/// Lenient mapping key: scalar keys render to their string form, collection
/// keys (valid YAML, e.g. `? [a]`) are drained without copying and simply
/// never match `title`. Only real syntax errors must move to the next route.
struct LenientKey(Option<String>);

impl<'de> serde::Deserialize<'de> for LenientKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = LenientKey;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a scalar or collection YAML key")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
                Ok(LenientKey(Some(value.to_string())))
            }
            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(LenientKey(Some(value)))
            }
            fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
                Ok(LenientKey(Some(value.to_string())))
            }
            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
                Ok(LenientKey(Some(value.to_string())))
            }
            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
                Ok(LenientKey(Some(value.to_string())))
            }
            fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
                Ok(LenientKey(Some(value.to_string())))
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(LenientKey(None))
            }
            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(LenientKey(None))
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(LenientKey(None))
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                while map
                    .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                    .is_some()
                {}
                Ok(LenientKey(None))
            }
        }
        deserializer.deserialize_any(V)
    }
}

/// Lenient `Option<String>`: accepts string, int, float, bool; `null`/missing
/// -> None. Maps/sequences (e.g. `title: {a: b}`) coerce to None instead of
/// failing the whole frontmatter (the field is optional).
fn opt_string_lenient<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("optional string-coercible")
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D2: serde::Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            // `deserialize_any` (not `Option<String>`) so maps/sequences can
            // be drained to None instead of failing the whole document.
            d.deserialize_any(Inner)
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(Some(v))
        }
    }

    /// Inner visitor: coerces scalars, drains collections to None.
    struct Inner;
    impl<'de> Visitor<'de> for Inner {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a scalar or a collection to ignore")
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(Some(v))
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut m: A,
        ) -> Result<Self::Value, A::Error> {
            // `title: {a: b}`: optional field with a map value -> None.
            while m
                .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                .is_some()
            {}
            Ok(None)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut s: A,
        ) -> Result<Self::Value, A::Error> {
            while s.next_element::<serde::de::IgnoredAny>()?.is_some() {}
            Ok(None)
        }
    }

    d.deserialize_option(V)
}

/// Splits raw text into `(meta, body)`.
///
/// - No leading `---` => `(Meta::default(), full_text)`.
/// - Leading `---` with a closer and a YAML mapping => `(parsed_meta, rest)`.
/// - Leading `---` WITHOUT a closer => `(Meta::default(), full_text)`:
///   a lone thematic break is CommonMark content, never an error.
/// - Leading `---` with a closer but no YAML mapping (no `:`) =>
///   `(Meta::default(), full_text)`: also content (the viewer renders it).
/// - Closer + `:` + unparsable YAML => `Err` (next route is tried).
/// - Oversized YAML mid-scan is bounded; only errors when a closer confirms
///   the block really was frontmatter.
///
/// The body is `Cow::Borrowed` whenever the source is returned unchanged
/// (no frontmatter is the common case): callers no longer copy the whole
/// document just to parse it.
pub fn split(raw: &str) -> Result<(Meta, std::borrow::Cow<'_, str>), String> {
    // Strip UTF-8 BOM when present (`\uFEFF---` must still open frontmatter).
    let raw_stripped = raw.strip_prefix('\u{FEFF}').unwrap_or(raw);
    // Work on the stripped view but keep byte indices relative to it;
    // the returned body is built from the stripped view (BOM never leaks).
    let raw = raw_stripped;
    let mut lines = raw.lines();
    let first = lines.next().unwrap_or("");
    // Opener: `---` (whitespace around it tolerated).
    if first.trim() != "---" {
        return Ok((Meta::default(), std::borrow::Cow::Borrowed(raw)));
    }
    // Scan for the `---` / `...` closer line by line.
    // Track the byte index where the body starts.
    let mut yaml = String::new();
    // Skip past the first line (including its `\n` when present).
    let mut byte_idx = match raw.find('\n') {
        Some(pos) => pos + 1,
        None => return Ok((Meta::default(), std::borrow::Cow::Borrowed(raw))),
    };
    let rest = &raw[byte_idx..];
    let mut body_start: Option<usize> = None;
    // Anti-bomb: stop accumulating once clearly over the cap. We keep
    // scanning (bounded memory) so a later closer still yields the precise
    // "exceeds" error; without a closer the text is simply content.
    let mut over_cap = false;
    for line in rest.split_inclusive('\n') {
        // `line` keeps its trailing `\n` when present; trim to compare.
        // Only a COLUMN-0 delimiter closes the block: an indented `---` or
        // `...` is legitimate YAML content (block scalar / quoted text) and
        // must not truncate the metadata.
        let end = line.trim_end();
        if end == "---" || end == "..." {
            // The body starts after this line.
            body_start = Some(byte_idx + line.len());
            break;
        }
        if !over_cap {
            if yaml.len() + line.len() > crate::config::MAX_FRONTMATTER_BYTES + 1024 {
                over_cap = true;
                yaml.shrink_to_fit();
            } else {
                yaml.push_str(line);
            }
        }
        byte_idx += line.len();
    }
    let body_start = match body_start {
        Some(i) => i,
        None => {
            // No closer: a leading `---` is a thematic break, not metadata.
            // The whole text stays content (CommonMark semantics; the parser
            // runs without YAML metadata blocks so it renders as HR + text).
            return Ok((Meta::default(), std::borrow::Cow::Borrowed(raw)));
        }
    };
    // Anti-bomb: a confirmed frontmatter block over the cap is malformed.
    if over_cap || yaml.len() > crate::config::MAX_FRONTMATTER_BYTES {
        return Err(format!(
            "frontmatter exceeds {} bytes",
            crate::config::MAX_FRONTMATTER_BYTES
        ));
    }
    // Empty YAML -> default (delimiters stripped).
    if yaml.trim().is_empty() {
        let body = std::borrow::Cow::Borrowed(&raw[body_start..]);
        return Ok((Meta::default(), body));
    }
    // Only a YAML MAPPING root is frontmatter. Any other root (a sequence,
    // a scalar — even one containing `:`, a comment-only block, `null`...)
    // is CommonMark content: HR + list/scalar/paragraph, never an error.
    // The custom `Root` deserializer reads the mapping directly and skips
    // unknown fields with `IgnoredAny`, so YAML aliases are never expanded
    // (a 64 KiB alias fan-out used to allocate gigabytes). A malformed
    // confirmed block still errors so the next route is tried.
    match serde_norway::from_str::<Root>(&yaml) {
        Ok(Root::Mapping(meta)) => {
            let body = std::borrow::Cow::Borrowed(&raw[body_start..]);
            Ok((meta, body))
        }
        Ok(Root::Other) => Ok((Meta::default(), std::borrow::Cow::Borrowed(raw))),
        Err(e) => Err(format!("YAML: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn without_frontmatter() {
        let (m, b) = split("# Hello\ntext").unwrap();
        assert!(m.title.is_none());
        assert!(b.contains("Hello"));
    }

    #[test]
    fn with_frontmatter() {
        let raw = "---\ntitle: Love\nauthor: Me\n---\n# Body\n";
        let (m, b) = split(raw).unwrap();
        assert_eq!(m.title.as_deref(), Some("Love"));
        assert!(b.contains("Body"));
    }

    #[test]
    fn broken_errors() {
        // Closer present + `:` + malformed YAML => error (next route tried).
        assert!(split("---\ntitle: [unclosed\n---\n# Body").is_err());
        // No closer: a leading `---` is a thematic break, never an error.
        let (m, b) = split("---\ntitle: x\n# no closer").unwrap();
        assert!(m.title.is_none());
        assert!(b.contains("title: x"));
    }

    #[test]
    fn closerless_block_is_content_even_with_colon() {
        // BUG: this used to be rejected as broken frontmatter, killing a
        // valid CommonMark document (HR + paragraph).
        let raw = "---\n\nRemember: it is https only\n";
        let (m, b) = split(raw).unwrap();
        assert!(m.title.is_none());
        assert_eq!(b, raw);
    }

    #[test]
    fn non_mapping_block_is_content() {
        // Closer but no `:`: NOT metadata; the viewer must render it.
        let raw = "---\nhello\n---\n# Body\n";
        let (m, b) = split(raw).unwrap();
        assert!(m.title.is_none());
        assert_eq!(b, raw, "content must not be swallowed");
    }

    #[test]
    fn non_mapping_roots_with_colon_are_content() {
        // A YAML SEQUENCE (even one with `:` inside) is not a mapping:
        // it must render as content, never fail the whole route.
        let raw = "---\n- a: b\n- c: d\n---\n# Body\n";
        let (m, b) = split(raw).unwrap();
        assert!(m.title.is_none());
        assert_eq!(b, raw, "sequence root stays content");

        // A quoted scalar containing `:` is a scalar, not a mapping.
        let raw = "---\n\"hello: world\"\n---\n# Body\n";
        let (m, b) = split(raw).unwrap();
        assert!(m.title.is_none());
        assert_eq!(b, raw, "scalar root stays content");

        // A comment-only block with `:` in the comment parses to null.
        let raw = "---\n# see: x\n---\n# Body\n";
        let (m, b) = split(raw).unwrap();
        assert!(m.title.is_none());
        assert_eq!(b, raw, "comment-only root stays content");

        // An explicit `null` root is not a mapping either.
        let raw = "---\nnull\n---\n# Body\n";
        let (m, b) = split(raw).unwrap();
        assert!(m.title.is_none());
        assert_eq!(b, raw, "null root stays content");
    }

    #[test]
    fn mapping_with_colonless_content_still_parses() {
        // Real frontmatter (mapping root) keeps working after the root check.
        let raw = "---\ntitle: Love\ntags:\n  - a\n---\n# Body\n";
        let (m, b) = split(raw).unwrap();
        assert_eq!(m.title.as_deref(), Some("Love"));
        assert!(b.contains("Body"));
    }

    #[test]
    fn oversized_frontmatter_errors() {
        let big = "a".repeat(crate::config::MAX_FRONTMATTER_BYTES + 1);
        let raw = format!("---\ntitle: {big}\n---\n# Body\n");
        assert!(split(&raw).is_err());
    }

    #[test]
    fn huge_closerless_scan_stays_bounded() {
        // ~1 MiB of YAML-looking text with no closer: must return the text
        // as content (bounded accumulation, no parse bomb).
        let big = "key: value\n".repeat(100_000);
        let raw = format!("---\n{big}");
        let (m, b) = split(&raw).unwrap();
        assert!(m.title.is_none());
        assert_eq!(b, raw);
    }

    #[test]
    fn crlf_and_bom_tolerated() {
        let (m, b) = split("\u{FEFF}---\r\ntitle: Hi\r\n---\r\n# Body").unwrap();
        assert_eq!(m.title.as_deref(), Some("Hi"));
        assert!(b.contains("Body"));
    }

    /// The old `serde_norway::Value` root check expanded YAML aliases while
    /// materializing the tree: a 64 KiB fan-out allocated gigabytes. Unknown
    /// fields are now skipped with `IgnoredAny`, so aliases never expand.
    #[test]
    fn alias_fan_out_stays_bounded() {
        let seq = "x,".repeat(4000);
        let refs = "*a,".repeat(2000);
        let yaml = format!("a: &a [{seq}]\nb: [{refs}]\ntitle: Safe\n");
        let raw = format!("---\n{yaml}---\n# Body\n");
        assert!(raw.len() < crate::config::MAX_FRONTMATTER_BYTES);
        let (m, b) = split(&raw).expect("alias fan-out must not fail or explode");
        assert_eq!(m.title.as_deref(), Some("Safe"));
        assert!(b.contains("Body"));
    }

    /// `title` as a map or sequence coerces to None instead of rejecting the
    /// whole route (the field is optional).
    #[test]
    fn map_or_seq_title_coerces_to_none() {
        let (m, b) = split("---\ntitle: {a: b}\n---\n# Body").unwrap();
        assert!(m.title.is_none());
        assert!(b.contains("Body"));
        let (m, _) = split("---\ntitle: [a, b]\n---\n# Body").unwrap();
        assert!(m.title.is_none());
        // Scalars keep coercing.
        let (m, _) = split("---\ntitle: 123\n---\n# Body").unwrap();
        assert_eq!(m.title.as_deref(), Some("123"));
        let (m, _) = split("---\ntitle: true\n---\n# Body").unwrap();
        assert_eq!(m.title.as_deref(), Some("true"));
    }

    /// A valid mapping with a COLLECTION key (`? [a]`, `{a: b}: v`) is still
    /// frontmatter: the contract reserves `Err` for malformed YAML, and the
    /// old `next_key::<String>` failed the whole route on these.
    #[test]
    fn collection_keys_stay_frontmatter() {
        let (m, b) = split("---\n? [a]\n: b\ntitle: Real\n---\n# Body\n").unwrap();
        assert_eq!(m.title.as_deref(), Some("Real"));
        assert!(b.contains("Body"));
        let (m, b) = split("---\n{a: b}: v\ntitle: Real\n---\n# Body\n").unwrap();
        assert_eq!(m.title.as_deref(), Some("Real"));
        assert!(b.contains("Body"));
        // A collection key alone is still a mapping (not content).
        let (m, b) = split("---\n? [a]\n: b\n---\n# Body\n").unwrap();
        assert!(m.title.is_none());
        assert!(b.contains("Body"));
    }

    /// A `---`/`...` INSIDE an indented YAML scalar is content, not a closer:
    /// the metadata must keep parsing past it.
    #[test]
    fn indented_delimiter_does_not_close_early() {
        let raw =
            "---\nsummary: |\n  first\n  ---\n  title: not-metadata\ntitle: Real\n---\n# Body\n";
        let (m, b) = split(raw).unwrap();
        assert_eq!(m.title.as_deref(), Some("Real"));
        assert!(b.contains("Body"));
    }
}
