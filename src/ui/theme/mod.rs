// Copyright (c) 2026 MDowser
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `ui::theme`: README-style look for the rendered document.
//!
//! - Palette tokens (a neutral documentation scheme): gray chrome
//!   (#f6f8fa / #151b23), white page (#ffffff / #0d1117), accent-blue links
//!   (#0969da / #4493f8), border gray (#d1d9e0 / #3d444d), muted text
//!   (#59636e / #9198a1); the OS picks light/dark and dark mirrors light.
//! - `apply(ctx)`: airy spacing + full `Visuals` per theme.
//! - Helpers honoring the active theme: `tonal_button_sized` (secondary
//!   container), `primary_button_sized` (accent fill, white label: the one
//!   emphasized action of a surface), `card` (raised surface),
//!   `code_card` (code block), `rule` (document `<hr>`), type scale
//!   (`display`, `weak`) and `scheme_for(ui)`.
//! - Interface icons are monochrome vector strokes (`ui::icons`): one
//!   24x24 design grid, tinted with the active theme, with a subtle
//!   hover/press animation in the buttons. Document emoji still come from
//!   the color pipeline below.
//! - Bundled Noto typefaces only: Noto Sans (`assets::NOTO_SANS`) for
//!   proportional text (chrome, menus, documents) and its true-monospace
//!   cut (`noto-cjk-mono`, face 5 of the CJK collection) for code; the
//!   rest of the embedded faces follow only as glyph fallbacks: complete
//!   monochrome emoji, CJK (Chinese/Japanese/Korean), extended
//!   Latin/Vietnamese and the major non-CJK scripts (Arabic, Hebrew,
//!   Indic, Southeast Asian, ...). The framework's own bundled fallback
//!   faces are never registered. Text emoji render monochrome (epaint has
//!   no color-font support); the viewer paints color textures through
//!   `media::emoji`. RTL runs get display-order
//!   reordering from `viewer::bidi`.
//!
//! VERIFIED AGAINST egui 0.36 SOURCE (past bugs, do not regress):
//! - `TextEdit` background comes from `Visuals::text_edit_bg_color`, which
//!   falls back to `extreme_bg_color`. It MUST be an input color (white in
//!   light), never `on_surface` (that painted black boxes).
//! - `TextEdit` default align is `LEFT_TOP`: tall single-line fields need
//!   `.vertical_align(Center)` or text sticks to the top.
//! - `hovered.expansion` must be 0: any growth shifts neighbors on hover.
//! - Every `WidgetVisuals` state must keep the SAME `bg_stroke.width`:
//!   `Frame::total_margin` adds the stroke width back to the inner margin,
//!   but a `small` `Button` zeroes its vertical padding AFTER the `- width`
//!   subtraction, so a 0-width active stroke allocated 2px LESS than the
//!   idle state and made the pressed (or keyboard-focused) button shrink,
//!   shifting its neighbors. NO state uses `Stroke::NONE` for `bg_stroke`;
//!   a transparent 1px stroke paints nothing.
//! - `WidgetVisuals` uses `corner_radius`, not `rounding`.
//! - `Painter::rect` takes a 5th `StrokeKind` argument.

use eframe::egui;

/// Document palette (subset used by the app).
#[derive(Debug, Clone, Copy)]
pub struct Scheme {
    /// Main accent (links, focus outlines, toggles): `#0969da`.
    pub primary: egui::Color32,
    /// Primary container (text selection): light blue wash.
    pub primary_container: egui::Color32,
    /// Secondary container (tonal buttons): button gray.
    pub secondary_container: egui::Color32,
    /// Text on `secondary_container`.
    pub on_secondary_container: egui::Color32,
    /// Base page background (pure white in light).
    pub surface: egui::Color32,
    /// Subtle surface (chrome, code cards, table headers, quotes).
    pub container_low: egui::Color32,
    /// Mid variant (separator/heading-rule tone).
    pub container: egui::Color32,
    /// High variant (resting buttons).
    pub container_high: egui::Color32,
    /// Inline code pill background (`rgba(175,184,193,0.2)` in light).
    pub code_bg: egui::Color32,
    /// Input field background (MUST stay white in light).
    pub input_bg: egui::Color32,
    /// Omnibox pill at rest (toolbar address bar): soft chrome gray.
    pub omnibox_bg: egui::Color32,
    /// Omnibox pill hover: one step darker than `omnibox_bg`.
    pub omnibox_hover: egui::Color32,
    /// Hover fill of neutral chrome controls (button ramp). Single token so
    /// the omnibox pill and the widget ramp cannot drift apart.
    pub container_hover: egui::Color32,
    /// Pressed/emphasized fill of neutral chrome controls.
    pub container_active: egui::Color32,
    /// Main text.
    pub on_surface: egui::Color32,
    /// Secondary/muted text (quotes, h6, captions).
    pub on_surface_variant: egui::Color32,
    /// Default border (tables, H1/H2 rule, `<hr>`, code cards).
    pub outline_variant: egui::Color32,
    /// Error (error titles, CAUTION accents).
    pub error: egui::Color32,
    /// Token accents: keyword (red), string (navy), comment (gray),
    /// number (blue), function (purple).
    pub code_keyword: egui::Color32,
    pub code_string: egui::Color32,
    pub code_comment: egui::Color32,
    pub code_number: egui::Color32,
    pub code_func: egui::Color32,
    /// Alert accents (NOTE / TIP / IMPORTANT / WARNING / CAUTION).
    pub alert_note: egui::Color32,
    pub alert_tip: egui::Color32,
    pub alert_important: egui::Color32,
    pub alert_warning: egui::Color32,
    pub alert_caution: egui::Color32,
}

