# Rendering

What MDowser renders and how Markdown maps to the screen. Everything here is
user-visible behavior; the parser pipeline, caches and heuristics live in
[`ARCHITECTURE.md`](ARCHITECTURE.md), and the exact caps and SVG hardening
contract in [`SECURITY.md`](SECURITY.md).

## Markdown

- CommonMark plus GFM-style tables, links, `[^N]` footnotes, `[ ]`/`[x]`
  tasklists and strikethrough.
- Optional YAML frontmatter (`--- ... ---`); a YAML parse error moves to the
  next route. A leading `---` with no closer — or a delimited block that parses
  as YAML but is not a mapping — is CommonMark content (thematic break plus
  text), not an error. `title` goes to the OS window only. Without a frontmatter
  `title`, the first `#` heading names the tab, OS window, page-info card and
  default bookmark/history entry (still rendered as content); the schemeless URL
  is the last resort.
- `+++ ... +++` TOML-style metadata blocks parse (the parser option is on) but
  are never rendered or copied: invisible.
- `:::` and `!!!` admonitions render as alert-blockquote callouts (`> [!NOTE]`).
  The `[!TYPE]` marker is never painted or copied, at any quote depth
  (`> > [!WARNING]` included); unknown types stay literal and code fences are
  untouched. An escaped marker (`> \[!NOTE]`) still folds into a badge; an
  inline code span keeps it literal. Structural `:::` containers
  (`:::tabs`/`:::tab`, `:::code-group`, `:::steps`/`:::step`, `:::card`,
  `:::accordion`/`:::details`) render as the JSX natives below (tab/step labels
  via bare text or `label=`/`title=`, cards via `title=`/`href=`); unknown
  names, missing closers and fenced code stay literal.
- `:shortcode:` emoji expand in text (`:rocket:` -> 🚀), never in code spans or
  blocks. An escaped `\:rocket:` still expands (the pass runs on parsed text).
  Use an inline code span to show one literally.
