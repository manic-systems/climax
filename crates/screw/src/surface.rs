use unicode_width::UnicodeWidthChar as _;

use crate::Style;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RowBreak {
    #[default]
    None,
    Hard,
    Soft,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cell {
    pub text:  String,
    pub width: usize,
    pub style: Style,
}

impl Cell {
    pub fn new(ch: char, style: Style) -> Option<Self> {
        let width = ch.width().unwrap_or(0);
        (width > 0).then(|| {
            Self {
                text: ch.to_string(),
                width,
                style,
            }
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    cells:       Vec<Cell>,
    break_after: RowBreak,
}

impl Row {
    pub const fn new() -> Self {
        Self {
            cells:       Vec::new(),
            break_after: RowBreak::None,
        }
    }

    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    pub const fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub const fn break_after(&self) -> RowBreak {
        self.break_after
    }

    pub const fn set_break_after(&mut self, row_break: RowBreak) {
        self.break_after = row_break;
    }

    fn push(&mut self, cell: Cell) {
        self.cells.push(cell);
    }

    fn truncate(&mut self, len: usize) {
        self.cells.truncate(len);
    }

    fn last_mut(&mut self) -> Option<&mut Cell> {
        self.cells.last_mut()
    }
}

impl Default for Row {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Surface {
    rows:   Vec<Row>,
    cursor: Option<Position>,
}

impl Default for Surface {
    fn default() -> Self {
        Self::new()
    }
}

impl Surface {
    pub fn new() -> Self {
        Self {
            rows:   vec![Row::new()],
            cursor: None,
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub const fn height(&self) -> usize {
        self.rows.len()
    }

    pub const fn cursor(&self) -> Option<Position> {
        self.cursor
    }

    pub const fn set_cursor(&mut self, position: Position) {
        self.cursor = Some(position);
    }

    pub fn set_cursor_here(&mut self) {
        self.cursor = Some(Position {
            row: self.rows.len().saturating_sub(1),
            col: self.current_col(),
        });
    }

    pub fn write(&mut self, text: impl AsRef<str>, style: Style) {
        for ch in text.as_ref().chars() {
            if ch == '\n' {
                self.newline();
            } else if let Some(cell) = Cell::new(ch, style) {
                self.current_row_mut().push(cell);
            } else if let Some(last) = self.current_row_mut().last_mut() {
                last.text.push(ch);
            }
        }
    }

    pub fn newline(&mut self) {
        self.newline_with_break(RowBreak::Hard);
    }

    pub fn soft_wrap(&mut self) {
        self.newline_with_break(RowBreak::Soft);
    }

    pub fn current_col(&self) -> usize {
        self.rows
            .last()
            .map_or(0, |row| row.cells().iter().map(|cell| cell.width).sum())
    }

    pub fn row_width(&self, row: usize) -> usize {
        self.rows
            .get(row)
            .map_or(0, |row| row.cells().iter().map(|cell| cell.width).sum())
    }

    pub fn fit_width(&mut self, terminal_width: usize) {
        let max_columns = fitted_columns(terminal_width);

        for row in &mut self.rows {
            let mut width = 0_usize;
            let keep = row
                .cells()
                .iter()
                .take_while(|cell| {
                    let next = width + cell.width;
                    if next <= max_columns {
                        width = next;
                        true
                    } else {
                        false
                    }
                })
                .count();
            row.truncate(keep);
        }

        if let Some(cursor) = self.cursor {
            let row = cursor.row.min(self.rows.len().saturating_sub(1));
            let col = cursor.col.min(self.row_width(row)).min(max_columns);
            self.cursor = Some(Position { row, col });
        }
    }

    pub fn plain_text(&self) -> String {
        self.rows
            .iter()
            .map(|row| {
                row.cells()
                    .iter()
                    .map(|cell| cell.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn newline_with_break(&mut self, row_break: RowBreak) {
        self.current_row_mut().set_break_after(row_break);
        self.rows.push(Row::new());
    }

    fn current_row_mut(&mut self) -> &mut Row {
        self.rows
            .last_mut()
            .expect("surface always contains at least one row")
    }
}

fn fitted_columns(terminal_width: usize) -> usize {
    terminal_width.saturating_sub(1).max(1)
}