/// Light scheme: white page, `#f6f8fa` chrome, accent blue.
///
/// Tokens: default surface #ffffff, muted surface #f6f8fa, default text
/// #1f2328, muted text #59636e, default border #d1d9e0, accent #0969da and
/// the syntax colors below.
pub fn light() -> Scheme {
    Scheme {
        primary: rgb(9, 105, 218),
        primary_container: rgb(182, 215, 255),
        secondary_container: rgb(234, 238, 242),
        on_secondary_container: rgb(31, 35, 40),
        surface: rgb(255, 255, 255),
        container_low: rgb(246, 248, 250),
        container: rgb(234, 238, 242),
        container_high: rgb(234, 238, 242),
        // Flattened `--bgColor-neutral-muted` #818b981f over the white page.
        code_bg: rgb(240, 241, 242),
        input_bg: rgb(255, 255, 255),
        omnibox_bg: rgb(234, 238, 242),
        omnibox_hover: rgb(222, 226, 231),
        container_hover: rgb(222, 226, 231),
        container_active: rgb(206, 212, 218),
        on_surface: rgb(31, 35, 40),
        on_surface_variant: rgb(89, 99, 110),
        outline_variant: rgb(209, 217, 224),
        error: rgb(209, 36, 47),
        code_keyword: rgb(207, 34, 46),
        code_string: rgb(10, 48, 105),
        code_comment: rgb(89, 99, 110),
        code_number: rgb(5, 80, 174),
        code_func: rgb(102, 57, 186),
        alert_note: rgb(9, 105, 218),
        alert_tip: rgb(26, 127, 55),
        alert_important: rgb(130, 80, 223),
        alert_warning: rgb(154, 103, 0),
        alert_caution: rgb(209, 36, 47),
    }
}

/// Dark scheme: `#0d1117` page, `#151b23` chrome, brighter accent blue.
///
/// Tokens: default surface #0d1117, muted surface #151b23, default text
/// #f0f6fc, muted text #9198a1, default border #3d444d, accent #4493f8 and
/// the syntax colors below.
pub fn dark() -> Scheme {
    Scheme {
        primary: rgb(68, 147, 248),
        primary_container: rgb(31, 77, 125),
        secondary_container: rgb(33, 38, 45),
        on_secondary_container: rgb(230, 237, 243),
        surface: rgb(13, 17, 23),
        container_low: rgb(21, 27, 35),
        container: rgb(33, 38, 45),
        container_high: rgb(33, 38, 45),
        // Flattened `--bgColor-neutral-muted` #656c7633 over the dark page.
        code_bg: rgb(31, 35, 42),
        input_bg: rgb(13, 17, 23),
        omnibox_bg: rgb(33, 38, 45),
        omnibox_hover: rgb(45, 51, 59),
        container_hover: rgb(45, 51, 59),
        container_active: rgb(54, 61, 70),
        on_surface: rgb(240, 246, 252),
        on_surface_variant: rgb(145, 152, 161),
        outline_variant: rgb(61, 68, 77),
        error: rgb(248, 81, 73),
        code_keyword: rgb(255, 123, 114),
        code_string: rgb(165, 214, 255),
        code_comment: rgb(145, 152, 161),
        code_number: rgb(121, 192, 255),
        code_func: rgb(210, 168, 255),
        alert_note: rgb(68, 147, 248),
        alert_tip: rgb(63, 185, 80),
        alert_important: rgb(163, 113, 247),
        alert_warning: rgb(210, 153, 34),
        alert_caution: rgb(248, 81, 73),
    }
}

