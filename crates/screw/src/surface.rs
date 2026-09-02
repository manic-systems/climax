use unicode_width::UnicodeWidthChar as _;

use crate::{Rect, Style};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fill {
    Transparent,
    Opaque(Style),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorMerge {
    PreserveBase,
    PreferOverlay,
    Hide,
}

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
    pub text: String,
    pub width: usize,
    pub style: Style,
}

impl Cell {
    pub fn new(ch: char, style: Style) -> Option<Self> {
        let width = ch.width().unwrap_or(0);
        (width > 0).then(|| Self {
            text: ch.to_string(),
            width,
            style,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    cells: Vec<Cell>,
    break_after: RowBreak,
}

impl Row {
    pub const fn new() -> Self {
        Self {
            cells: Vec::new(),
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
    rows: Vec<Row>,
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
            rows: vec![Row::new()],
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

    pub fn display_width(&self) -> usize {
        self.rows
            .iter()
            .map(|row| row.cells().iter().map(|cell| cell.width).sum())
            .max()
            .unwrap_or(0)
    }

    pub fn overlay(
        &mut self,
        source: &Self,
        destination: Rect,
        canvas: Rect,
        fill: Fill,
        cursor: CursorMerge,
    ) {
        let requested = destination.intersection(canvas);
        if !requested.is_empty() {
            for row in requested.origin.row..requested.bottom() {
                let source_row = row.saturating_sub(destination.origin.row);
                let writes = overlay_writes(source, source_row, destination, requested, fill);
                if writes.is_empty() {
                    continue;
                }
                self.ensure_row(row);
                apply_writes(
                    &mut self.rows[row],
                    &writes,
                    canvas.origin.col,
                    canvas.right(),
                );
            }
        }

        match cursor {
            CursorMerge::PreserveBase => {},
            CursorMerge::Hide => self.cursor = None,
            CursorMerge::PreferOverlay => {
                if let Some(position) = overlay_cursor(source, destination, canvas) {
                    self.cursor = Some(position);
                }
            },
        }
    }

    pub fn fit_width(&mut self, max_columns: usize) {
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

    /// Clip physical rows to a terminal height and discard an outside cursor.
    pub fn fit_height(&mut self, terminal_height: usize) {
        let height = terminal_height.max(1);
        self.rows.truncate(height);
        if self.cursor.is_some_and(|cursor| cursor.row >= height) {
            self.cursor = None;
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

    fn ensure_row(&mut self, row: usize) {
        while self.rows.len() <= row {
            self.rows.push(Row::new());
        }
    }
}

#[derive(Clone)]
enum Column {
    Cell(Cell),
    Continuation { start: usize },
}

#[derive(Clone)]
struct PlacedCell {
    start: usize,
    cell: Cell,
}

fn overlay_writes(
    source: &Surface,
    source_row: usize,
    destination: Rect,
    requested: Rect,
    fill: Fill,
) -> Vec<PlacedCell> {
    let mut writes = Vec::new();
    if let Fill::Opaque(style) = fill {
        writes.extend(
            (requested.origin.col..requested.right()).map(|start| PlacedCell {
                start,
                cell: Cell {
                    text: " ".into(),
                    width: 1,
                    style,
                },
            }),
        );
    }

    let Some(row) = source.rows().get(source_row) else {
        return writes;
    };
    let mut source_col = 0_usize;
    for cell in row.cells() {
        let source_right = source_col.saturating_add(cell.width);
        let target = destination.origin.col.saturating_add(source_col);
        let target_right = target.saturating_add(cell.width);
        let inside_source = source_right <= destination.size.width;
        let inside_requested = target >= requested.origin.col && target_right <= requested.right();
        if inside_source && inside_requested {
            writes.retain(|write| write.start < target || write.start >= target_right);
            writes.push(PlacedCell {
                start: target,
                cell: cell.clone(),
            });
        }
        source_col = source_right;
    }
    writes.sort_by_key(|write| write.start);
    writes
}

fn apply_writes(row: &mut Row, writes: &[PlacedCell], canvas_left: usize, canvas_right: usize) {
    let mut columns = columns(row);
    for write in writes {
        let end = write
            .start
            .saturating_add(write.cell.width)
            .min(canvas_right);
        if end.saturating_sub(write.start) != write.cell.width {
            continue;
        }
        ensure_columns(&mut columns, end);
        if (write.start..end)
            .any(|column| !footprint_within(&columns, column, canvas_left, canvas_right))
        {
            continue;
        }
        for column in write.start..end {
            clear_column(&mut columns, column);
        }
        columns[write.start] = Column::Cell(write.cell.clone());
        for slot in columns
            .iter_mut()
            .take(end)
            .skip(write.start.saturating_add(1))
        {
            *slot = Column::Continuation { start: write.start };
        }
    }
    row.cells = rebuild(&columns);
}

fn footprint_within(columns: &[Column], column: usize, left: usize, right: usize) -> bool {
    let Some(slot) = columns.get(column) else {
        return true;
    };
    let start = match slot {
        Column::Cell(_) => column,
        Column::Continuation { start } => *start,
    };
    let Some(Column::Cell(cell)) = columns.get(start) else {
        return true;
    };
    start >= left && start.saturating_add(cell.width) <= right
}

fn columns(row: &Row) -> Vec<Column> {
    let mut columns = Vec::new();
    for cell in row.cells() {
        let start = columns.len();
        columns.push(Column::Cell(cell.clone()));
        columns.extend((1..cell.width).map(|_| Column::Continuation { start }));
    }
    columns
}

fn ensure_columns(columns: &mut Vec<Column>, len: usize) {
    columns.extend((columns.len()..len).map(|_| Column::Cell(space(Style::PLAIN))));
}

fn clear_column(columns: &mut [Column], column: usize) {
    let Some(slot) = columns.get(column) else {
        return;
    };
    let start = match slot {
        Column::Cell(_) => column,
        Column::Continuation { start } => *start,
    };
    let Some(Column::Cell(cell)) = columns.get(start) else {
        return;
    };
    let width = cell.width;
    let style = cell.style;
    for slot in columns.iter_mut().skip(start).take(width) {
        *slot = Column::Cell(space(style));
    }
}

fn rebuild(columns: &[Column]) -> Vec<Cell> {
    let mut cells = Vec::new();
    let mut column = 0_usize;
    while column < columns.len() {
        match &columns[column] {
            Column::Cell(cell) => {
                let width = cell.width.max(1);
                cells.push(cell.clone());
                column = column.saturating_add(width);
            },
            Column::Continuation { .. } => {
                cells.push(space(Style::PLAIN));
                column += 1;
            },
        }
    }
    cells
}

fn space(style: Style) -> Cell {
    Cell {
        text: " ".into(),
        width: 1,
        style,
    }
}

fn overlay_cursor(source: &Surface, destination: Rect, canvas: Rect) -> Option<Position> {
    let source_cursor = source.cursor()?;
    if source_cursor.row >= source.height()
        || source_cursor.row >= destination.size.height
        || source_cursor.col > source.row_width(source_cursor.row)
        || source_cursor.col > destination.size.width
    {
        return None;
    }
    let position = Position {
        row: destination.origin.row.checked_add(source_cursor.row)?,
        col: destination.origin.col.checked_add(source_cursor.col)?,
    };
    (position.row >= canvas.origin.row
        && position.row < canvas.bottom()
        && position.col >= canvas.origin.col
        && position.col <= canvas.right())
    .then_some(position)
}

pub fn append_surface(out: &mut Surface, surface: &Surface, limit: usize) -> usize {
    let rows = surface.rows().iter().take(limit);
    let base_row = out.height().saturating_sub(1);
    let mut written = 0_usize;
    for (row_index, row) in rows.enumerate() {
        if row_index > 0 {
            let previous_break = surface.rows()[row_index - 1].break_after();
            out.newline_with_break(previous_break);
        }
        for cell in row.cells() {
            out.write(&cell.text, cell.style);
        }
        out.current_row_mut().set_break_after(row.break_after());
        written += 1;
    }
    if let Some(cursor) = surface.cursor()
        && cursor.row < written
    {
        out.set_cursor(Position {
            row: base_row + cursor.row,
            col: cursor.col,
        });
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    fn surface(text: &str) -> Surface {
        let mut surface = Surface::new();
        surface.write(text, Style::PLAIN);
        surface
    }

    #[test]
    fn transparent_and_opaque_gaps_have_distinct_semantics() {
        let source = surface("XY");
        let canvas = Rect::new(0, 0, 6, 1);

        let mut transparent = surface("abcdef");
        transparent.overlay(
            &source,
            Rect::new(0, 2, 4, 1),
            canvas,
            Fill::Transparent,
            CursorMerge::PreserveBase,
        );
        assert_eq!(transparent.plain_text(), "abXYef");

        let fill = Style::PLAIN.bg(Color::Blue);
        let mut opaque = surface("abcdef");
        opaque.overlay(
            &source,
            Rect::new(0, 2, 4, 1),
            canvas,
            Fill::Opaque(fill),
            CursorMerge::PreserveBase,
        );
        assert_eq!(opaque.plain_text(), "abXY  ");
        assert_eq!(opaque.rows()[0].cells().last().unwrap().style, fill);
    }

    #[test]
    fn explicit_spaces_are_opaque_in_transparent_mode() {
        let mut base = surface("abc");
        base.overlay(
            &surface(" "),
            Rect::new(0, 1, 1, 1),
            Rect::new(0, 0, 3, 1),
            Fill::Transparent,
            CursorMerge::PreserveBase,
        );
        assert_eq!(base.plain_text(), "a c");
    }

    #[test]
    fn overlay_repairs_an_intersected_wide_base_cell() {
        let wide = Style::PLAIN.fg(Color::Magenta);
        let mut base = Surface::new();
        base.write("a", Style::PLAIN);
        base.write("界", wide);
        base.write("z", Style::PLAIN);
        base.overlay(
            &surface("X"),
            Rect::new(0, 2, 1, 1),
            Rect::new(0, 0, 4, 1),
            Fill::Transparent,
            CursorMerge::PreserveBase,
        );

        assert_eq!(base.plain_text(), "a Xz");
        assert_eq!(base.row_width(0), 4);
        assert_eq!(base.rows()[0].cells()[1].style, wide);
    }

    #[test]
    fn overlay_does_not_damage_a_wide_base_cell_across_the_canvas_edge() {
        let mut base = surface("a界z");
        base.overlay(
            &surface("X"),
            Rect::new(0, 2, 1, 1),
            Rect::new(0, 2, 2, 1),
            Fill::Transparent,
            CursorMerge::PreserveBase,
        );

        assert_eq!(base.plain_text(), "a界z");
        assert_eq!(base.row_width(0), 4);
    }

    #[test]
    fn clipped_wide_source_is_dropped_then_uses_fill_policy() {
        let source = surface("界");
        let destination = Rect::new(0, 1, 1, 1);
        let canvas = Rect::new(0, 0, 3, 1);

        let mut transparent = surface("abc");
        transparent.overlay(
            &source,
            destination,
            canvas,
            Fill::Transparent,
            CursorMerge::PreserveBase,
        );
        assert_eq!(transparent.plain_text(), "abc");

        let mut opaque = surface("abc");
        opaque.overlay(
            &source,
            destination,
            canvas,
            Fill::Opaque(Style::PLAIN),
            CursorMerge::PreserveBase,
        );
        assert_eq!(opaque.plain_text(), "a c");
    }

    #[test]
    fn combining_marks_copy_with_their_base() {
        let source = surface("e\u{301}");
        let mut base = surface("abc");
        base.overlay(
            &source,
            Rect::new(0, 1, 1, 1),
            Rect::new(0, 0, 3, 1),
            Fill::Transparent,
            CursorMerge::PreserveBase,
        );
        assert_eq!(base.rows()[0].cells()[1].text, "e\u{301}");
        assert_eq!(base.row_width(0), 3);
    }

    #[test]
    fn overlay_materializes_blank_canvas_and_obeys_cursor_policy() {
        let mut source = surface("x");
        source.set_cursor(Position { row: 0, col: 0 });
        let mut base = surface("a");
        base.set_cursor(Position { row: 0, col: 1 });
        base.overlay(
            &source,
            Rect::new(2, 3, 1, 1),
            Rect::new(0, 0, 5, 4),
            Fill::Transparent,
            CursorMerge::PreferOverlay,
        );
        assert_eq!(base.height(), 3);
        assert_eq!(base.plain_text(), "a\n\n   x");
        assert_eq!(base.cursor(), Some(Position { row: 2, col: 3 }));

        base.overlay(
            &Surface::new(),
            Rect::new(0, 0, 1, 1),
            Rect::new(0, 0, 5, 4),
            Fill::Transparent,
            CursorMerge::Hide,
        );
        assert_eq!(base.cursor(), None);
    }

    #[test]
    fn repeated_opaque_composition_is_idempotent() {
        let source = surface("xy");
        let mut base = surface("abcdef");
        let destination = Rect::new(0, 1, 4, 1);
        let canvas = Rect::new(0, 0, 6, 1);
        base.overlay(
            &source,
            destination,
            canvas,
            Fill::Opaque(Style::PLAIN.bg(Color::Red)),
            CursorMerge::PreserveBase,
        );
        let once = base.clone();
        base.overlay(
            &source,
            destination,
            canvas,
            Fill::Opaque(Style::PLAIN.bg(Color::Red)),
            CursorMerge::PreserveBase,
        );
        assert_eq!(base, once);
    }

    #[test]
    fn ascii_overlays_clip_on_every_edge_and_compose_in_order() {
        let mut base = surface("......\n......\n......");
        let source = surface("abcd\nefgh");
        base.overlay(
            &source,
            Rect::new(0, 0, 4, 2),
            Rect::new(1, 1, 2, 1),
            Fill::Transparent,
            CursorMerge::PreserveBase,
        );
        assert_eq!(base.plain_text(), "......\n.fg...\n......");

        for (row, col) in [(0, 0), (0, 4), (2, 0), (2, 4)] {
            base.overlay(
                &surface("XY"),
                Rect::new(row, col, 2, 1),
                Rect::new(0, 0, 6, 3),
                Fill::Transparent,
                CursorMerge::PreserveBase,
            );
        }
        base.overlay(
            &surface("Z"),
            Rect::new(2, 5, 1, 1),
            Rect::new(0, 0, 6, 3),
            Fill::Transparent,
            CursorMerge::PreserveBase,
        );
        assert_eq!(base.plain_text(), "XY..XY\n.fg...\nXY..XZ");
    }

    #[test]
    fn wide_source_cells_are_copied_whole_or_not_at_all_at_each_boundary() {
        for destination_width in 0..=3 {
            for canvas_left in 0..=3 {
                for canvas_width in 0..=4 {
                    let mut base = surface(".....");
                    let destination = Rect::new(0, 1, destination_width, 1);
                    let canvas = Rect::new(0, canvas_left, canvas_width, 1);
                    base.overlay(
                        &surface("界"),
                        destination,
                        canvas,
                        Fill::Transparent,
                        CursorMerge::PreserveBase,
                    );
                    let copied = destination_width >= 2
                        && canvas_left <= 1
                        && canvas_left.saturating_add(canvas_width) >= 3;
                    assert_eq!(
                        base.plain_text().contains('界'),
                        copied,
                        "destination={destination:?}, canvas={canvas:?}",
                    );
                    assert_eq!(base.row_width(0), 5);
                }
            }
        }
    }

    #[test]
    fn deterministic_composition_cases_preserve_surface_invariants() {
        let texts = ["", "a", "界", "e\u{301}", "🙂x", "a界b"];
        for base_text in texts {
            for source_text in texts {
                for row in 0..=2 {
                    for col in 0..=4 {
                        for width in 0..=4 {
                            let mut base = surface(base_text);
                            let original = base.clone();
                            let empty = Surface::new();
                            base.overlay(
                                &empty,
                                Rect::new(row, col, width, 2),
                                Rect::new(0, 0, 6, 3),
                                Fill::Transparent,
                                CursorMerge::PreserveBase,
                            );
                            assert_eq!(base, original, "transparent empty source is identity");

                            base.overlay(
                                &surface(source_text),
                                Rect::new(row, col, width, 2),
                                Rect::new(0, 0, 6, 3),
                                Fill::Opaque(Style::PLAIN.bg(Color::Blue)),
                                CursorMerge::PreserveBase,
                            );
                            for (row_index, rendered) in base.rows().iter().enumerate() {
                                assert!(rendered.cells().iter().all(|cell| cell.width > 0));
                                assert_eq!(
                                    rendered
                                        .cells()
                                        .iter()
                                        .map(|cell| cell.width)
                                        .sum::<usize>(),
                                    base.row_width(row_index),
                                );
                            }
                            let once = base.clone();
                            base.overlay(
                                &surface(source_text),
                                Rect::new(row, col, width, 2),
                                Rect::new(0, 0, 6, 3),
                                Fill::Opaque(Style::PLAIN.bg(Color::Blue)),
                                CursorMerge::PreserveBase,
                            );
                            assert_eq!(base, once, "opaque composition is idempotent");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn clipped_overlay_cursor_falls_back_to_the_base_cursor() {
        let mut source = surface("x");
        source.set_cursor(Position { row: 0, col: 0 });
        let mut base = surface("base");
        base.set_cursor(Position { row: 0, col: 2 });
        base.overlay(
            &source,
            Rect::new(4, 4, 1, 1),
            Rect::new(0, 0, 4, 2),
            Fill::Transparent,
            CursorMerge::PreferOverlay,
        );
        assert_eq!(base.cursor(), Some(Position { row: 0, col: 2 }));
    }

    #[test]
    fn overlay_cursor_may_follow_the_final_child_cell() {
        let mut source = surface("xy");
        source.set_cursor(Position { row: 0, col: 2 });
        let mut base = surface("....");
        base.overlay(
            &source,
            Rect::new(0, 1, 2, 1),
            Rect::new(0, 0, 4, 1),
            Fill::Transparent,
            CursorMerge::PreferOverlay,
        );
        assert_eq!(base.cursor(), Some(Position { row: 0, col: 3 }));
    }

    #[test]
    fn append_surface_preserves_soft_row_boundaries() {
        let mut source = surface("abc");
        source.soft_wrap();
        source.write("def", Style::PLAIN);
        let mut output = Surface::new();

        append_surface(&mut output, &source, source.height());

        assert_eq!(output.rows()[0].break_after(), RowBreak::Soft);
        assert_eq!(output.rows()[1].break_after(), RowBreak::None);
    }

    #[test]
    fn overlay_keeps_base_row_break_metadata() {
        let mut base = surface("abcd");
        base.rows[0].set_break_after(RowBreak::Soft);
        let mut source = surface("xy");
        source.rows[0].set_break_after(RowBreak::Hard);
        base.overlay(
            &source,
            Rect::new(0, 1, 2, 1),
            Rect::new(0, 0, 4, 1),
            Fill::Transparent,
            CursorMerge::PreserveBase,
        );
        assert_eq!(base.rows()[0].break_after(), RowBreak::Soft);
    }
}
