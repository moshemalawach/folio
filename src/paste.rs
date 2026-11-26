//! Putting copied blocks into a note at the caret, the three ways the desktop notebook's Paste Options
//! offer: as they were (Keep Source Formatting), in the look of the text at the caret
//! (Merge Formatting), or as plain text (Keep Text Only).

use std::borrow::Cow;
use std::ops::Range;

use crate::i18n::t;
use crate::icons;
use crate::model::{
    next_block_id, Block, BlockKind, Document, InlineStyle, ParaAlign, StyledText, MAX_LEVEL,
};

/// How Paste brings content in (the desktop notebook's Paste Options). `Default` is what Ctrl+V and
/// the main Paste button send: Keep Source Formatting, the desktop notebook's default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteMode {
    Default,
    KeepSource,
    Merge,
    TextOnly,
}

impl PasteMode {
    /// The options the Paste menu and the Paste Options button offer.
    pub const ALL: [PasteMode; 3] = [PasteMode::KeepSource, PasteMode::Merge, PasteMode::TextOnly];

    pub fn label(self) -> Cow<'static, str> {
        match self {
            PasteMode::Default | PasteMode::KeepSource => t!("ribbon-paste-keep-source"),
            PasteMode::Merge => t!("ribbon-paste-merge"),
            PasteMode::TextOnly => t!("ribbon-paste-text-only"),
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            PasteMode::Default | PasteMode::KeepSource => icons::CLIPBOARD_TEXT_EDIT,
            PasteMode::Merge => icons::CLIPBOARD_TEXT_LTR,
            PasteMode::TextOnly => icons::TEXT_T,
        }
    }
}

/// Where a paste goes: a paragraph (or a table cell) and the chars it replaces.
#[derive(Debug, Clone, PartialEq)]
pub struct PasteAt {
    pub block: u64,
    pub cell: Option<(usize, usize)>,
    pub range: Range<usize>,
}

/// Where the caret ends up: block, cell, char.
pub type Caret = (u64, Option<(usize, usize)>, usize);

/// The blocks of a plain text: a paragraph per line, addresses made links.
pub fn plain_blocks(text: &str) -> Vec<Block> {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(|line| {
            let mut block = Block::paragraph(line.trim_end());
            block.text.autolink_all();
            block
        })
        .collect()
}

/// `style` with font, size, colour and highlight from `caret` (Merge Formatting keeps
/// only the marks that carry meaning: bold, italic, underline, strike, scripts, links).
fn merged(style: &InlineStyle, caret: &InlineStyle) -> InlineStyle {
    InlineStyle {
        bold: style.bold,
        italic: style.italic,
        underline: style.underline,
        strike: style.strike,
        superscript: style.superscript,
        subscript: style.subscript,
        link: style.link.clone(),
        ..caret.clone()
    }
}

