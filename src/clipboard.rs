//! Rich text from other apps. egui hands a paste over as plain text; when it arrives,
//! Folio also reads the clipboard's HTML (what browsers and word processors put there)
//! and keeps its paragraphs, lists, tables and text formatting, as the desktop notebook does.

#[cfg(test)]
use std::cell::RefCell;
use std::sync::Arc;

use crate::model::{Block, BlockKind, InlineStyle, Rgb, StyledText, TableBlock, TableCell};

/// The clipboard's HTML, if it has some.
pub fn read_html() -> Option<String> {
    #[cfg(test)]
    {
        TEST_HTML.with(|html| html.borrow().clone())
    }
    #[cfg(not(test))]
    {
        arboard::Clipboard::new().ok()?.get().html().ok()
    }
}

#[cfg(test)]
thread_local! {
    /// What `read_html` returns in tests (UI tests run on the test's thread).
    pub static TEST_HTML: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Paragraphs, lists, quotes, code and tables of an HTML fragment, with the text
/// formatting the desktop notebook keeps when pasting from a web page or a word processor. Pictures are
/// left out (they would have to be downloaded).
pub fn html_blocks(html: &str) -> Vec<Block> {
    let mut builder = Builder::default();
    for token in tokens(html) {
        builder.token(token);
    }
    builder.finish()
}

/// Whether parsed HTML says what the pasted text says. The clipboard's HTML can be left
/// over from an earlier copy, so it is used only when its letters match, in order and
/// ignoring case; digits and bullets are ignored, since apps number lists differently in
/// their plain text.
pub fn matches_text(blocks: &[Block], text: &str) -> bool {
    let key = |text: &str| -> String {
        text.chars()
            .filter(|ch| ch.is_alphabetic())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let html = blocks
        .iter()
        .map(|block| match &block.kind {
            BlockKind::Table(table) => table.plain_text(),
            _ => block.text.text.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let html = key(&html);
    !html.is_empty() && html == key(text)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        closed: bool,
    },
    Close(String),
    Text(String),
}

/// A forgiving tokenizer for clipboard HTML: tags with their attributes, closing tags and
/// text with entities decoded. Comments, doctypes and the content of style, script, head
/// and title are skipped.
fn tokens(html: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        if !rest.starts_with('<') {
            let end = rest.find('<').unwrap_or(rest.len());
            out.push(Token::Text(decode(&rest[..end])));
            rest = &rest[end..];
            continue;
        }
        let Some(end) = tag_end(rest) else {
            out.push(Token::Text(decode(rest)));
            break;
        };
        let inner = &rest[1..end];
        rest = &rest[end + 1..];
        if inner.starts_with('!') || inner.starts_with('?') {
            continue;
        }
        if let Some(name) = inner.strip_prefix('/') {
            out.push(Token::Close(name.trim().to_ascii_lowercase()));
            continue;
        }
        let closed = inner.ends_with('/');
        let (name, attrs) = parse_tag(inner.trim_end_matches('/'));
        if matches!(name.as_str(), "style" | "script" | "head" | "title") && !closed {
            let close = format!("</{name}");
            rest = match rest.to_ascii_lowercase().find(&close) {
                Some(at) => rest[at..].find('>').map_or("", |gt| &rest[at + gt + 1..]),
                None => "",
            };
            continue;
        }
        out.push(Token::Open {
            name,
            attrs,
            closed,
        });
    }
    out
}

/// Where a tag that starts `text` ends (its '>'), skipping quoted attribute values.
fn tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (at, ch) in text.char_indices().skip(1) {
        match (quote, ch) {
            (None, '"' | '\'') => quote = Some(ch),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(at),
            _ => {}
        }
    }
    None
}

fn parse_tag(inner: &str) -> (String, Vec<(String, String)>) {
    let inner = inner.trim();
    let name_end = inner
        .find(|c: char| c.is_whitespace())
        .unwrap_or(inner.len());
    let name = inner[..name_end].to_ascii_lowercase();
    let mut attrs = Vec::new();
    let mut rest = inner[name_end..].trim_start();
    while !rest.is_empty() {
        let key_end = rest
            .find(|c: char| c == '=' || c.is_whitespace())
            .unwrap_or(rest.len());
        let key = rest[..key_end].to_ascii_lowercase();
        rest = rest[key_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            if let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let body = &after[1..];
                let end = body.find(q).unwrap_or(body.len());
                value = decode(&body[..end]);
                rest = body.get(end + 1..).unwrap_or("").trim_start();
            } else {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                value = decode(&after[..end]);
                rest = after[end..].trim_start();
            }
        }
        if !key.is_empty() {
            attrs.push((key, value));
        }
    }
    (name, attrs)
}