/// Const-compatible shortcut for colors.
const fn rgb(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

/// Scheme for the active `Ui` theme (light/dark from the OS).
///
/// One-off convenience for non-hot callers (dialog setup, tooltips). Hot draw
/// paths must use [`scheme_ref`] instead: it borrows the interned palette
/// instead of copying the 50-field struct per element.
#[allow(dead_code)]
pub fn scheme_for(ui: &egui::Ui) -> Scheme {
    if ui.visuals().dark_mode {
        dark()
    } else {
        light()
    }
}

/// Shared `'static` schemes, built once per process.
///
/// The palette is constant data (no `egui::Context` input). Callers that
/// paint many elements resolve the scheme ONCE per surface with
/// [`scheme_ref`] and pass the reference down instead of rebuilding the
/// 50-field struct per element (see [`scheme_for`] for the one-off copy).
static LIGHT: std::sync::OnceLock<Scheme> = std::sync::OnceLock::new();
static DARK: std::sync::OnceLock<Scheme> = std::sync::OnceLock::new();

/// Borrowed variant of [`scheme_for`]: an interning, allocation-free
/// `&'static Scheme` for the active theme. Use in draw paths.
pub fn scheme_ref(ui: &egui::Ui) -> &'static Scheme {
    if ui.visuals().dark_mode {
        DARK.get_or_init(dark)
    } else {
        LIGHT.get_or_init(light)
    }
}

/// [`scheme_ref`] for callers that have a `Context` but no `Ui` yet
/// (floating windows set their frame and shadow before any `Ui` exists).
/// Reading the interned palette here instead of calling `dark()`/`light()`
/// keeps the "palette interned per theme, never rebuilt per call site"
/// contract true outside the draw path too.
pub fn scheme_ref_ctx(ctx: &egui::Context) -> &'static Scheme {
    if ctx.theme() == egui::Theme::Dark {
        DARK.get_or_init(dark)
    } else {
        LIGHT.get_or_init(light)
    }
}

/// Registers the app's text stack from the bundled Noto faces ONLY:
/// [`crate::assets::NOTO_SANS`] leads the proportional family (every menu,
/// label and document paragraph) and the bundled Noto Sans Mono CJK JP cut
/// (`noto-cjk-mono`, face 5 of the CJK collection) leads the monospace
/// family, so code keeps true monospace alignment. The complete
/// monochrome emoji face (`noto-emoji-full`), CJK (`noto-cjk`) and every
/// script face in [`crate::assets::SCRIPT_FONTS`] follow purely as glyph
/// fallbacks for code points the primaries cannot draw.
///
/// epaint's bundled defaults (its old emoji subset and icon font
/// included) are deliberately NOT registered:
/// `FontDefinitions::empty()` starts a stack with no dependency-shipped
/// face, so nothing outside `assets/` can ever paint.
fn install_fonts(ctx: &egui::Context) {
    /// One face of an embedded font file (`.ttc` collections included).
    fn face(bytes: &'static [u8], index: u32) -> egui::FontData {
        let mut font = egui::FontData::from_static(bytes);
        font.index = index;
        font
    }

    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        "noto-emoji-full".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(crate::assets::NOTO_EMOJI)),
    );
    // 0 = Noto Sans CJK JP, 5 = Noto Sans Mono CJK JP. The collection is
    // embedded once; each registered face only picks its index.
    fonts.font_data.insert(
        "noto-cjk".to_owned(),
        std::sync::Arc::new(face(crate::assets::noto_sans_cjk(), 0)),
    );
    fonts.font_data.insert(
        "noto-cjk-mono".to_owned(),
        std::sync::Arc::new(face(crate::assets::noto_sans_cjk(), 5)),
    );
    fonts.font_data.insert(
        "noto-sans".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(crate::assets::NOTO_SANS)),
    );
    for (name, bytes) in crate::assets::SCRIPT_FONTS {
        fonts.font_data.insert(
            (*name).to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
    }

    // One chain per family: the primary face, then the complete emoji
    // face (real glyphs win over epaint's replacement box), then the
    // remaining faces, in `SCRIPT_FONTS` order.
    //
    // Monospace: the app DOES bundle a mono cut — face 5 of the Noto Sans
    // CJK collection (`noto-cjk-mono`) is Noto Sans Mono CJK JP, whose
    // Latin/Greek/Cyrillic glyphs are true monospace (measured: eight `i`
    // and eight `W` advance identically). It leads the monospace chain so
    // code keeps its alignment; `noto-sans` follows as a fallback for code
    // points the mono cut lacks.
    let mut proportional = vec![
        "noto-sans".to_owned(),
        "noto-emoji-full".to_owned(),
        "noto-cjk".to_owned(),
    ];
    let mut monospace = vec![
        "noto-cjk-mono".to_owned(),
        "noto-emoji-full".to_owned(),
        "noto-sans".to_owned(),
    ];
    for (name, _) in crate::assets::SCRIPT_FONTS {
        proportional.push((*name).to_owned());
        monospace.push((*name).to_owned());
    }
    fonts
        .families
        .insert(egui::FontFamily::Proportional, proportional.clone());
    fonts
        .families
        .insert(egui::FontFamily::Monospace, monospace);

    // A REAL bold face for `**strong**`, headings and footnote refs.
    // epaint 0.36 has no weight axis (`FontId` carries only family + size),
    // so bold is a named FAMILY whose first face is the bundled Noto Sans
    // variable font pinned to `wght = 700` (`FontTweak::coords`, applied by
    // the shaper). The rest of the proportional list is inherited verbatim,
    // so CJK/emoji/script fallbacks keep working (they stay regular: no
    // variable CJK face is bundled). Regular and bold are the SAME face at
    // two weights, so their metrics match and no baseline tweak is needed.
    fonts.font_data.insert(
        "noto-sans-bold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(crate::assets::NOTO_SANS).tweak(
            egui::FontTweak {
                coords: egui::epaint::text::VariationCoords::new([(b"wght", 700.0)]),
                ..Default::default()
            },
        )),
    );
    let mut strong = proportional;
    strong[0] = "noto-sans-bold".to_owned();
    fonts.families.insert(strong_family(), strong);
    ctx.set_fonts(fonts);
}

