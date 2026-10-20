//! Help ▸ Release Notes (`menus-and-shortcuts.md` §1.8): opened by `help.releaseNotes`, the
//! user-facing changelog [`whats-new.md`](NOTES) compiled into the binary (`include_str!`). It has
//! no draft and one button, Close (Esc).
//!
//! The Markdown is shown as what it is made of (`whats-new.md` → its conventions): `##` and `###`
//! headings, bullets (nested by indent) and plain paragraphs. HTML comments (the file's own
//! conventions) are left out, and so is the `#` title, which the dialog's title says. Inline
//! markup is reduced to its text: `**bold**` and `` `code` `` lose their marks, `[text](url)`
//! keeps its text. A menu path's ▸ is shown as › when the interface fonts lack it.

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use egui::{Label, RichText, Ui};
use serde_json::Value;

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::tl;

pub const TITLE: &str = "Release Notes";
/// The dialog's width (points).
pub const WIDTH: f32 = 600.0;
/// The height of the notes (points), less on a window too short for it.
pub const NOTES_H: f32 = 380.0;
/// What the dialog's frame, title and button row take besides the notes, and the room kept above
/// and below it (points).
const CHROME_H: f32 = 100.0;
const SCREEN_MARGIN: f32 = 48.0;

/// `docs/whats-new.md`, as this build carries it.
pub const NOTES: &str = include_str!("../../../../docs/whats-new.md");

/// One block of the notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A `##` (2) or deeper heading.
    Heading {
        level: u8,
        text: String,
    },
    /// A bullet; `depth` 0 at the margin, one more per two spaces of indent (at most 3).
    Bullet {
        depth: u8,
        text: String,
    },
    Paragraph(String),
}

/// `text` without its HTML comments (`<!-- … -->`, over any number of lines; an unclosed one runs
/// to the end).
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(rest.get(..start).unwrap_or(""));
        let after = rest.get(start + 4..).unwrap_or("");
        rest = match after.find("-->") {
            Some(end) => after.get(end + 3..).unwrap_or(""),
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Inline Markdown reduced to its text: links keep their text, emphasis and code marks go.
pub fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = rest.get(open + 1..).unwrap_or("");
        // The label runs to the first `]`, which must open the target at once.
        let link = after.find(']').filter(|&close| after.get(close + 1..).is_some_and(|r| r.starts_with('('))).and_then(|close| {
            let target = after.get(close + 2..)?;
            let end = target.find(')')?;
            Some((after.get(..close)?, target.get(end + 1..)?))
        });
        match link {
            Some((label, tail)) => {
                out.push_str(rest.get(..open).unwrap_or(""));
                out.push_str(label);
                rest = tail;
            }
            None => {
                out.push_str(rest.get(..=open).unwrap_or(""));
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out.replace("**", "").replace('`', "")
}

/// The notes as blocks, comments and the `#` title left out.
pub fn blocks(markdown: &str) -> Vec<Block> {
    let text = strip_comments(markdown);
    let mut out: Vec<Block> = Vec::new();
    // Whether the last block still takes continuation lines (a paragraph or a bullet).
    let mut open = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            open = false;
            continue;
        }
        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes).is_none_or(|&b| b == b' ') {
            open = false;
            if hashes > 1 {
                let level = u8::try_from(hashes).unwrap_or(6);
                out.push(Block::Heading { level, text: inline(trimmed.get(hashes..).unwrap_or("").trim()) });
            }
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            let depth = u8::try_from((indent / 2).min(3)).unwrap_or(3);
            out.push(Block::Bullet { depth, text: inline(item.trim()) });
            open = true;
            continue;
        }
        let more = inline(trimmed);
        match out.last_mut() {
            Some(Block::Paragraph(p) | Block::Bullet { text: p, .. }) if open => {
                p.push(' ');
                p.push_str(&more);
            }
            _ => {
                out.push(Block::Paragraph(more));
                open = true;
            }
        }
    }
    out
}

pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::ReleaseNotes)
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look { title: tl!("Release Notes"), width: WIDTH, buttons: vec![Button::close()] }
}

