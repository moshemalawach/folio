//! A table's grid: rows and columns in and out, sorting, shading and alignment over a
//! range of cells, and the part of a table a copy takes. Pure functions on `TableBlock`,
//! so the editor stays about input and drawing.

use std::cmp::Ordering;

use crate::model::{ParaAlign, Rgb, StyledText, TableBlock, TableCell};

/// A rectangle of cells, inclusive: rows `top..=bottom`, columns `left..=right`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRange {
    pub top: usize,
    pub left: usize,
    pub bottom: usize,
    pub right: usize,
}

impl CellRange {
    pub fn cell(row: usize, col: usize) -> Self {
        Self {
            top: row,
            left: col,
            bottom: row,
            right: col,
        }
    }

    /// The cells between two corners, in any order.
    pub fn spanning(a: (usize, usize), b: (usize, usize)) -> Self {
        Self {
            top: a.0.min(b.0),
            left: a.1.min(b.1),
            bottom: a.0.max(b.0),
            right: a.1.max(b.1),
        }
    }

    pub fn whole(table: &TableBlock) -> Self {
        Self {
            top: 0,
            left: 0,
            bottom: table.rows.len().saturating_sub(1),
            right: table.cols() - 1,
        }
    }

    /// The range's rows, across every column.
    pub fn full_rows(self, table: &TableBlock) -> Self {
        Self {
            left: 0,
            right: table.cols() - 1,
            ..self
        }
    }

    /// The range's columns, down every row.
    pub fn full_columns(self, table: &TableBlock) -> Self {
        Self {
            top: 0,
            bottom: table.rows.len().saturating_sub(1),
            ..self
        }
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        (self.top..=self.bottom).contains(&row) && (self.left..=self.right).contains(&col)
    }

    pub fn is_single(&self) -> bool {
        self.top == self.bottom && self.left == self.right
    }

    /// The range cut to the table's size (after rows or columns went).
    pub fn clamped(self, table: &TableBlock) -> Self {
        let last_row = table.rows.len().saturating_sub(1);
        let last_col = table.cols() - 1;
        Self {
            top: self.top.min(last_row),
            left: self.left.min(last_col),
            bottom: self.bottom.min(last_row),
            right: self.right.min(last_col),
        }
    }
}

/// A command of the Table tab or the cell menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableCmd {
    SelectTable,
    SelectColumns,
    SelectRows,
    SelectCell,
    DeleteTable,
    DeleteColumns,
    DeleteRows,
    InsertAbove,
    InsertBelow,
    InsertLeft,
    InsertRight,
    HideBorders,
    Shading(Option<Rgb>),
    Align(ParaAlign),
    SortAscending,
    SortDescending,
    HeaderRow,
}

/// Light cell shadings, Office's tints of grey, blue, green, yellow, orange, purple and red.
pub const SHADES: [Rgb; 10] = [
    Rgb(0xf2, 0xf2, 0xf2),
    Rgb(0xd9, 0xd9, 0xd9),
    Rgb(0xdd, 0xeb, 0xf7),
    Rgb(0xbd, 0xd7, 0xee),
    Rgb(0xe2, 0xef, 0xda),
    Rgb(0xc6, 0xe0, 0xb4),
    Rgb(0xff, 0xf2, 0xcc),
    Rgb(0xfc, 0xe4, 0xd6),
    Rgb(0xe4, 0xdf, 0xec),
    Rgb(0xf8, 0xcb, 0xad),
];

pub fn insert_rows(table: &mut TableBlock, at: usize, count: usize) {
    let cols = table.cols();
    let at = at.min(table.rows.len());
    for _ in 0..count {
        table.rows.insert(at, vec![TableCell::default(); cols]);
    }
}

/// A new column at `at`, fitting its text.
pub fn insert_column(table: &mut TableBlock, at: usize) {
    let at = at.min(table.cols());
    for row in &mut table.rows {
        row.insert(at.min(row.len()), TableCell::default());
    }
    table.widths.insert(at, 0.0);
}

