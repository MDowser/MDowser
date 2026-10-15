# Rendering

What MDowser renders, and how Markdown maps to the screen. Everything here
is user-visible behavior; the parser pipeline, caches and heuristics
internals live in [`ARCHITECTURE.md`](ARCHITECTURE.md).

## Markdown

- CommonMark plus GFM-style tables, links, `[^N]` footnotes, `[ ]`/`[x]`
  tasklists and strikethrough.
- Optional YAML frontmatter (`--- ... ---`). Malformed YAML (a parse error)
  moves to the next route. A leading `---` without a closing delimiter — or
  a delimited block that parses as YAML but is not a mapping — is CommonMark
  content (thematic break + text), never an error. The `title` goes to the
  OS window, not the content. When there is no frontmatter `title`, the
  document's first `#` heading names the tab, the OS window, the page-info
  card and the default bookmark/history entry (the heading still renders as
  content; the schemeless URL is the last resort).
- `+++ ... +++` TOML-style metadata blocks parse (the parser option is
  on) and are never rendered or copied: they are invisible, like YAML
  frontmatter that survived the split.
- `:::` and `!!!` admonitions render as the same callouts as alert
  blockquotes (`> [!NOTE]`); the `[!TYPE]` marker is never painted and never
  copied, at any quote depth (`> > [!WARNING]` included), while unknown types
  stay literal and code fences are
  never touched. An ESCAPED marker (`> \[!NOTE]`) still folds into a badge
  for the same reason as the `:shortcode:` expansion below (it is matched on
  the rendered text); an inline code span keeps it literal. Structural `:::`
  custom
  containers (`:::tabs`/`:::tab`,
  `:::code-group`, `:::steps`/`:::step`, `:::card`,
  `:::accordion`/`:::details`) render as the same natives as their JSX
  equivalents below (tab/step labels via bare text or `label=`/`title=`,
  cards via `title=`/`href=`); unknown names, missing closers and fenced
  code stay literal.
- `:shortcode:` emoji are expanded in text (`:rocket:` -> 🚀), never
  inside code spans/blocks. An ESCAPED `\:rocket:` still expands: the
  shortcode pass runs on the parsed text, where CommonMark has already
  resolved the backslash. An inline code span (`` `:rocket:` ``) is the way
  to show a shortcode literally.