/// Family name of the bundled bold face (see `install_fonts`).
pub const STRONG_FAMILY: &str = "mdowser-strong";

/// The bold [`egui::FontFamily`], interned (cloning one is an `Arc` bump, so
/// the per-fragment call sites never allocate).
pub fn strong_family() -> egui::FontFamily {
    static FAMILY: std::sync::OnceLock<egui::FontFamily> = std::sync::OnceLock::new();
    FAMILY
        .get_or_init(|| egui::FontFamily::Name(STRONG_FAMILY.into()))
        .clone()
}

/// Applies the bold face to a `RichText`: `.strong()` (which keeps the
/// on-surface color) plus the real bold family. Every document string that
/// means "bold" should go through this instead of a bare `.strong()`.
pub fn strong(rt: egui::RichText) -> egui::RichText {
    rt.strong().family(strong_family())
}

/// Applies airy spacing + palettes to BOTH themes (the OS picks).
/// Starts from the `Visuals::light()/dark()` base (correct text/shadows).
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    for (theme, scheme) in [(egui::Theme::Light, light()), (egui::Theme::Dark, dark())] {
        ctx.style_mut_of(theme, |style| {
            apply_spacing_type(style);
            apply_visuals(style, theme, &scheme);
        });
    }
}

/// Binds the embedded font stack (bold family included) on a bare context
/// without applying the theme's spacing/visuals.
///
/// Production gets this through [`apply`]. Tests that build an
/// `egui::Context::default()` and render `**strong**` text / headings need
/// it: a named family that is not bound panics inside epaint's font lookup.
/// Registers the SAME Noto Sans stack as production, so test metrics match
/// the shipped app.
#[cfg(test)]
pub(crate) fn install_test_fonts(ctx: &egui::Context) {
    install_fonts(ctx);
}