/// Removes rows `top..=bottom`. False (and nothing removed) when that would leave none:
/// the caller deletes the table instead.
pub fn delete_rows(table: &mut TableBlock, top: usize, bottom: usize) -> bool {
    let last = table.rows.len().saturating_sub(1);
    let (top, bottom) = (top.min(last), bottom.min(last));
    if top == 0 && bottom == last {
        return false;
    }
    table.rows.drain(top..=bottom);
    if top == 0 {
        table.header = false;
    }
    true
}

/// Removes columns `left..=right`; false when that would leave none.
pub fn delete_columns(table: &mut TableBlock, left: usize, right: usize) -> bool {
    let last = table.cols() - 1;
    let (left, right) = (left.min(last), right.min(last));
    if left == 0 && right == last {
        return false;
    }
    for row in &mut table.rows {
        row.drain(left..=right.min(row.len().saturating_sub(1)));
    }
    table.widths.drain(left..=right);
    true
}

fn each_cell(table: &mut TableBlock, range: CellRange, mut f: impl FnMut(&mut TableCell)) {
    for (row_index, row) in table.rows.iter_mut().enumerate() {
        for (col, cell) in row.iter_mut().enumerate() {
            if range.contains(row_index, col) {
                f(cell);
            }
        }
    }
}

pub fn shade(table: &mut TableBlock, range: CellRange, color: Option<Rgb>) {
    each_cell(table, range, |cell| cell.shading = color);
}

pub fn align(table: &mut TableBlock, range: CellRange, align: ParaAlign) {
    each_cell(table, range, |cell| cell.align = align);
}

pub fn clear(table: &mut TableBlock, range: CellRange) {
    each_cell(table, range, |cell| cell.text = StyledText::default());
}

/// The cells of `range` as a table of their own (a copy of part of a table).
pub fn sub_table(table: &TableBlock, range: CellRange) -> TableBlock {
    let range = range.clamped(table);
    TableBlock {
        rows: table.rows[range.top..=range.bottom]
            .iter()
            .map(|row| row[range.left..=range.right].to_vec())
            .collect(),
        widths: table.widths[range.left..=range.right].to_vec(),
        header: table.header && range.top == 0,
        hide_borders: table.hide_borders,
    }
}

/// Sorts the rows (below a header row) by one column's text, as the desktop notebook's Sort does:
/// ignoring case and accents, numbers by value, ties keeping their order.
pub fn sort(table: &mut TableBlock, col: usize, descending: bool) {
    let start = usize::from(table.header).min(table.rows.len());
    let key = |row: &Vec<TableCell>| {
        row.get(col)
            .map(|cell| cell.text.text.clone())
            .unwrap_or_default()
    };
    table.rows[start..].sort_by(|a, b| {
        let order = compare_cells(&key(a), &key(b));
        if descending {
            order.reverse()
        } else {
            order
        }
    });
}

/// The order cells sort in: numbers by value, before words, which ignore case and
/// accents. It is a total order, as `sort_by` needs (it may panic on one that is not):
/// a column mixing "9", "10" and "1er" must not compare numbers one way and text the
/// other.
fn compare_cells(a: &str, b: &str) -> Ordering {
    // Only real numbers: "nan" and "inf" parse as floats but read as words.
    let number = |text: &str| {
        text.trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    };
    match (number(a), number(b)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => fold(a).cmp(&fold(b)),
    }
}