- HTML comments (`<!-- ... -->`) are invisible, block and inline, exactly
  like in a browser: they never paint and `Copy` never includes them (an
  `<!-- agent-instructions -->` note disappears). See
  [MDX and raw tags](#mdx-and-raw-tags) for how tags themselves render.

## MDX and raw tags

MDX is Markdown plus JSX; MDowser has no JavaScript, so it recognizes the
STRUCTURE and never evaluates it:

- Callout components render as the same callouts as `> [!NOTE]`:
  `<Note>`, `<Info>`, `<Tip>`, `<Check>`, `<Warning>`, `<Important>`,
  `<Caution>`, `<Danger>`, `<Success>`, `<Hint>`, `<Attention>`, `<Error>`,
  and the `type`/`title` containers `<Callout>`,
  `<Alert>`, `<Aside>`, `<Admonition>`. The open tag sits at the start of
  its own line and the matching `</Name>` closes the body (a nested callout
  with a DIFFERENT name renders as a nested alert; a same-name one closes
  the outer body first, so it reads as a wrapper).
- Structural components render NATIVELY: `<Tabs>`/`<Tab>` as a tab strip
  (labels over a full-width rule, the active one underlined in the accent —
  not filled buttons — and only the selected panel is laid out; the choice is
  remembered per document and per tab set), `<Accordion>` as a collapsible
  section (collapsed by default, toggled by its title; each section keeps its
  OWN open/closed state, so two sections with the same title — or two
  untitled ones, both shown as `Details` — never toggle each other), `<Card>`
  as a bordered
  box (a `title` becomes the first line — a link when `href` is set),
  `<Steps>`/`<Step>` as numbered rows and `<CodeGroup>` as one code tab
  per fenced child (the label is the fence `title=`, else its language).
  `<Tabs items={["A", "B"]}>` labels its `<Tab>` children that carry no
  `label`/`title` (a common docs-generator convention), so tab
  sets that rely on it show their real names instead of `Tab 1…N`.
  A very long label (tab cell, accordion header, step title) is elided
  (with the full text in the hover tooltip): one 2 000-char label used to
  push every later tab outside the reading sheet.
  `<CardGroup>`, `<Frame>`, `<Tiles>` and `<AccordionGroup>` are transparent wrappers.
  Tab and accordion
  controls keep a fixed size in every state, so hovering never shifts the
  page. `Copy` still includes every panel's text, selected or not.
  Components are recognized at column 0; an indented one (inside a list item
  or quote) is just stripped transparently so its tags never show and its
  content stays in place.
- The `:::` custom-container equivalents render identically:
  `:::tabs`/`:::tab`, `:::code-group`, `:::steps`/`:::step`, `:::card` and
  `:::accordion`/`:::details` (see [Markdown](#markdown)). Like the JSX
  openers they only count at column 0, so an indented one stays literal
  instead of breaking out of its list item or quote.
- `<Badge>text</Badge>` (inline) renders as a small pill: a faint tint of the
  accent with the accent as ink and a hairline border (readable in both
  themes). `type="…"` (`info`/`success`/`warning`/`danger`) picks the alert
  accent, anything else is a neutral chrome pill. Inside a heading the same
  pill is drawn, scaled with the heading (capped at the H3 step) instead of
  flattening into plain heading text.
- Every other tag — JSX component or plain HTML (`<div>`, `<a>`, `<h1>`,
  `<details>`, …) — is a TRANSPARENT wrapper: the tag disappears and its
  content renders normally. Inline `<br>` stays a break and `<img src>`
  becomes a Markdown image (so a badge wrapped in HTML still loads).
- In an MDX document (`import`/`export`, or any capitalized component) ESM
  statements and `{…}` expressions are removed, since they cannot be
  evaluated without JavaScript; heading attributes (`{#id}`) are kept, and
  the `[#id]` heading-anchor convention (`## Title [#custom-id]`) is
  rewritten to `{#custom-id}` so the marker never shows and the id is the
  heading's anchor.
- `<script>`/`<style>` regions are dropped; `<pre>`/`<textarea>` stay inert
  verbatim (their bytes are not Markdown). HTML comments are invisible.
  No JavaScript, import or expression is ever executed, and nothing is
  fetched from a component.
- Code spans and fenced/indented code are never touched, and unbalanced or
  unrecognized markup stays literal, exactly like the `:::` admonitions.

## Links

- Bare URLs linkify like markdown links: `www.celticos.com` and bare domains
  (`celticos.com`, `scholar-sidekick.com`, `agentready.md`) get `https://`
  inserted, `https://example.com/docs` stays as written, a `host:port`
  spelling the omnibox accepts (`localhost:3000`, `example.com:8080/x`)
  resolves as `https://` too, and local `file:`
  URLs (`file:///Users/me/notes.md`, `file://localhost/…`) linkify as
  local files. No other scheme does: `http:`, `mailto:`, `ftp:` and `data:`
  stay plain text (the browser only navigates https and local files).
  Recognition starts at a delimiter boundary (a run glued to a link's closing
  `)` or a code span does not qualify) and leaves trailing punctuation
  and unmatched parentheses outside the link. A bare domain must end in a
  real IANA root-zone TLD, so `Cargo.toml`, `notes.txt`, `foo.local` and
  `v1.2` stay text; when the TLD is also a common file extension (`md`,
  `rs`, `sh`, `py`, `pl`, `ai`, `zip`, ...), the domain label immediately
  before the TLD must not be a known file name, so `README.md`,
  `config.rs`, `blog.readme.md`, `script.pl` and `logo.ai` stay text while
  real hosts (`agentready.md`, `mkplandeki.pl`, `readme.blog.md`) link.
  Emails never link: a domain
  followed by `@` stays text in every candidate form (`www.`/`https://`
  included). Code spans and
  blocks, existing links and image alt text are never touched.
- Relative/partial links are absolutized before rendering; absolute ones are
  kept. Clicks navigate internally (never open an external browser). A local
  document resolves its relative links and images against its own directory;
  a remote document can never link to or load a local file.
- In-document anchors navigate: `[text](#heading)` scrolls to the heading
  whose slug (or explicit `{#id}` / `[#id]` heading marker) matches, and
  `[^N]` jumps to
  its footnote. A link whose anchor does not exist in the document renders
  as plain text instead of a dead blue link. A `#fragment` on another
  document survives the load and scrolls once it resolves.

## Footnotes

`[^label]` renders as a raised marker and a numbered note
(`markdown::anchors` + `viewer::markdown`), never as the source spelling:

- A **reference** is a small raised marker showing its NUMBER (a
  superscript), linked to its definition. Numbers follow FIRST REFERENCE
  order, so `[^note]` before `[^2]` renders as 1 and 2, and a repeated
  reference keeps its number. A label with no definition stays plain text.
- **Definitions** render as one footnotes section: a single 1px rule before
  the FIRST definition (not one per note), then a numbered entry per
  definition (`1.`, `2.`, ... matching the reference numbers): the marker
  shares the first text line with the note body (`1. nota`), wrapped lines
  and continuation blocks hang under the text column, and a small vector
  back-arrow that scrolls to the reference it belongs to closes the note
  TEXT: it flows as the note's last inline box, so it lands right after the
  final word on the same line (and wraps like a word when there is no room
  left). A note whose last block cannot flow inline (a code fence, list,
  quote, table, rule, image) puts the arrow on a small right-aligned row
  under it instead, so it is never lost. A
  definition whose first block cannot flow inline (a code fence, list,
  quote, table, rule, image) keeps the marker on its own row above the
  indented block, exactly like a list item.
- Definitions the document never references still render (common
  renderers drop them) and continue the numbering after the referenced ones, so nothing the
  author wrote is hidden.
- `Copy` keeps the source `[^label]` spelling — like an autolinked URL,
  whose copy text is the URL the document wrote — so a copied page pastes
  back into Markdown with its footnote graph intact.

## Styling

README styling: a neutral palette (light/dark), H1/H2 bottom rules, muted
H6 and quotes, borderless code cards (a `pre` recipe: muted surface, no
border) at the BODY type size (code is exactly as wide as the prose around
it) with syntax colors, striped bordered tables with collapsed interior
row/column separators, 4px `<hr>`, callout accents, and a soft text
selection tint.
Inside a callout or quote, paragraph air goes BETWEEN blocks, not after the
last one, so the box's bottom edge hugs its text.
The document itself closes with a `theme::spacing::PAGE_END` (24px) blank
pad under its last block, so a page scrolled to its end keeps a small
margin under the text instead of ending flush against the sheet's bottom
edge.

Body text paints the readable `on_surface` token (light `#1f2328` / dark
`#f0f6fc`); the muted tone is opt-in (`theme::weak`, plain quote bodies,
H6 — list markers included, which follow the enclosing scope's text
override), so a quote visibly recedes while its neighbors stay high-contrast.
Proportional text (chrome, menus, body) draws the bundled Noto Sans
variable font, the primary face of that family; code draws the bundled
Noto Sans Mono CJK JP cut (face 5 of the CJK collection), the primary face
of the monospace family, so code keeps true monospace alignment with no
extra asset. Bold runs (`**strong**`), headings, callout
titles and footnote refs use a REAL bold face: the same Noto Sans font is
registered a second time pinned to `wght = 700` (`FontTweak::coords`)
behind a named family (`theme::strong`), because epaint's `FontId` carries
no weight axis. Monospace text (inline code pills, code cards) keeps one
weight.

The full typography and layout contract — headings, lists, inline code
pills, code cards, tables, quotes — is under "Reading surface" in
[`INTERFACE.md`](INTERFACE.md).

## Emoji and fonts

- Document emoji render in COLOR: Noto Color Emoji (the ~24 MB
  SVG-in-OpenType face) is shaped, extracted and rasterized at
  app level (`media::emoji`) into bounded inline textures, because epaint's
  text stack only paints monochrome outlines. The monochrome Noto Emoji
  face remains the fallback (and the only emoji source for SVG text and code
  spans, which stay monochrome). The interface icons are a separate
  monochrome vector set (`ui::icons`): toolbar, tab rail, omnibox and
  placeholders are hand-drawn line glyphs tinted with the theme, so the
  chrome follows the palette and stays crisp at any zoom while the
  document keeps its color emoji.
- Every glyph paints through the bundled Noto faces only: Noto Sans leads
  proportional text (chrome, menus, documents and SVG text) and its
  true-monospace cut (`noto-cjk-mono`, the Noto Sans Mono CJK JP face of
  the CJK collection) leads code; the fallback chain adds the rest of
  Noto Sans CJK (the ~19 MB TTC with
  Chinese, Japanese and Korean, kana/Hangul included) and one Noto face
  per major non-CJK script (Arabic, Hebrew, the Indic scripts, Thai, Lao,
  Khmer, Myanmar, Ethiopic, Armenian, Georgian, Thaana, N'Ko, Cherokee,
  Canadian Aboriginal, Symbols 2), so non-English documents render
  instead of showing tofu boxes. No system font and none of epaint's
  bundled default faces is ever registered.
- Right-to-left runs are reordered
  before layout (`viewer::bidi`): base direction, visual run order,
  word order and mirrored punctuation are resolved and RTL paragraphs are
  laid out from the right; content split across inline elements (a link
  inside an Arabic sentence) is the remaining approximation. Whole-document
  copy stays logical; a native drag selection of a reordered line copies
  the displayed (visual-order) text.

## Layout heuristics

- Accessibility skip links leaked by `.md` variants (`[Skip to
  content](#main-content)`) are dropped instead of appearing as the first
  page line. Only the document's FIRST block qualifies, and only when it is
  a link to a fragment, so a plain paragraph with the same words is never
  swallowed.
- A paragraph that is only long links (a site card list flattened by an
  exporter) stacks one link per line; short link runs (`Docs Chat Forum`)
  stay inline.
- One-line pipe/dot menus (`Uno | Dos | Tres`, `Uno · Dos · Tres`,
  `Home • Docs`) render as a centered menu row with muted separators —
  never left as a plain paragraph and never mistaken for a table (tables
  arrive as `Tag::Table`). Both separator families keep a prose guard:
  a menu needs a link or a label that starts uppercase/digit, so
  `pan | vino | queso` and `término · definición` stay paragraphs.

## Images

Images (`![alt](url)`) are absolutized too and fetched in the background
(from disk when the document is local). A freshly fetched page waits for
its images (bounded by `config::DOC_IMAGE_GATE_SECS`) before it replaces
the page on screen, so images never pop in one by one and shift the text.

- Markdown carries no width/height, so raster images render at their natural
  pixel size, never upscaled; anything wider than the reading column is
  scaled down to the column width (aspect kept).
- Inline SVG icons/logos are clamped to an icon-sized box (32px tall)
  because their intrinsic size is usually a stylesheet decision.
- A paragraph holding only an image renders it centered as a block (inside
  quotes and list items too). Images sharing a line with text stay inline
  only while small (icons, badges); taller or wider ones are promoted to
  centered blocks so they never open a huge blank line around the
  neighboring text. Inline images keep a few px of breathing room on both
  sides. An image inside a link stays clickable.
- Loading/failed images show a compact placeholder with the alt text.
  Non-image responses, non-https destinations and undecodable bytes never
  render as content.

## Accepted image formats

PNG, JPEG (`.jpg`/`.jpeg`), GIF, WebP, JPEG XL, AVIF, ICO and static SVG,
fetched as HTTPS subresources when the server declares an image type
(checked before the body is read; a generic `application/octet-stream` or a
missing header passes to the magic-byte sniff instead). Decoded on
background threads in pure Rust (no C libraries) with size, pixel and VRAM
caps; a job that exceeds its deadline stops counting as pending. SVG is
rasterized statically with scripts, `<foreignObject>`, every `href` (network,
local and `data:` URLs), `marker`/`pattern`/SMIL (`animate`,
`animateTransform`, `animateMotion`, `set`, `discard`) and DTD markup disabled (bundled
fonts only); SVG `filter` markup is ignored (definitions and references are
stripped, so a badge with a shadow renders flat, and CSS-spelled filters are
rejected); HTML is never interpreted; GIF/JXL animations show their first
frame. A transient image failure gets one automatic retry.

The exact caps and the SVG hardening contract are in
[`SECURITY.md`](SECURITY.md).