/// Spacing + type scale shared by both themes (no theme branch).
fn apply_spacing_type(style: &mut egui::Style) {
    style.spacing.button_padding = egui::Vec2::new(12.0, 7.0);
    style.spacing.item_spacing = egui::Vec2::new(8.0, 8.0);
    style.spacing.indent = 20.0;

    // Scrollbars: subtle gray handles.
    //
    // `foreground_color = true` picks `widgets.inactive.fg_stroke`, which
    // the ramp below sets to the border tone: the handle used to paint
    // `inactive.bg_fill` (`container_high`) with `foreground_color = false`,
    // the SAME tone as the chrome behind it, so the document bar rendered at
    // ~1.1:1 contrast — a scrollbar the reader could not see. epaint's own
    // default (`fg_stroke` = near-black `on_surface`) is what the old code
    // avoided by painting `bg_fill`; the border tone is the visible middle
    // ground, in both themes.
    style.spacing.scroll.foreground_color = true;
    style.spacing.scroll.bar_width = 10.0;
    style.spacing.scroll.handle_min_length = 24.0;
    style.spacing.scroll.dormant_handle_opacity = 0.75;
    style.spacing.scroll.active_handle_opacity = 0.9;
    style.spacing.scroll.interact_handle_opacity = 1.0;
    // No TRACK: epaint paints a floating bar's track with
    // `visuals.extreme_bg_color` (the white input tone here), which washed
    // a white band over the gray code cards and Preferences surfaces and a
    // darker band in dark mode. The app reserves real lanes for these bars
    // (`theme::scrollbar_lane`), so the handle alone is the whole bar.
    style.spacing.scroll.dormant_background_opacity = 0.0;
    style.spacing.scroll.active_background_opacity = 0.0;
    style.spacing.scroll.interact_background_opacity = 0.0;

    // Type scale: Body 16, code at the SAME 16px (one width for prose and
    // for code: inline pills and code cards measure like the body text),
    // small UI text 12. Document paragraphs/lists/tables use `Body`
    // via plain `Label`s, so this single change shapes the whole
    // reading experience (toolbar zoom still multiplies on top).
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(crate::ui::theme::BODY_FONT_SIZE, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(
            crate::ui::theme::typography::BUTTON,
            FontFamily::Proportional,
        ),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(
            crate::ui::theme::typography::UI_SMALL,
            FontFamily::Proportional,
        ),
    );
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(
            crate::ui::theme::typography::HEADING,
            FontFamily::Proportional,
        ),
    );
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(crate::ui::theme::BODY_FONT_SIZE, FontFamily::Monospace),
    );
}