/// The height of the notes on a screen `screen_h` points tall.
pub fn notes_height(screen_h: f32) -> f32 {
    NOTES_H.min(screen_h - CHROME_H - SCREEN_MARGIN).max(160.0)
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    let notes = crate::credits::baked(ui.ctx()).release_notes;
    let height = notes_height(ui.ctx().content_rect().height());
    let mut list = blocks(notes);
    // Each heading, bullet and paragraph through the catalogs (`localization.md` → Release notes);
    // an edited one shows in English until its translation is updated.
    for b in &mut list {
        let (Block::Heading { text, .. } | Block::Bullet { text, .. } | Block::Paragraph(text)) = b;
        *text = crate::i18n::release_note(text);
    }
    // Menu paths are written with ▸, which the interface fonts may lack: show › (or >) instead of
    // a missing glyph.
    let font = fonts::regular(t.body);
    if !ui.fonts_mut(|f| f.has_glyph(&font, '▸')) {
        let arrow = if ui.fonts_mut(|f| f.has_glyph(&font, '›')) { "›" } else { ">" };
        for b in &mut list {
            let (Block::Heading { text, .. } | Block::Bullet { text, .. } | Block::Paragraph(text)) = b;
            *text = text.replace('▸', arrow);
        }
    }
    egui::ScrollArea::vertical()
        .id_salt("dialog:releaseNotes/notes")
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if list.is_empty() {
                ui.label(RichText::new(tl!("There are no release notes in this copy.")).font(fonts::regular(t.body)).color(t.text_label));
            }
            for (i, block) in list.iter().enumerate() {
                match block {
                    Block::Heading { level, text } => {
                        if i > 0 {
                            ui.add_space(if *level <= 2 { 12.0 } else { 6.0 });
                        }
                        let size = if *level <= 2 { t.heading } else { t.body };
                        let color = if *level <= 2 { t.text } else { t.text_label };
                        ui.add(Label::new(RichText::new(text).font(fonts::semibold(size)).color(color)).wrap());
                    }
                    Block::Bullet { depth, text } => {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.add_space(4.0 + f32::from(*depth) * 16.0);
                            ui.label(RichText::new("•").font(fonts::regular(t.body)).color(t.text_dim));
                            ui.add(Label::new(RichText::new(text).font(fonts::regular(t.body)).color(t.text_label)).wrap());
                        });
                    }
                    Block::Paragraph(text) => {
                        ui.add(Label::new(RichText::new(text).font(fonts::regular(t.body)).color(t.text_label)).wrap());
                    }
                }
            }
            ui.add_space(4.0);
        });
}

/// The dialog has no button but Close.
pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_bullets_and_paragraphs_without_comments_or_the_title() {
        let md = "# What's new\n\n<!-- a comment\nover lines -->\n\n## 0.1 — June 2026\n\n### Library\n\n- **Star** photos with `photo.star.toggle`\n  and more on the next line\n  - nested [see the guide](docs/x.md)\n\nA paragraph\nthat wraps. <!-- inline --> End.\n<!-- unclosed\n- never shown\n";
        assert_eq!(
            blocks(md),
            [
                Block::Heading { level: 2, text: "0.1 — June 2026".into() },
                Block::Heading { level: 3, text: "Library".into() },
                Block::Bullet { depth: 0, text: "Star photos with photo.star.toggle and more on the next line".into() },
                Block::Bullet { depth: 1, text: "nested see the guide".into() },
                Block::Paragraph("A paragraph that wraps.  End.".into()),
            ]
        );
    }

    #[test]
    fn odd_input_never_panics() {
        for md in ["", "#", "######", "####### seven", "[", "[a](", "[a](b", "<!--", "-->", "- ", "*", "#no space", "  \n\t\n", "ü[ä](ö)"] {
            let _ = blocks(md);
        }
        assert_eq!(blocks("#no space"), [Block::Paragraph("#no space".into())]);
        assert_eq!(inline("a [b] c [d](e) f"), "a [b] c d f");
    }

    #[test]
    fn the_compiled_notes_show_no_comment() {
        let shown = blocks(NOTES);
        assert!(!shown.is_empty());
        let text = format!("{shown:?}");
        assert!(!text.contains("<!--") && !text.contains("-->") && !text.contains("Adapted from"), "{text}");
    }
}