/// Decodes HTML entities: the common named ones and numeric ones.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        // The ';' is looked for in bytes, so text after '&' that is not ASCII never splits
        // a character.
        let Some(end) = rest.bytes().take(12).position(|byte| byte == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| entity.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The link of an `<a href>` in pasted HTML, kept only when it goes to the web or to mail.
/// A page or document on the clipboard could otherwise plant `file:` links, UNC paths
/// (`\\server\share`, which can leak Windows credentials when opened) or script URLs that
/// a later Ctrl+click opens; those, and addresses with no scheme, are dropped and the
/// anchor's text stays plain. Folio-to-Folio pastes come from the editor's own clipboard,
/// not HTML, so their page links are untouched. Typed links go through `normalize_link`.
fn pasted_link(href: &str) -> Option<String> {
    let href = href.trim();
    let lower = href.to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
        .then(|| href.to_string())
}

/// A CSS colour: #rgb, #rrggbb or rgb(r, g, b). Transparent or unknown gives None.
fn css_color(value: &str) -> Option<Rgb> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let hex: String = match hex.len() {
            3 => hex.chars().flat_map(|c| [c, c]).collect(),
            6 => hex.to_string(),
            _ => return None,
        };
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        return Some(Rgb(channel(0)?, channel(2)?, channel(4)?));
    }
    let inner = value
        .strip_prefix("rgb(")
        .or_else(|| value.strip_prefix("rgba("))?
        .trim_end_matches(')');
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() == 4 && parts[3].parse::<f32>().is_ok_and(|alpha| alpha == 0.0) {
        return None;
    }
    Some(Rgb(
        parts.first()?.parse().ok()?,
        parts.get(1)?.parse().ok()?,
        parts.get(2)?.parse().ok()?,
    ))
}

/// Applies an inline `style="…"` to the text style inside an element.
fn apply_css(style: &mut InlineStyle, css: &str) {
    for declaration in css.split(';') {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        let value = value.trim().to_ascii_lowercase();
        match property.trim().to_ascii_lowercase().as_str() {
            "font-weight" => {
                style.bold = value == "bold"
                    || value == "bolder"
                    || value.parse::<u32>().is_ok_and(|weight| weight >= 600)
            }
            "font-style" => {
                style.italic = value.starts_with("italic") || value.starts_with("oblique")
            }
            "text-decoration" | "text-decoration-line" => {
                style.underline |= value.contains("underline");
                style.strike |= value.contains("line-through");
            }
            "color" => style.color = css_color(&value).filter(|color| *color != Rgb(0, 0, 0)),
            "background-color" | "background" => {
                style.highlight = css_color(&value).filter(|color| *color != Rgb(255, 255, 255))
            }
            "font-size" => {
                let points = value
                    .strip_suffix("pt")
                    .and_then(|pt| pt.trim().parse::<f32>().ok())
                    .or_else(|| {
                        value
                            .strip_suffix("px")
                            .and_then(|px| px.trim().parse::<f32>().ok())
                            .map(|px| px * 0.75)
                    });
                // Office pastes 11pt everywhere; only sizes that differ from the note text stay.
                style.size = points
                    .map(|pt| pt.round().clamp(8.0, 72.0) as u8)
                    .filter(|pt| *pt != 11);
            }
            "vertical-align" => {
                style.superscript = value == "super";
                style.subscript = value == "sub";
            }
            _ => {}
        }
    }
}

/// The kind of Home › Styles' Heading `level` (1 = Heading 1). P4 stores the desktop notebook's
/// Heading n one level down, level 1 being Page Title.
fn heading(level: u8) -> BlockKind {
    BlockKind::heading(level.clamp(1, 6))
}

