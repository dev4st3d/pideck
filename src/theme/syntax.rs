//! Syntax colors for code surfaces. Hues follow the muted ANSI palettes so code
//! reads the same in the editor, the diff view, and terminal output.

use gpui::{Rgba, rgba};
use gpui_component::highlighter::{SyntaxColors, ThemeStyle};
use serde_json::json;

use super::{appearance, ash};

struct Syntax {
    keyword: u32,
    function: u32,
    type_name: u32,
    string: u32,
    number: u32,
    property: u32,
    /// Operators, escapes, and list markers.
    marker: u32,
    tag: u32,
}

// Keyword, function, and tag are a step lighter than their ANSI counterparts
// so they stay at 4.5:1 on Stone's lifted, tinted diff rows.
const DARK: Syntax = Syntax {
    keyword: 0xd2b2e6ff,
    function: 0x9cc0f0ff,
    type_name: 0xd9bf83ff,
    string: 0x9dc88fff,
    number: 0xe0b07aff,
    property: 0xb0caffff,
    marker: 0x89c8caff,
    tag: 0xfaa497ff,
};

const LIGHT: Syntax = Syntax {
    keyword: 0x854080ff,
    function: 0x315ea3ff,
    type_name: 0x765b00ff,
    string: 0x2f6843ff,
    number: 0x8a541cff,
    property: 0x395f9bff,
    marker: 0x226d70ff,
    tag: 0x9e3238ff,
};

#[derive(Clone, Copy)]
enum Emphasis {
    Plain,
    Italic,
    Bold,
}

/// `ThemeStyle` keeps its fields private, so styles are built through its
/// serde form. Comments, which stay dimmer than code, reuse the shared `ash`.
fn style(color: Rgba, emphasis: Emphasis) -> Option<ThemeStyle> {
    let mut style = json!({ "color": color });
    match emphasis {
        Emphasis::Plain => {}
        Emphasis::Italic => style["font_style"] = json!("italic"),
        Emphasis::Bold => style["font_weight"] = json!(700),
    }
    serde_json::from_value(style).ok()
}

/// Colors for the active appearance. Roles left unset (variables, punctuation)
/// keep the surrounding text color.
pub(crate) fn colors() -> SyntaxColors {
    let syntax = if appearance().is_dark() {
        &DARK
    } else {
        &LIGHT
    };
    let plain = |color: u32| style(rgba(color), Emphasis::Plain);
    let comment = style(ash(), Emphasis::Italic);
    SyntaxColors {
        comment,
        comment_doc: comment,
        keyword: plain(syntax.keyword),
        preproc: plain(syntax.keyword),
        function: plain(syntax.function),
        link_text: plain(syntax.function),
        type_: plain(syntax.type_name),
        constructor: plain(syntax.type_name),
        enum_: plain(syntax.type_name),
        variant: plain(syntax.type_name),
        attribute: plain(syntax.type_name),
        label: plain(syntax.type_name),
        string: plain(syntax.string),
        text_literal: plain(syntax.string),
        number: plain(syntax.number),
        boolean: plain(syntax.number),
        constant: plain(syntax.number),
        string_special_symbol: plain(syntax.number),
        property: plain(syntax.property),
        operator: plain(syntax.marker),
        string_escape: plain(syntax.marker),
        string_regex: plain(syntax.marker),
        string_special: plain(syntax.marker),
        punctuation_special: plain(syntax.marker),
        punctuation_list_marker: plain(syntax.marker),
        link_uri: plain(syntax.marker),
        tag: plain(syntax.tag),
        variable_special: plain(syntax.tag),
        tag_doctype: comment,
        title: style(rgba(syntax.function), Emphasis::Bold),
        emphasis: style(rgba(syntax.string), Emphasis::Italic),
        emphasis_strong: style(rgba(syntax.string), Emphasis::Bold),
        ..SyntaxColors::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Appearance, canvas, diff_added, diff_removed, selection, set_appearance};

    const ROLES: [&str; 32] = [
        "comment",
        "comment.doc",
        "keyword",
        "preproc",
        "function",
        "link_text",
        "type",
        "constructor",
        "enum",
        "variant",
        "attribute",
        "label",
        "string",
        "text.literal",
        "number",
        "boolean",
        "constant",
        "string.special.symbol",
        "property",
        "operator",
        "string.escape",
        "string.regex",
        "string.special",
        "punctuation.special",
        "punctuation.list_marker",
        "link_uri",
        "tag",
        "variable.special",
        "tag.doctype",
        "title",
        "emphasis",
        "emphasis.strong",
    ];

    fn luminance(color: Rgba) -> f32 {
        let linear = |component: f32| {
            if component <= 0.04045 {
                component / 12.92
            } else {
                ((component + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
    }

    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn every_role_resolves_and_unset_roles_keep_the_text_color() {
        let colors = colors();
        for role in ROLES {
            assert!(
                colors
                    .style(role)
                    .is_some_and(|style| style.color.is_some()),
                "{role} has no color"
            );
        }
        assert_eq!(colors.style("variable"), None);
        assert_eq!(colors.style("punctuation.bracket"), None);
        // Dotted captures fall back to their family.
        assert_eq!(colors.style("keyword.modifier"), colors.style("keyword"));
        assert_eq!(colors.style("function.method"), colors.style("function"));
    }

    #[test]
    fn comments_are_italic_and_headings_bold() {
        let colors = colors();
        assert_eq!(
            colors.style("comment").and_then(|style| style.font_style),
            Some(gpui::FontStyle::Italic)
        );
        assert_eq!(
            colors.style("title").and_then(|style| style.font_weight),
            Some(gpui::FontWeight::BOLD)
        );
    }

    #[test]
    fn light_and_dark_appearances_use_different_colors() {
        let original = appearance();
        set_appearance(Appearance::Light);
        let light = colors().style("keyword");
        set_appearance(Appearance::Black);
        let dark = colors().style("keyword");
        set_appearance(original);
        assert!(light.is_some() && dark.is_some());
        assert_ne!(light, dark);
    }

    #[test]
    fn code_stays_readable_on_every_surface_in_every_appearance() {
        let original = appearance();
        for appearance in Appearance::ALL {
            set_appearance(appearance);
            let colors = colors();
            for role in ROLES {
                let color = colors
                    .style(role)
                    .and_then(|style| style.color)
                    .map(Rgba::from)
                    .expect(role);
                // Comments reuse `ash`, which the diff view already allows
                // to drop to 3:1 on light tinted rows.
                let dimmed = role.starts_with("comment") || role == "tag.doctype";
                // A selection is transient and keeps `bone` and `ash` at 4.5:1,
                // so colored tokens only need 3:1 there.
                let surfaces = [
                    (canvas(), 4.5),
                    (diff_added(), 4.5),
                    (diff_removed(), 4.5),
                    (selection(), 3.0),
                ];
                for (background, minimum) in surfaces {
                    let minimum = if dimmed { 3.0 } else { minimum };
                    assert!(
                        contrast(color, background) >= minimum,
                        "{appearance:?}: {role} {color:?} on {background:?}"
                    );
                }
            }
        }
        set_appearance(original);
    }
}