/// The copied blocks as the mode pastes them. `align` is the alignment of the paragraph
/// at the caret, which Merge Formatting gives the pasted paragraphs.
fn shape(source: &[Block], mode: PasteMode, caret: &InlineStyle, align: ParaAlign) -> Vec<Block> {
    match mode {
        PasteMode::Default | PasteMode::KeepSource => source.to_vec(),
        PasteMode::Merge => source
            .iter()
            .filter(|block| block.kind.is_textual() || matches!(block.kind, BlockKind::Table(_)))
            .map(|block| {
                let mut block = block.clone();
                if block.kind.is_textual() && !block.kind.is_list() {
                    block.kind = BlockKind::Paragraph;
                }
                block.tags.clear();
                block.checked_tags.clear();
                block.custom_tags.clear();
                block.align = align;
                let styles: Vec<InlineStyle> = block
                    .text
                    .char_styles()
                    .iter()
                    .map(|style| merged(style, caret))
                    .collect();
                block.text.rebuild(&styles);
                block
            })
            .collect(),
        PasteMode::TextOnly => {
            let text = source
                .iter()
                .filter_map(|block| match &block.kind {
                    BlockKind::Table(table) => Some(table.plain_text()),
                    kind if kind.is_textual() => Some(block.text.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            plain_blocks(&text)
                .into_iter()
                .map(|mut block| {
                    let styles = vec![caret.clone(); block.text.char_count()];
                    let links: Vec<_> = block
                        .text
                        .char_styles()
                        .into_iter()
                        .map(|style| style.link)
                        .collect();
                    let styles: Vec<InlineStyle> = styles
                        .into_iter()
                        .zip(links)
                        .map(|(style, link)| InlineStyle { link, ..style })
                        .collect();
                    block.text.rebuild(&styles);
                    block
                })
                .collect()
        }
    }
}

/// The pasted blocks as one text, a line per block (tables as tab-separated rows), for a
/// paste into a table cell.
fn joined(blocks: &[Block]) -> StyledText {
    let mut out = StyledText::default();
    for (n, block) in blocks.iter().enumerate() {
        if n > 0 {
            out.append(&StyledText::new("\n"));
        }
        match &block.kind {
            BlockKind::Table(table) => out.append(&StyledText::new(table.plain_text())),
            kind if kind.is_textual() => out.append(&block.text),
            _ => {}
        }
    }
    out
}

/// Pastes `source` at `at`. `inline` marks a copy from inside one paragraph: its text
/// joins the paragraph at the caret. Returns where the caret goes (after the pasted text).
pub fn paste_into(
    doc: &mut Document,
    at: &PasteAt,
    source: &[Block],
    inline: bool,
    mode: PasteMode,
    caret_style: &InlineStyle,
) -> Option<Caret> {
    let index = doc.block_index(at.block)?;
    let pieces = shape(source, mode, caret_style, doc.blocks[index].align);

    if let Some((row, col)) = at.cell {
        // A table holds no blocks: everything goes into the cell as lines.
        let BlockKind::Table(table) = &mut doc.blocks[index].kind else {
            return None;
        };
        let text = &mut table.rows.get_mut(row)?.get_mut(col)?.text;
        let count = text.char_count();
        let (start, end) = (at.range.start.min(count), at.range.end.min(count));
        text.delete_chars(start..end.max(start));
        let piece = joined(&pieces);
        text.insert_styled(start, &piece);
        text.normalize();
        return Some((at.block, Some((row, col)), start + piece.char_count()));
    }

    let head = &mut doc.blocks[index];
    let count = head.text.char_count();
    let (start, end) = (at.range.start.min(count), at.range.end.min(count));
    head.text.delete_chars(start..end.max(start));
    if pieces.is_empty() {
        return Some((at.block, None, start));
    }
    // A copy from inside one paragraph or cell joins the line whole, its line breaks too
    // (Keep Text Only splits such a clip into a block per line).
    if inline || (pieces.len() == 1 && pieces[0].kind.is_textual()) {
        let piece = if pieces.len() == 1 && pieces[0].kind.is_textual() {
            pieces[0].text.clone()
        } else {
            joined(&pieces)
        };
        head.text.insert_styled(start, &piece);
        head.text.normalize();
        return Some((at.block, None, start + piece.char_count()));
    }

    // Several blocks: the paragraph splits at the caret. The first pasted text ends its
    // head, the other blocks follow, and the rest of the paragraph joins the last one.
    // Their levels are kept relative to the shallowest of them (a copy keeps each
    // paragraph's own level), from the level of the paragraph at the caret.
    let tail = head.text.split_off_chars(start);
    let head_kind = head.kind.clone();
    let base = head.indent;
    let shallowest = pieces.iter().map(|block| block.indent).min().unwrap_or(0);
    let mut rest = pieces.into_iter().peekable();
    if rest.peek().is_some_and(|first| first.kind.is_textual()) {
        let first = rest.next()?;
        doc.blocks[index].text.append(&first.text);
    }
    let mut inserted: Vec<Block> = rest
        .map(|mut block| {
            block.id = next_block_id();
            block.indent = base
                .saturating_add(block.indent - shallowest)
                .min(MAX_LEVEL);
            block
        })
        .collect();
    let caret = match inserted.last_mut() {
        Some(last) if last.kind.is_textual() => {
            let caret = (last.id, None, last.text.char_count());
            last.text.append(&tail);
            caret
        }
        _ => {
            // After a table (or nothing more): the rest of the paragraph on a line of its own.
            let mut after = Block::new(if head_kind.is_list() {
                head_kind
            } else {
                BlockKind::Paragraph
            });
            after.indent = base;
            after.text = tail;
            let caret = (after.id, None, 0);
            inserted.push(after);
            caret
        }
    };
    doc.blocks.splice(index + 1..index + 1, inserted);
    for block in &mut doc.blocks {
        block.text.normalize();
    }
    Some(caret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Rgb;

    fn doc_of(lines: &[&str]) -> Document {
        Document {
            blocks: lines.iter().map(|line| Block::paragraph(*line)).collect(),
        }
    }

    fn texts(doc: &Document) -> Vec<String> {
        doc.blocks
            .iter()
            .map(|block| block.text.text.clone())
            .collect()
    }

    fn bold_world() -> Block {
        let mut block = Block::paragraph("world");
        block.text.apply_style_chars(0..5, |style| {
            style.bold = true;
            style.color = Some(Rgb(200, 0, 0));
        });
        block
    }

    #[test]
    fn an_inline_copy_joins_the_line_with_its_marks() {
        let mut doc = doc_of(&["Hello !"]);
        let at = PasteAt {
            block: doc.blocks[0].id,
            cell: None,
            range: 6..6,
        };
        let caret = paste_into(
            &mut doc,
            &at,
            &[bold_world()],
            true,
            PasteMode::KeepSource,
            &InlineStyle::default(),
        )
        .unwrap();
        assert_eq!(texts(&doc), ["Hello world!"]);
        assert_eq!(caret.2, 11);
        let text = &doc.blocks[0].text;
        assert!(text.style_at(text.char_to_byte(7)).bold);
        assert!(!text.style_at(text.char_to_byte(11)).bold);
    }

    #[test]
    fn several_paragraphs_split_the_line_at_the_caret() {
        let mut doc = doc_of(&["Start end"]);
        let at = PasteAt {
            block: doc.blocks[0].id,
            cell: None,
            range: 6..6,
        };
        let mut list = Block::new(BlockKind::Bullet);
        list.text = StyledText::new("item");
        list.indent = 1;
        let source = [Block::paragraph("one"), list, Block::paragraph("two")];
        let caret = paste_into(
            &mut doc,
            &at,
            &source,
            false,
            PasteMode::KeepSource,
            &InlineStyle::default(),
        )
        .unwrap();
        assert_eq!(texts(&doc), ["Start one", "item", "twoend"]);
        assert!(matches!(doc.blocks[1].kind, BlockKind::Bullet));
        assert_eq!(doc.blocks[1].indent, 1);
        assert_eq!((caret.0, caret.2), (doc.blocks[2].id, 3));
        let ids: std::collections::HashSet<u64> = doc.blocks.iter().map(|block| block.id).collect();
        assert_eq!(ids.len(), 3, "pasted blocks get fresh ids");
    }

    #[test]
    fn merge_keeps_marks_and_takes_the_look_at_the_caret() {
        let mut doc = doc_of(&[""]);
        let at = PasteAt {
            block: doc.blocks[0].id,
            cell: None,
            range: 0..0,
        };
        let caret = InlineStyle {
            size: Some(14),
            ..InlineStyle::default()
        };
        paste_into(
            &mut doc,
            &at,
            &[bold_world()],
            true,
            PasteMode::Merge,
            &caret,
        )
        .unwrap();
        let style = doc.blocks[0].text.style_at(0);
        assert!(style.bold, "bold is kept");
        assert_eq!(style.color, None, "the colour is the caret's");
        assert_eq!(style.size, Some(14));
    }

    #[test]
    fn merge_takes_the_alignment_of_the_paragraph_at_the_caret() {
        let mut doc = doc_of(&["ab"]);
        doc.blocks[0].align = ParaAlign::Center;
        let at = PasteAt {
            block: doc.blocks[0].id,
            cell: None,
            range: 1..1,
        };
        let mut right = Block::paragraph("two");
        right.align = ParaAlign::Right;
        let source = [Block::paragraph("one"), right];
        paste_into(
            &mut doc,
            &at,
            &source,
            false,
            PasteMode::Merge,
            &InlineStyle::default(),
        )
        .unwrap();
        assert_eq!(texts(&doc), ["aone", "twob"]);
        assert!(doc
            .blocks
            .iter()
            .all(|block| block.align == ParaAlign::Center));
    }

    #[test]
    fn text_only_drops_the_formatting_and_flattens_tables() {
        let mut doc = doc_of(&[""]);
        let at = PasteAt {
            block: doc.blocks[0].id,
            cell: None,
            range: 0..0,
        };
        let table =
            crate::model::TableBlock::from_strings(&[vec!["a".into(), "b".into()]], false, 0.0);
        let source = [bold_world(), Block::new(BlockKind::Table(table))];
        paste_into(
            &mut doc,
            &at,
            &source,
            false,
            PasteMode::TextOnly,
            &InlineStyle::default(),
        )
        .unwrap();
        assert_eq!(texts(&doc), ["world", "a\tb"]);
        assert!(!doc.blocks[0].text.style_at(0).bold);
        assert!(doc
            .blocks
            .iter()
            .all(|block| matches!(block.kind, BlockKind::Paragraph)));
    }

    #[test]
    fn a_table_pasted_mid_line_keeps_a_line_after_it() {
        let mut doc = doc_of(&["ab"]);
        let at = PasteAt {
            block: doc.blocks[0].id,
            cell: None,
            range: 1..1,
        };
        let table = crate::model::TableBlock::new(1, 2);
        let caret = paste_into(
            &mut doc,
            &at,
            &[Block::new(BlockKind::Table(table))],
            false,
            PasteMode::KeepSource,
            &InlineStyle::default(),
        )
        .unwrap();
        assert_eq!(texts(&doc), ["a", "", "b"]);
        assert!(matches!(doc.blocks[1].kind, BlockKind::Table(_)));
        assert_eq!((caret.0, caret.2), (doc.blocks[2].id, 0));
    }

    #[test]
    fn a_paste_into_a_cell_stays_in_the_cell() {
        let mut doc = Document {
            blocks: vec![Block::new(BlockKind::Table(crate::model::TableBlock::new(
                1, 1,
            )))],
        };
        let at = PasteAt {
            block: doc.blocks[0].id,
            cell: Some((0, 0)),
            range: 0..0,
        };
        let caret = paste_into(
            &mut doc,
            &at,
            &plain_blocks("one\ntwo"),
            false,
            PasteMode::KeepSource,
            &InlineStyle::default(),
        )
        .unwrap();
        let BlockKind::Table(table) = &doc.blocks[0].kind else {
            unreachable!()
        };
        assert_eq!(table.rows[0][0].text.text, "one\ntwo");
        assert_eq!(caret, (doc.blocks[0].id, Some((0, 0)), 7));
    }

    #[test]
    fn indented_paragraphs_keep_their_levels_relative_to_the_caret() {
        // Two bullets copied at level 1, pasted at the end of a level-1 bullet: the
        // second one lands at level 1 too, not one deeper.
        let bullet = |text: &str, level: u8| {
            let mut block = Block::new(BlockKind::Bullet);
            block.text = StyledText::new(text);
            block.indent = level;
            block
        };
        let mut doc = Document {
            blocks: vec![bullet("c", 1)],
        };
        let at = PasteAt {
            block: doc.blocks[0].id,
            cell: None,
            range: 1..1,
        };
        let source = [bullet("a", 1), bullet("b", 1), bullet("deeper", 2)];
        for mode in [PasteMode::KeepSource, PasteMode::Merge] {
            let mut doc = doc.clone();
            paste_into(&mut doc, &at, &source, false, mode, &InlineStyle::default()).unwrap();
            let levels: Vec<(String, u8)> = doc
                .blocks
                .iter()
                .map(|block| (block.text.text.clone(), block.indent))
                .collect();
            assert_eq!(
                levels,
                [("ca".into(), 1), ("b".into(), 1), ("deeper".into(), 2)],
                "{mode:?}"
            );
        }
        // At level 0 the same copy keeps its shape from there.
        doc.blocks[0].indent = 0;
        paste_into(
            &mut doc,
            &at,
            &source,
            false,
            PasteMode::KeepSource,
            &InlineStyle::default(),
        )
        .unwrap();
        let levels: Vec<u8> = doc.blocks.iter().map(|block| block.indent).collect();
        assert_eq!(levels, [0, 0, 1]);
    }

    #[test]
    fn a_copy_with_line_breaks_pastes_whole_as_text_only() {
        // A cell holding two lines, copied (inline) and pasted with Keep Text Only at the
        // end of a paragraph: both lines come.
        let mut doc = doc_of(&["Intro"]);
        let at = PasteAt {
            block: doc.blocks[0].id,
            cell: None,
            range: 5..5,
        };
        let clip = [Block::paragraph("one\ntwo")];
        let caret = paste_into(
            &mut doc,
            &at,
            &clip,
            true,
            PasteMode::TextOnly,
            &InlineStyle::default(),
        )
        .unwrap();
        assert_eq!(texts(&doc), ["Introone\ntwo"]);
        assert_eq!(caret.2, 12);
    }
}