/// Text with every run of whitespace made one space (outside <pre>).
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if matches!(ch, ' ' | '\n' | '\r' | '\t') {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(ch);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

fn append_styled(text: &mut StyledText, piece: &str, style: &InlineStyle) {
    let mut styled = StyledText::new(piece);
    let count = styled.char_count();
    styled.apply_style_chars(0..count, |each| *each = style.clone());
    text.append(&styled);
}

#[derive(Default)]
struct TableBuild {
    rows: Vec<Vec<TableCell>>,
    header: bool,
    in_cell: bool,
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    /// The paragraph being filled.
    line: Option<Block>,
    /// Open elements and the text style inside each.
    styles: Vec<(String, InlineStyle)>,
    /// Open lists, innermost last: numbered or not.
    lists: Vec<bool>,
    /// What the next paragraph starts as (a heading, list item, quote or code).
    kind: Option<BlockKind>,
    pre: usize,
    table: Option<TableBuild>,
}

impl Builder {
    fn style(&self) -> InlineStyle {
        self.styles
            .last()
            .map(|(_, style)| style.clone())
            .unwrap_or_default()
    }

    fn token(&mut self, token: Token) {
        match token {
            Token::Text(text) => self.text(&text),
            Token::Open {
                name,
                attrs,
                closed,
            } => self.open(&name, &attrs, closed),
            Token::Close(name) => self.close(&name),
        }
    }

    fn text(&mut self, text: &str) {
        let text = if self.pre > 0 {
            text.to_string()
        } else {
            collapse(text)
        };
        if text.is_empty() {
            return;
        }
        let style = self.style();
        if let Some(table) = self.table.as_mut() {
            if !table.in_cell {
                return;
            }
            if let Some(cell) = table.rows.last_mut().and_then(|row| row.last_mut()) {
                let fresh = cell.text.text.is_empty() || cell.text.text.ends_with('\n');
                append_styled(
                    &mut cell.text,
                    if fresh { text.trim_start() } else { &text },
                    &style,
                );
            }
            return;
        }
        let kind = self.kind.clone().unwrap_or(BlockKind::Paragraph);
        let level = self.lists.len().saturating_sub(1) as u8;
        let line = self.line.get_or_insert_with(|| {
            let mut block = Block::new(kind);
            block.indent = level;
            block
        });
        let fresh = line.text.text.is_empty();
        append_styled(
            &mut line.text,
            if fresh && self.pre == 0 {
                text.trim_start()
            } else {
                &text
            },
            &style,
        );
    }

    /// Ends the paragraph being filled (an empty one is dropped).
    fn flush(&mut self) {
        if let Some(mut line) = self.line.take() {
            let trimmed = line.text.text.trim_end().chars().count();
            line.text.split_off_chars(trimmed);
            if !line.text.text.is_empty() {
                line.text.normalize();
                self.blocks.push(line);
            }
        }
    }

    fn cell_break(&mut self) {
        if let Some(cell) = self
            .table
            .as_mut()
            .filter(|table| table.in_cell)
            .and_then(|table| table.rows.last_mut())
            .and_then(|row| row.last_mut())
        {
            if !cell.text.text.is_empty() && !cell.text.text.ends_with('\n') {
                cell.text.append(&StyledText::new("\n"));
            }
        }
    }

    fn open(&mut self, name: &str, attrs: &[(String, String)], closed: bool) {
        let attr = |key: &str| {
            attrs
                .iter()
                .find(|(each, _)| each == key)
                .map(|(_, value)| value.as_str())
        };
        let in_cell = self.table.as_ref().is_some_and(|table| table.in_cell);
        match name {
            "br" | "p" | "div" | "li" if in_cell => self.cell_break(),
            "br" => self.flush(),
            "ul" | "ol" => {
                self.flush();
                self.lists.push(name == "ol");
            }
            "li" => {
                self.flush();
                self.kind = Some(if self.lists.last() == Some(&true) {
                    BlockKind::Numbered
                } else {
                    BlockKind::Bullet
                });
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.flush();
                self.kind = Some(heading(name[1..].parse().unwrap_or(1)));
            }
            "blockquote" => {
                self.flush();
                self.kind = Some(BlockKind::Quote);
            }
            "pre" => {
                self.flush();
                self.pre += 1;
                self.kind = Some(BlockKind::Code);
            }
            "p" | "div" | "section" | "article" | "header" | "footer" | "dd" | "dt"
            | "figcaption" => self.flush(),
            "hr" => {
                self.flush();
                self.blocks.push(Block::new(BlockKind::Divider));
            }
            "table" => {
                self.flush();
                self.table = Some(TableBuild::default());
            }
            "tr" => {
                if let Some(table) = self.table.as_mut() {
                    table.rows.push(Vec::new());
                }
            }
            "td" | "th" => {
                if let Some(table) = self.table.as_mut() {
                    if table.rows.is_empty() {
                        table.rows.push(Vec::new());
                    }
                    if name == "th" && table.rows.len() == 1 {
                        table.header = true;
                    }
                    if let Some(row) = table.rows.last_mut() {
                        row.push(TableCell::default());
                    }
                    table.in_cell = true;
                }
            }
            _ => {}
        }
        // Void elements and self-closed tags hold no text.
        if closed
            || matches!(
                name,
                "br" | "img" | "hr" | "meta" | "link" | "input" | "col" | "wbr" | "area" | "source"
            )
        {
            return;
        }
        let mut style = self.style();
        match name {
            "b" | "strong" => style.bold = true,
            "i" | "em" | "cite" => style.italic = true,
            "u" | "ins" => style.underline = true,
            "s" | "strike" | "del" => style.strike = true,
            "sub" => style.subscript = true,
            "sup" => style.superscript = true,
            "code" | "kbd" | "tt" => style.code = true,
            "mark" => style.highlight = Some(Rgb::HIGHLIGHTERS[0]),
            "a" => {
                style.link = attr("href")
                    .and_then(pasted_link)
                    .map(|link| Arc::from(link.as_str()));
            }
            "font" => style.color = attr("color").and_then(css_color),
            _ => {}
        }
        if let Some(css) = attr("style") {
            apply_css(&mut style, css);
        }
        self.styles.push((name.to_string(), style));
    }

    fn close(&mut self, name: &str) {
        if let Some(at) = self.styles.iter().rposition(|(open, _)| open == name) {
            self.styles.truncate(at);
        }
        match name {
            "td" | "th" => {
                if let Some(table) = self.table.as_mut() {
                    table.in_cell = false;
                }
            }
            "table" => self.end_table(),
            "ul" | "ol" => {
                self.flush();
                self.lists.pop();
                self.kind = None;
            }
            "pre" => {
                self.flush();
                self.pre = self.pre.saturating_sub(1);
                self.kind = None;
            }
            "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "p" | "div"
            | "section" | "article" | "header" | "footer" | "dd" | "dt" | "figcaption" => {
                if self.table.as_ref().is_some_and(|table| table.in_cell) {
                    return;
                }
                self.flush();
                // Back to the list's item kind inside a list, plain text outside.
                self.kind = None;
            }
            _ => {}
        }
    }

    fn end_table(&mut self) {
        let Some(build) = self.table.take() else {
            return;
        };
        let rows: Vec<Vec<TableCell>> = build
            .rows
            .into_iter()
            .filter(|row| !row.is_empty())
            .collect();
        if rows.is_empty() {
            return;
        }
        let mut table = TableBlock {
            rows,
            widths: Vec::new(),
            header: build.header,
            hide_borders: false,
        };
        for cell in table.rows.iter_mut().flatten() {
            let trimmed = cell.text.text.trim_end().chars().count();
            cell.text.split_off_chars(trimmed);
        }
        table.repair();
        self.blocks.push(Block::new(BlockKind::Table(table)));
    }

    fn finish(mut self) -> Vec<Block> {
        self.flush();
        self.end_table();
        self.blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(name: &str, attrs: &[(&str, &str)], closed: bool) -> Token {
        Token::Open {
            name: name.to_string(),
            attrs: attrs
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            closed,
        }
    }

    #[test]
    fn tokens_read_tags_attributes_entities_and_skip_styles() {
        let got =
            tokens("<p class=\"x\">A &amp; B&nbsp;<b>c</b></p><style>p{}</style><!-- x --><br/>");
        assert_eq!(
            got,
            vec![
                open("p", &[("class", "x")], false),
                Token::Text("A & B\u{a0}".to_string()),
                open("b", &[], false),
                Token::Text("c".to_string()),
                Token::Close("b".to_string()),
                Token::Close("p".to_string()),
                open("br", &[], true),
            ]
        );
        // A stray '&' before text that is not ASCII stays as it is.
        assert_eq!(decode("&日本語の文; &#233;"), "&日本語の文; é");
    }

    #[test]
    fn css_reads_colours_weights_sizes_and_decorations() {
        assert_eq!(css_color("#f00"), Some(Rgb(255, 0, 0)));
        assert_eq!(css_color("rgb(1, 2, 3)"), Some(Rgb(1, 2, 3)));
        assert_eq!(css_color("rgba(0,0,0,0)"), None);
        assert_eq!(css_color("#€"), None);
        let mut style = InlineStyle::default();
        apply_css(
            &mut style,
            "font-weight:700; text-decoration: underline line-through; font-size:16px",
        );
        assert!(style.bold);
        assert!(style.underline);
        assert!(style.strike);
        assert_eq!(style.size, Some(12));
    }

    fn kinds(blocks: &[Block]) -> Vec<(String, String, u8)> {
        blocks
            .iter()
            .map(|block| {
                (
                    format!("{:?}", block.kind),
                    block.text.text.clone(),
                    block.indent,
                )
            })
            .collect()
    }

    #[test]
    fn html_paragraphs_lists_and_marks_become_blocks() {
        let blocks = html_blocks(
            "<p>Hello <b>bold</b> <span style=\"color:#c00000\">red</span></p>\
             <ul><li>One</li><li>Two<ul><li>Deep</li></ul></li></ul><ol><li>First</li></ol>",
        );
        let shapes = kinds(&blocks);
        assert_eq!(shapes[0].1, "Hello bold red");
        assert_eq!(
            shapes[1..]
                .iter()
                .map(|(_, text, level)| (text.as_str(), *level))
                .collect::<Vec<_>>(),
            [("One", 0), ("Two", 0), ("Deep", 1), ("First", 0)]
        );
        assert!(matches!(blocks[3].kind, BlockKind::Bullet));
        assert!(matches!(blocks[4].kind, BlockKind::Numbered));
        let text = &blocks[0].text;
        assert!(text.style_at(text.char_to_byte(6)).bold);
        assert_eq!(
            text.style_at(text.char_to_byte(11)).color,
            Some(Rgb(0xc0, 0, 0))
        );
        assert!(!text.style_at(text.char_to_byte(0)).bold);
    }

    #[test]
    fn html_tables_and_links_become_a_table_block() {
        let blocks = html_blocks(
            "<table><tr><th>Day</th><th>Task</th></tr>\
             <tr><td>Mon</td><td><a href=\"https://example.org\">Maths</a></td></tr></table>",
        );
        let BlockKind::Table(table) = &blocks[0].kind else {
            panic!("{blocks:?}")
        };
        assert!(table.header);
        assert_eq!(table.rows[1][1].text.text, "Maths");
        assert!(table.rows[1][1].text.style_at(0).link.is_some());
    }

    #[test]
    fn pasted_html_keeps_only_web_and_mail_links() {
        let hrefs = [
            ("file:///etc/passwd", None),
            ("\\\\server\\share\\doc.txt", None),
            ("folio://page/abc", None),
            ("javascript:alert(1)", None),
            ("data:text/html,hi", None),
            ("example.com", None),
            ("http://a.example", Some("http://a.example")),
            (" HTTPS://b.example ", Some("HTTPS://b.example")),
            ("mailto:me@example.org", Some("mailto:me@example.org")),
        ];
        let html: String = hrefs
            .iter()
            .enumerate()
            .map(|(at, (href, _))| format!("<p>Go <a href=\"{href}\">link {at}</a> now</p>"))
            .collect();
        let blocks = html_blocks(&html);
        assert_eq!(blocks.len(), hrefs.len(), "{blocks:?}");
        for (at, (block, (href, kept))) in blocks.iter().zip(hrefs).enumerate() {
            let text = &block.text;
            assert_eq!(text.text, format!("Go link {at} now"));
            assert_eq!(
                text.style_at(text.char_to_byte(3)).link.as_deref(),
                kept,
                "{href}"
            );
            assert!(text.style_at(0).link.is_none());
        }
    }

    #[test]
    fn word_html_keeps_sizes_and_drops_its_noise() {
        let blocks = html_blocks(
            "<html><head><style>p{margin:0}</style></head><body><!--StartFragment-->\
             <p class=MsoNormal><span style='font-size:14.0pt;mso-bidi-font-size:11.0pt'>Titre</span><o:p></o:p></p>\
             <!--EndFragment--></body></html>",
        );
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text.text, "Titre");
        assert_eq!(blocks[0].text.style_at(0).size, Some(14));
    }

    #[test]
    fn stale_html_is_not_used() {
        assert!(!matches_text(&html_blocks("<p>Old</p>"), "New"));
        assert!(matches_text(
            &html_blocks("<ol><li>First</li></ol>"),
            "1. First"
        ));
        assert!(matches_text(
            &html_blocks("<p>Hello <b>bold</b></p>"),
            "Hello bold\n"
        ));
    }
}