/// Visuals + widget ramp for one theme (extracted from `apply` so the
/// light/dark branches stay reviewable without a 160-line closure).
fn apply_visuals(style: &mut egui::Style, theme: egui::Theme, scheme: &Scheme) {
    let v = &mut style.visuals;
    v.dark_mode = theme == egui::Theme::Dark;
    // Body text MUST resolve to `on_surface`: `Visuals::text_color()` falls
    // back to `widgets.noninteractive.fg_stroke`, which is the MUTED tone by
    // design (separators, hints). Without this override every plain
    // paragraph/table/list label painted muted `#59636e` while bold runs,
    // headings and bullets painted `#1f2328` — the text a reader actually
    // reads was the low-contrast one, and `**bold**` showed as "different
    // color" instead of heavier (see `strong_family`, which supplies the real
    // bold face). Muted text opts in explicitly (`theme::weak`, quote
    // bodies), so the default stays the readable one.
    v.override_text_color = Some(scheme.on_surface);
    // Quotes paint their own structure: egui's indent guide line would
    // show as a SECOND vertical line next to the quote bar.
    v.indent_has_left_vline = false;
    // Pointing hand on everything clickable (buttons, tabs, links):
    // without this egui keeps the arrow even on a Button.
    v.interact_cursor = Some(egui::CursorIcon::PointingHand);
    // Floating surfaces (tooltips, context menu, toasts, ComboBox
    // dropdowns) use `window_fill` + `window_stroke`. An elevated
    // fill distinct from the page plus a hairline border keeps them
    // readable: the old `surface` fill + `Stroke::NONE` made dark
    // popups invisible on the dark page (and near-invisible in
    // light, where only a faint shadow separated white on white).
    v.window_fill = if theme == egui::Theme::Dark {
        scheme.container
    } else {
        scheme.surface
    };
    v.panel_fill = scheme.container_low;
    // Grid striping (tables) uses `faint_bg_color`: the
    // alternate-row `#f6f8fa`.
    v.faint_bg_color = scheme.container_low;
    v.extreme_bg_color = scheme.input_bg;
    // Explicit: TextEdit background MUST be the input color.
    v.text_edit_bg_color = Some(scheme.input_bg);
    // Inline code pills: a translucent gray wash.
    v.code_bg_color = scheme.code_bg;
    v.hyperlink_color = scheme.primary;
    v.selection = egui::style::Selection {
        bg_fill: scheme.primary_container,
        // A zero-WIDTH stroke carries the selected-text color:
        // egui paints the glyphs of a native (drag) selection with
        // `selection.stroke.color`. `Stroke::NONE` made them
        // TRANSPARENT (selected text vanished into the highlight).
        // Width 0 keeps the border off (a visible `stroke` painted
        // stray lines around selected text and focused fields).
        stroke: egui::Stroke::new(0.0, scheme.on_surface),
    };
    v.error_fg_color = scheme.error;
    v.window_stroke = egui::Stroke::new(1.0, scheme.outline_variant);
    // Rounded, softly elevated floating surfaces. Dark mode stays
    // flat: black shadows on dark chrome looked like dirty frames. Same
    // recipe as `soft_shadow` (no `Ui` here to ask).
    v.window_corner_radius = egui::CornerRadius::same(radius::TILE);
    v.menu_corner_radius = egui::CornerRadius::same(radius::TILE);
    let dark = theme == egui::Theme::Dark;
    v.window_shadow = shadow_recipe(dark, [0, 2], 12, 20);
    v.popup_shadow = shadow_recipe(dark, [0, 2], 14, 26);

    // Widget states: a neutral button ramp. Expansion 0 and ONE
    // stroke width in every state: a frame grows by its expansion,
    // and a `small` button subtracts `bg_stroke.width` from its
    // zeroed vertical padding, so a 0-width active stroke made the
    // pressed Copy 2px shorter and moved everything below it.
    let r = egui::CornerRadius::same(radius::CARD);
    // `noninteractive.bg_stroke` feeds `Separator` (H1/H2 rules):
    // the 1px `#d0d7de` border.
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, scheme.outline_variant);
    v.widgets.noninteractive.bg_fill = scheme.container;
    v.widgets.noninteractive.weak_bg_fill = scheme.container_low;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, scheme.on_surface_variant);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.bg_fill = scheme.container_high;
    v.widgets.inactive.weak_bg_fill = scheme.container_low;
    // The border tone doubles as the scrollbar handle (`foreground_color`):
    // on_surface was a hard black line, container_high was invisible on the
    // chrome, the border tone is visible but quiet in both themes.
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, scheme.outline_variant);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, scheme.outline_variant);
    v.widgets.inactive.corner_radius = r;
    v.widgets.inactive.expansion = 0.0;
    // Hover/active must LIGHTEN in dark mode: `container` and
    // `container_high` are the same dark gray there, so the old ramp
    // made idle, hover and press identical. Both tones are Scheme tokens
    // (`container_hover`/`container_active`), shared with the omnibox pill:
    // the two used to be separate near-identical literals.
    v.widgets.hovered.bg_fill = scheme.container_hover;
    v.widgets.hovered.weak_bg_fill = v.widgets.hovered.bg_fill;
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, scheme.on_surface);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, scheme.on_surface_variant);
    v.widgets.hovered.corner_radius = r;
    v.widgets.hovered.expansion = 0.0;
    v.widgets.active.bg_fill = scheme.container_active;
    v.widgets.active.weak_bg_fill = v.widgets.active.bg_fill;
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, scheme.on_surface);
    // Pressed AND keyboard-focused widgets use the `active` stripe:
    // a neutral 1px border makes focus visible without the blue
    // "ghost outline" that made the old design drop it entirely.
    // Width stays 1.0 for every state (the horizontal padding math
    // depends on it; a 0-width stroke used to shrink focused
    // buttons by 2px).
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, scheme.on_surface_variant);
    v.widgets.active.corner_radius = r;
    v.widgets.active.expansion = 0.0;
    // `open` is what a `Button`/`ComboBox` paints while its menu is open
    // (Preferences dropdowns): `weak_bg_fill`/`bg_stroke` are the ones egui
    // actually reads there, so setting only `bg_fill` left the popup button
    // with epaint's default gray fill and a `gray(160)` border that matched
    // no other surface in the app.
    v.widgets.open.bg_fill = scheme.container_high;
    v.widgets.open.weak_bg_fill = scheme.container_high;
    v.widgets.open.bg_stroke = egui::Stroke::new(1.0, scheme.on_surface_variant);
    v.widgets.open.fg_stroke = egui::Stroke::new(1.0, scheme.on_surface);
    v.widgets.open.corner_radius = r;
    v.widgets.open.expansion = 0.0;
}

mod widgets;

pub use widgets::*;

#[cfg(test)]
mod tests;