/// Lowercase without accents, so "École" sorts with "ecole".
pub(crate) fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.trim().chars().flat_map(char::to_lowercase) {
        match ch {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => out.push('a'),
            'ç' => out.push('c'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ñ' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => out.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'ÿ' | 'ý' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&[&str]]) -> TableBlock {
        TableBlock::from_strings(
            &rows
                .iter()
                .map(|row| row.iter().map(|cell| cell.to_string()).collect())
                .collect::<Vec<_>>(),
            false,
            0.0,
        )
    }

    fn texts(table: &TableBlock) -> Vec<Vec<String>> {
        table
            .rows
            .iter()
            .map(|row| row.iter().map(|cell| cell.text.text.clone()).collect())
            .collect()
    }

    #[test]
    fn rows_and_columns_go_in_and_out_with_their_widths() {
        let mut t = table(&[&["a", "b"], &["c", "d"]]);
        t.widths[1] = 90.0;
        insert_column(&mut t, 1);
        assert_eq!(t.widths, vec![0.0, 0.0, 90.0]);
        assert_eq!(texts(&t)[0], ["a", "", "b"]);
        insert_rows(&mut t, 1, 1);
        assert_eq!(t.rows.len(), 3);
        assert!(delete_columns(&mut t, 1, 1));
        assert_eq!(t.widths, vec![0.0, 90.0]);
        assert!(delete_rows(&mut t, 1, 1));
        assert_eq!(texts(&t), [["a", "b"], ["c", "d"]]);
        assert!(
            !delete_rows(&mut t, 0, 1),
            "deleting every row deletes the table instead"
        );
        assert!(!delete_columns(&mut t, 0, 1));
        assert_eq!(t.rows.len(), 2);
    }

    #[test]
    fn sorting_keeps_the_header_and_reads_numbers_and_accents() {
        let mut t = table(&[
            &["Name", "Mark"],
            &["Zoé", "9"],
            &["école", "12"],
            &["Adam", "10,5"],
        ]);
        t.header = true;
        sort(&mut t, 0, false);
        assert_eq!(
            texts(&t)
                .iter()
                .map(|row| row[0].as_str())
                .collect::<Vec<_>>(),
            ["Name", "Adam", "école", "Zoé"]
        );
        sort(&mut t, 1, true);
        assert_eq!(
            texts(&t)
                .iter()
                .map(|row| row[1].as_str())
                .collect::<Vec<_>>(),
            ["Mark", "12", "10,5", "9"]
        );
    }

    #[test]
    fn a_column_mixing_numbers_and_words_sorts_numbers_first() {
        // A French ranking column ("1er", "2e"…) with plain numbers and words like "nan",
        // long enough for `sort_by` to check its comparator: numbers by value, then words.
        let mut cells: Vec<String> = (1..=30)
            .map(|n| match n % 3 {
                0 => format!("{n}e"),
                1 => n.to_string(),
                _ => "nan".to_string(),
            })
            .collect();
        cells.push("1er".into());
        let rows: Vec<Vec<String>> = cells.iter().map(|cell| vec![cell.clone()]).collect();
        let mut t = TableBlock::from_strings(&rows, false, 0.0);
        sort(&mut t, 0, false);
        let sorted: Vec<String> = texts(&t).into_iter().map(|row| row[0].clone()).collect();
        let numbers: Vec<f64> = sorted
            .iter()
            .map_while(|cell| cell.parse::<f64>().ok().filter(|v| v.is_finite()))
            .collect();
        assert_eq!(numbers.len(), 10, "{sorted:?}");
        assert!(numbers.windows(2).all(|pair| pair[0] <= pair[1]));
        let words = &sorted[numbers.len()..];
        assert!(words
            .windows(2)
            .all(|pair| fold(&pair[0]) <= fold(&pair[1])));
        sort(&mut t, 0, true);
        assert_eq!(texts(&t)[0][0], "nan", "descending puts the words first");
    }

    #[test]
    fn ranges_shade_align_clear_and_copy_part_of_a_table() {
        let mut t = table(&[&["a", "b", "c"], &["d", "e", "f"]]);
        let range = CellRange::spanning((1, 2), (0, 1));
        assert_eq!(
            range,
            CellRange {
                top: 0,
                left: 1,
                bottom: 1,
                right: 2
            }
        );
        shade(&mut t, range, Some(SHADES[2]));
        align(&mut t, range, ParaAlign::Center);
        assert_eq!(t.rows[1][2].shading, Some(SHADES[2]));
        assert_eq!(t.rows[0][0].shading, None);
        let part = sub_table(&t, range);
        assert_eq!(texts(&part), [["b", "c"], ["e", "f"]]);
        clear(&mut t, CellRange::cell(0, 0));
        assert_eq!(t.rows[0][0].text.text, "");
        assert_eq!(
            range.full_rows(&t),
            CellRange {
                top: 0,
                left: 0,
                bottom: 1,
                right: 2
            }
        );
    }
}