- HTML comments (`<!-- ... -->`) are invisible, block and inline: they never
  paint and `Copy` omits them (an `<!-- agent-instructions -->` note
  disappears). See [MDX and raw tags](#mdx-and-raw-tags) for tags.

## MDX and raw tags

MDX is Markdown plus JSX. MDowser has no JavaScript, so it recognizes the
STRUCTURE and never evaluates it:

- Callout components render as `> [!NOTE]` callouts: `<Note>`, `<Info>`,
  `<Tip>`, `<Check>`, `<Warning>`, `<Important>`, `<Caution>`, `<Danger>`,
  `<Success>`, `<Hint>`, `<Attention>`, `<Error>`, plus the `type`/`title`
  containers `<Callout>`, `<Alert>`, `<Aside>`, `<Admonition>`. The open tag
  starts its own line, `</Name>` closes the body; a nested callout with a
  DIFFERENT name renders as a nested alert, a same-name one closes the outer
  body first.
- `<Tabs>`/`<Tab>` render as a tab strip (labels over a full-width rule, the
  active one underlined in the accent, only the selected panel laid out, the
  choice remembered per document and tab set); `<Accordion>` as a collapsible
  section (collapsed by default, toggled by title, each section keeping its own
  state, so two same-titled or untitled sections — both shown as `Details` —
  never toggle each other); `<Card>` as a bordered box (`title` first line,
  linked with `href`); `<Steps>`/`<Step>` as numbered rows; `<CodeGroup>` as one
  code tab per fenced child (label from the fence `title=`, else its language).
- `<Tabs items={["A", "B"]}>` labels its `<Tab>` children with no
  `label`/`title`, so tab sets relying on it show real names instead of
  `Tab 1…N`. A very long label (tab cell, accordion header, step title) is
  elided, full text in the hover tooltip.
- `<CardGroup>`, `<Frame>`, `<Tiles>` and `<AccordionGroup>` are transparent
  wrappers. Tab and accordion controls keep a fixed size in every state, so
  hovering never shifts the page. `Copy` includes every panel's text, selected
  or not. Components count only at column 0; an indented one (in a list item or
  quote) is stripped transparently, its tags hidden and content kept.
- The `:::` equivalents render identically: `:::tabs`/`:::tab`, `:::code-group`,
  `:::steps`/`:::step`, `:::card` and `:::accordion`/`:::details` (see
  [Markdown](#markdown)). They too count only at column 0, so an indented one
  stays literal.
- `<Badge>text</Badge>` (inline) is a small pill: faint accent tint, accent ink,
  hairline border, readable in both themes. `type="…"`
  (`info`/`success`/`warning`/`danger`) picks the alert accent; anything else is
  a neutral chrome pill. Inside a heading it scales with the heading (capped at
  H3) without flattening into plain text.
- Every other tag — JSX or HTML (`<div>`, `<a>`, `<h1>`, `<details>`, …) — is a
  transparent wrapper: the tag disappears, its content renders normally. Inline
  `<br>` stays a break and `<img src>` becomes a Markdown image.
- In an MDX document (`import`/`export`, or any capitalized component) ESM
  statements and `{…}` expressions are removed. Heading attributes (`{#id}`) are
  kept; `[#id]` (`## Title [#custom-id]`) becomes `{#custom-id}`, so the marker
  never shows and the id is the heading anchor.
- `<script>`/`<style>` regions are dropped; `<pre>`/`<textarea>` stay inert
  verbatim (their bytes are not Markdown). Nothing is executed, imported or
  fetched from a component.
- Code spans and fenced/indented code are never touched; unbalanced or
  unrecognized markup stays literal, like the `:::` admonitions.

## Links

- Bare URLs linkify like Markdown links. `www.celticos.com` and bare domains
  (`celticos.com`, `scholar-sidekick.com`, `agentready.md`) get `https://`
  inserted; `https://example.com/docs` stays as written; a bare domain with a
  port (`example.com:8080/x`) resolves as `https://` too; local `file:` URLs
  (`file:///Users/me/notes.md`, `file://localhost/…`) linkify as local files.
  Single-label `host:port` sugar resolves DESTINATIONS such as
  `[notes](localhost:3000)`; in prose a candidate needs a dotted domain, so
  `localhost:3000` stays text.
- No other scheme linkifies: `http:`, `mailto:`, `ftp:` and `data:` stay plain
  text (the browser only navigates `https` and local files).
- Recognition starts at a delimiter boundary (a run glued to a link's closing
  `)` or a code span does not qualify) and leaves trailing punctuation and
  unmatched parentheses outside the link.
- A bare domain must end in a real IANA root-zone TLD, so `Cargo.toml`,
  `notes.txt`, `foo.local` and `v1.2` stay text. When the TLD is also a common
  file extension (`md`, `rs`, `sh`, `py`, `pl`, `ai`, `zip`, ...), the domain
  label before it must not be a known file name, so `README.md`, `config.rs`,
  `blog.readme.md`, `script.pl` and `logo.ai` stay text while real hosts
  (`agentready.md`, `mkplandeki.pl`, `readme.blog.md`) link. Emails never link:
  a domain followed by `@` stays text in any form (`www.`/`https://` included).
  Code spans and blocks, existing links and image alt text are never touched.
- Relative/partial links are absolutized before rendering; absolute ones stay.
  Clicks navigate internally, never open an external browser. A local document
  resolves relative links and images against its own directory; a remote
  document can never link to or load a local file.
- In-document anchors navigate: `[text](#heading)` scrolls to the heading whose
  slug (or explicit `{#id}` / `[#id]` marker) matches; `[^N]` jumps to its
  footnote. A missing anchor renders as plain text. A
  `#fragment` on another document survives the load and scrolls when resolved.
  A heading that slugs to nothing (punctuation/emoji only) reserves a
  positional `heading-N` id, so `#heading-1` and its document-outline row still
  reach it.

## Footnotes

`[^label]` renders as a raised marker and a numbered note, never as the source
spelling:

- A **reference** is a small raised marker with its NUMBER (a superscript)
  linked to its definition. Numbers follow FIRST REFERENCE order, so `[^note]`
  before `[^2]` renders as 1 and 2; a repeat keeps its number. A label with no
  definition stays plain text.
- **Definitions** form one footnotes section: a single 1px rule before the FIRST
  definition (not one per note), then a numbered entry per definition (`1.`,
  `2.`, ... matching the references). The marker shares the first text line with
  the body (`1. nota`); wrapped lines and continuation blocks hang under the
  text column. A vector back-arrow closes the note TEXT: it flows as the last
  inline box, lands after the final word on the same line and wraps like a word
  when out of room. A last block that cannot flow inline (a code fence, list,
  quote, table, rule, image) puts the arrow on a small right-aligned row
  beneath. A first block that cannot flow inline keeps the marker on its own row
  above the indented block.
- Definitions the document never references still render and continue the
  numbering after the referenced ones.
- `Copy` keeps the source `[^label]` spelling, so a copied page pastes back into
  Markdown with its footnote graph intact.

## Styling

README styling: a neutral palette (light/dark), H1/H2 bottom rules, muted H6 and
quotes, borderless code cards (a `pre` recipe: muted surface, no border) at the
BODY type size (code exactly as wide as the prose) with syntax colors, striped
bordered tables with collapsed interior row/column separators, 4px `<hr>`,
callout accents and a soft text selection tint. Inside a callout or quote,
paragraph air goes BETWEEN blocks, not after the last, so the box hugs its
text. The document closes with a `theme::spacing::PAGE_END` (24px) blank
pad under its last block, so a page scrolled to its end keeps a margin.

Body text paints the readable `on_surface` token (light `#1f2328` / dark
`#f0f6fc`). The muted tone is opt-in (`theme::weak`, plain quote bodies, H6 —
list markers included, following the scope's text override), so a quote recedes
beside high-contrast neighbors.

Proportional text (chrome, menus, body) draws the bundled Noto Sans variable
font; code draws the bundled Noto Sans Mono CJK JP cut (face 5 of the CJK
collection), the monospace family's primary face, so code keeps true monospace
alignment with no extra asset. Bold runs (`**strong**`), headings, callout
titles and footnote refs use a REAL bold face: the same Noto Sans font
registered again pinned to `wght = 700` behind a named family (`theme::strong`).
Monospace text (inline code pills, code cards) keeps one weight.

The full typography and layout contract — headings, lists, inline code pills,
code cards, tables, quotes — is under "Reading surface" in
[`INTERFACE.md`](INTERFACE.md#reading-surface).

## Emoji and fonts

- Document emoji render in COLOR. Noto Color Emoji (the ~24 MB SVG-in-OpenType
  face) is shaped, extracted and rasterized at app level into bounded inline
  textures. The monochrome Noto Emoji face remains the fallback and the only
  emoji source for SVG text and code spans, which stay monochrome. Interface
  icons are a separate monochrome vector set (`ui::icons`): toolbar, tab rail,
  omnibox and placeholders are hand-drawn line glyphs tinted with the theme, so
  the chrome follows the palette and stays crisp at any zoom while the document
  keeps color emoji.
- Every glyph paints through the bundled Noto faces only. Noto Sans leads
  proportional text (chrome, menus, documents, SVG text); its true-monospace cut
  (`noto-cjk-mono`, the Noto Sans Mono CJK JP face) leads code. The fallback
  chain adds the rest of Noto Sans CJK (the ~19 MB TTC with Chinese, Japanese
  and Korean, kana/Hangul included) and one Noto face per major non-CJK script
  (Arabic, Hebrew, the Indic scripts, Thai, Lao, Khmer, Myanmar, Ethiopic,
  Armenian, Georgian, Thaana, N'Ko, Cherokee, Canadian Aboriginal, Symbols 2),
  so non-English text renders without tofu. No system font and none of epaint's
  default faces is registered.
- Right-to-left runs are reordered before layout: base direction, visual run
  order, word order and mirrored punctuation are resolved; RTL paragraphs lay
  out from the right. Content split across inline elements (a link inside an
  Arabic sentence) is the remaining approximation. Whole-document copy stays
  logical; a native drag selection of a reordered line copies the displayed
  (visual-order) text.

## Layout heuristics

- Accessibility skip links leaked by `.md` variants (`[Skip to
  content](#main-content)`) are dropped instead of becoming the first page
  line. Only the FIRST block qualifies, and only as a link to a fragment, so a
  plain paragraph with the same words is never swallowed.
- A paragraph of only long links (a site card list flattened by an exporter)
  stacks one link per line; short runs (`Docs Chat Forum`) stay inline.
- One-line pipe/dot menus (`Uno | Dos | Tres`, `Uno · Dos · Tres`,
  `Home • Docs`) render as a centered menu row with muted separators — never a
  plain paragraph, never a table (tables arrive as `Tag::Table`). Both separator
  families keep a prose guard: a menu needs a link or a label starting
  uppercase/digit, so `pan | vino | queso` and `término · definición` stay
  paragraphs.

## Images

Images (`![alt](url)`) are absolutized and fetched in the background (from disk
for a local document). A freshly fetched page waits for its images (bounded by
`config::DOC_IMAGE_GATE_SECS`, 5s) before replacing the page, so images never
pop in one by one and shift the text.

- Markdown carries no width/height, so raster images render at natural pixel
  size, never upscaled; anything wider than the reading column scales down to
  the column width (aspect kept).
- Inline SVG icons/logos clamp to an icon-sized box (32px tall).
- A paragraph holding only an image renders it centered as a block (inside
  quotes and list items too). Images sharing a line with text stay inline only
  while small (icons, badges); taller or wider ones are promoted to centered
  blocks, so they never open a blank line around the text. Inline images keep a
  few px of breathing room on both sides. An image inside a link stays
  clickable.
- A run of 2+ consecutive images with at least one block-sized image (a site
  photo strip) renders as ONE centered, wrapping gallery row in source order,
  so entries never move while textures resolve.
- A light/dark logo pair shipped back to back (`logo-light.svg` +
  `logo-dark.svg`) collapses to the variant matching the theme (only a single
  differing `light`/`dark` URL token pairs), so a card shows one logo.
- Loading and failed images show a compact placeholder with the alt text.
  Non-image responses, non-`https` destinations and undecodable bytes never
  render as content.

## Accepted image formats

PNG, JPEG (`.jpg`/`.jpeg`), GIF, WebP, JPEG XL, AVIF, ICO and static SVG,
fetched as HTTPS subresources when the server declares an image type (checked
before the body is read; a generic `application/octet-stream` or a missing
header falls to the magic-byte sniff). Decoded on background threads in pure
Rust (no C libraries) with size, pixel and VRAM caps; a job past its deadline
stops counting as pending. SVG rasterizes statically with scripts,
`<foreignObject>`, every `href` (network, local and `data:` URLs),
`marker`/`pattern`/SMIL (`animate`, `animateTransform`, `animateMotion`, `set`,
`discard`) and DTD markup disabled, bundled fonts only; `filter` markup is
ignored (definitions and references stripped, so a shadowed badge renders flat,
and CSS-spelled filters are rejected). HTML is never interpreted; GIF/JXL
animations show their first frame. A transient image failure gets one automatic
retry.

Exact caps and the SVG hardening contract are in
[`SECURITY.md`](SECURITY.md#scope).
