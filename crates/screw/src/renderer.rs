use std::io::{
    self,
    Write,
};

use crate::{
    Cell,
    Position,
    RenderCtx,
    Style,
    Surface,
    Theme,
    Widget,
    terminal_width_or_default,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderStats {
    pub changed_rows: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LayoutMode {
    #[default]
    Clip,
    Wrap,
}

/// Whether the renderer should preserve terminal cursor visibility or derive it
/// from the rendered surface's cursor anchor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CursorVisibility {
    /// Do not emit terminal cursor visibility controls.
    #[default]
    Preserve,
    /// Show the cursor when the surface has an anchor and hide it otherwise.
    FromSurface,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderedFrame {
    physical: Surface,
}

pub struct Renderer<W> {
    writer:         W,
    previous:       Option<RenderedFrame>,
    frame:          u64,
    width:          Option<usize>,
    height: Option<usize>,
    layout_mode:    LayoutMode,
    theme:          Theme,
    cursor_visibility: CursorVisibility,
    cursor_visible: Option<bool>,
    force_full: bool,
}

impl<W> Renderer<W>
where
    W: Write,
{
    pub const fn new(writer: W) -> Self {
        Self {
            writer,
            previous: None,
            frame: 0,
            width: None,
            height: None,
            layout_mode: LayoutMode::Clip,
            theme: Theme::DEFAULT,
            cursor_visibility: CursorVisibility::Preserve,
            cursor_visible: None,
            force_full: false,
        }
    }

    #[must_use]
    pub const fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    #[must_use]
    pub const fn height(mut self, height: usize) -> Self {
        self.height = Some(height);
        self
    }

    #[must_use]
    pub const fn layout_mode(mut self, mode: LayoutMode) -> Self {
        self.layout_mode = mode;
        self
    }

    /// Configure whether surface cursor intent controls terminal visibility.
    #[must_use]
    pub const fn cursor_visibility(mut self, visibility: CursorVisibility) -> Self {
        self.cursor_visibility = visibility;
        self
    }

    pub const fn resize(&mut self, width: usize) {
        self.width = Some(width);
        // Terminal resize reflow is emulator- and mode-dependent. Retained
        // state cannot safely infer the old physical frame at the new width.
        self.force_full = true;
    }

    pub const fn resize_viewport(&mut self, width: usize, height: usize) {
        self.height = Some(height);
        self.resize(width);
    }

    #[must_use]
    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn draw<T>(&mut self, widget: &T) -> io::Result<RenderStats>
    where
        T: Widget + ?Sized,
    {
        let mut next = Surface::new();
        widget.render(
            &RenderCtx::new()
                .with_frame(self.frame)
                .with_constraints(self.width.map(usable_columns), self.height)
                .with_layout_mode(self.layout_mode)
                .with_theme(self.theme),
            &mut next,
        );
        self.frame = self.frame.wrapping_add(1);
        self.draw_surface(next)
    }

    pub fn draw_surface(&mut self, next_logical: Surface) -> io::Result<RenderStats> {
        let next_physical = self.layout_surface(next_logical);

        let previous_physical = self
            .previous
            .as_ref()
            .map(|previous| previous.physical.clone());

        if !self.force_full && previous_physical.as_ref() == Some(&next_physical) {
            self.previous = Some(RenderedFrame {
                physical: next_physical,
            });
            return Ok(RenderStats::default());
        }

        let mut cursor = Cursor::default();
        let mut stats = RenderStats::default();

        if self.force_full {
            if let Some(previous) = &previous_physical {
                move_to_top(&mut self.writer, final_position(previous), &mut cursor)?;
                clear_surface(previous, &mut self.writer, &mut cursor, &mut stats)?;
                cursor.move_to(&mut self.writer, Position { row: 0, col: 0 })?;
            }
            write_initial_surface(&next_physical, &mut self.writer, &mut cursor, &mut stats)?;
            self.force_full = false;
        } else if let Some(previous) = &previous_physical {
            let from = extend_for_growth(previous, &next_physical, &mut self.writer, &mut cursor)?;
            move_to_top(&mut self.writer, from, &mut cursor)?;
            diff_surfaces(
                previous,
                &next_physical,
                &mut self.writer,
                &mut cursor,
                &mut stats,
            )?;
        } else {
            write_initial_surface(&next_physical, &mut self.writer, &mut cursor, &mut stats)?;
        }

        cursor.move_to(&mut self.writer, final_position(&next_physical))?;
        self.update_cursor_visibility(next_physical.cursor().is_some())?;
        self.writer.flush()?;
        self.previous = Some(RenderedFrame {
            physical: next_physical,
        });
        Ok(stats)
    }

    pub fn clear(&mut self) -> io::Result<RenderStats> {
        let Some(previous) = self.previous.take() else {
            return Ok(RenderStats::default());
        };
        let previous_physical = previous.physical;
        let mut cursor = Cursor::default();
        let mut stats = RenderStats::default();

        move_to_top(
            &mut self.writer,
            final_position(&previous_physical),
            &mut cursor,
        )?;
        clear_surface(
            &previous_physical,
            &mut self.writer,
            &mut cursor,
            &mut stats,
        )?;
        cursor.move_to(&mut self.writer, Position { row: 0, col: 0 })?;
        self.update_cursor_visibility(false)?;
        self.writer.flush()?;
        self.force_full = false;
        Ok(stats)
    }

    pub fn into_inner(self) -> W {
        self.writer
    }

    fn layout_surface(&self, surface: Surface) -> Surface {
        let mut surface = layout_surface(surface, self.width.map(usable_columns), self.layout_mode);
        if let Some(height) = self.height {
            surface.fit_height(height);
        }
        surface
    }

    fn update_cursor_visibility(&mut self, visible: bool) -> io::Result<()> {
        if !matches!(self.cursor_visibility, CursorVisibility::FromSurface)
            || self.cursor_visible == Some(visible)
        {
            return Ok(());
        }
        self.writer
            .write_all(if visible { b"\x1b[?25h" } else { b"\x1b[?25l" })?;
        self.cursor_visible = Some(visible);
        Ok(())
    }
}

impl Renderer<io::Stderr> {
    pub fn stderr() -> Self {
        Self::new(io::stderr()).width(terminal_width_or_default())
    }
}

pub fn layout_surface(mut surface: Surface, width: Option<usize>, mode: LayoutMode) -> Surface {
    match (width, mode) {
        (Some(width), LayoutMode::Clip) => {
            surface.fit_width(width);
            surface
        },
        (Some(width), LayoutMode::Wrap) => wrap_surface(&surface, width),
        (None, _) => surface,
    }
}

fn wrap_surface(surface: &Surface, max_columns: usize) -> Surface {
    let cursor = surface.cursor();
    let mut out = Surface::new();
    let mut first_physical_row = true;
    let mut physical_cursor = None;

    let mut previous_break = crate::RowBreak::None;
    for (logical_row, row) in surface.rows().iter().enumerate() {
        if !first_physical_row && previous_break != crate::RowBreak::Soft {
            out.newline();
        }
        first_physical_row = false;
        let mut logical_col = 0_usize;
        let cursor_on_row = cursor.filter(|cursor| cursor.row == logical_row);

        if row.is_empty() {
            if cursor_on_row.is_some_and(|cursor| cursor.col == 0) {
                physical_cursor = Some(Position {
                    row: out.height().saturating_sub(1),
                    col: 0,
                });
            }
            previous_break = row.break_after();
            continue;
        }

        for cell in row.cells() {
            if cell.width > max_columns {
                if cursor_crosses_cell(cursor_on_row, logical_col, cell.width) {
                    physical_cursor = Some(Position {
                        row: out.height().saturating_sub(1),
                        col: out.current_col(),
                    });
                }
                logical_col += cell.width;
                continue;
            }

            if out.current_col() > 0 && out.current_col() + cell.width > max_columns {
                out.soft_wrap();
            }

            if cursor_crosses_cell(cursor_on_row, logical_col, cell.width) {
                physical_cursor = Some(Position {
                    row: out.height().saturating_sub(1),
                    col: out.current_col() + cursor_on_row.unwrap_or_default().col - logical_col,
                });
            }

            out.write(&cell.text, cell.style);
            logical_col += cell.width;
        }

        if cursor_on_row.is_some_and(|cursor| cursor.col >= logical_col) {
            physical_cursor = Some(Position {
                row: out.height().saturating_sub(1),
                col: out.current_col(),
            });
        }
        previous_break = row.break_after();
    }

    if let Some(cursor) = physical_cursor {
        out.set_cursor(cursor);
    }
    out
}

fn cursor_crosses_cell(cursor: Option<Position>, logical_col: usize, cell_width: usize) -> bool {
    cursor.is_some_and(|cursor| {
        cursor.col >= logical_col && cursor.col < logical_col.saturating_add(cell_width)
    })
}

pub const fn usable_columns(terminal_columns: usize) -> usize {
    if terminal_columns > 1 {
        terminal_columns - 1
    } else {
        1
    }
}

/// Ensure every physical row addressed by the next diff exists.
///
/// Cursor movement cannot create terminal rows: moving below the bottom edge
/// simply clamps. A taller retained frame must therefore append real newlines
/// before the renderer moves back to its origin and patches the new rows.
fn extend_for_growth(
    previous: &Surface,
    next: &Surface,
    writer: &mut impl Write,
    cursor: &mut Cursor,
) -> io::Result<Position> {
    let previous_final = final_position(previous);
    let previous_bottom = allocated_bottom(previous);
    let next_bottom = allocated_bottom(next);
    if next_bottom <= previous_bottom {
        return Ok(previous_final);
    }

    *cursor = Cursor {
        row: previous_final.row,
        col: previous_final.col,
        style: Style::default(),
    };
    cursor.move_to(
        writer,
        Position {
            row: previous_bottom,
            col: 0,
        },
    )?;
    for _ in previous_bottom..next_bottom {
        writer.write_all(b"\r\n")?;
        cursor.row += 1;
        cursor.col = 0;
    }
    Ok(Position {
        row: next_bottom,
        col: 0,
    })
}

const fn allocated_bottom(surface: &Surface) -> usize {
    surface.height().saturating_sub(1)
}

fn move_to_top(writer: &mut impl Write, from: Position, cursor: &mut Cursor) -> io::Result<()> {
    writer.write_all(b"\r")?;
    if from.row > 0 {
        write!(writer, "\x1b[{}A", from.row)?;
    }
    *cursor = Cursor::default();
    Ok(())
}

fn write_initial_surface(
    surface: &Surface,
    writer: &mut impl Write,
    cursor: &mut Cursor,
    stats: &mut RenderStats,
) -> io::Result<()> {
    let final_position = final_position(surface);
    for (row_index, row) in surface.rows().iter().enumerate() {
        write_row_tail(writer, cursor, row.cells(), 0)?;
        writer.write_all(b"\x1b[K")?;
        stats.changed_rows += 1;

        let should_create_next_line =
            row_index + 1 < surface.height() || final_position.row > row_index;
        if should_create_next_line {
            writer.write_all(b"\r\n")?;
            cursor.row += 1;
            cursor.col = 0;
            cursor.style = Style::default();
        }
    }
    Ok(())
}

fn diff_surfaces(
    previous: &Surface,
    next: &Surface,
    writer: &mut impl Write,
    cursor: &mut Cursor,
    stats: &mut RenderStats,
) -> io::Result<()> {
    let rows = previous.height().max(next.height());
    for row_index in 0..rows {
        match (previous.rows().get(row_index), next.rows().get(row_index)) {
            (Some(old), Some(new)) if old.cells() == new.cells() => {},
            (Some(old), Some(new)) => {
                patch_row(writer, cursor, row_index, old.cells(), new.cells())?;
                stats.changed_rows += 1;
            },
            (Some(_), None) => {
                cursor.move_to(writer, Position {
                    row: row_index,
                    col: 0,
                })?;
                writer.write_all(b"\x1b[2K")?;
                stats.changed_rows += 1;
            },
            (None, Some(new)) => {
                cursor.move_to(writer, Position {
                    row: row_index,
                    col: 0,
                })?;
                write_row_tail(writer, cursor, new.cells(), 0)?;
                writer.write_all(b"\x1b[K")?;
                stats.changed_rows += 1;
            },
            (None, None) => {},
        }
    }
    Ok(())
}

fn clear_surface(
    surface: &Surface,
    writer: &mut impl Write,
    cursor: &mut Cursor,
    stats: &mut RenderStats,
) -> io::Result<()> {
    for row_index in 0..surface.height() {
        cursor.move_to(writer, Position {
            row: row_index,
            col: 0,
        })?;
        writer.write_all(b"\x1b[2K")?;
        stats.changed_rows += 1;
    }
    Ok(())
}

fn patch_row(
    writer: &mut impl Write,
    cursor: &mut Cursor,
    row_index: usize,
    old: &[Cell],
    new: &[Cell],
) -> io::Result<()> {
    let prefix = common_prefix(old, new);
    if prefix == old.len() && prefix == new.len() {
        return Ok(());
    }

    let suffix = common_suffix(&old[prefix..], &new[prefix..]);
    let old_changed_width = cells_width(&old[prefix..old.len() - suffix]);
    let new_changed_width = cells_width(&new[prefix..new.len() - suffix]);
    let can_patch_middle = suffix > 0 && old_changed_width == new_changed_width;
    let end = if can_patch_middle {
        new.len() - suffix
    } else {
        new.len()
    };
    let col = cells_width(&new[..prefix]);

    cursor.move_to(writer, Position {
        row: row_index,
        col,
    })?;
    write_row_tail(writer, cursor, &new[..end], prefix)?;

    if !can_patch_middle && cells_width(old) > cells_width(new) {
        writer.write_all(b"\x1b[K")?;
    }

    Ok(())
}

fn write_row_tail(
    writer: &mut impl Write,
    cursor: &mut Cursor,
    row: &[Cell],
    start: usize,
) -> io::Result<()> {
    for cell in &row[start..] {
        cursor.set_style(writer, cell.style)?;
        writer.write_all(cell.text.as_bytes())?;
        cursor.col += cell.width;
    }
    cursor.set_style(writer, Style::default())
}

fn common_prefix(old: &[Cell], new: &[Cell]) -> usize {
    old.iter()
        .zip(new)
        .take_while(|(old, new)| old == new)
        .count()
}

fn common_suffix(old: &[Cell], new: &[Cell]) -> usize {
    old.iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(old, new)| old == new)
        .count()
}

fn cells_width(cells: &[Cell]) -> usize {
    cells.iter().map(|cell| cell.width).sum()
}

fn final_position(surface: &Surface) -> Position {
    surface.cursor().unwrap_or_else(|| Position {
        row: surface.height().saturating_sub(1),
        col: surface.row_width(surface.height().saturating_sub(1)),
    })
}

#[derive(Clone, Copy, Debug, Default)]
struct Cursor {
    row:   usize,
    col:   usize,
    style: Style,
}

impl Cursor {
    fn move_to(&mut self, writer: &mut impl Write, target: Position) -> io::Result<()> {
        self.set_style(writer, Style::default())?;

        if target.row > self.row {
            write!(writer, "\x1b[{}B", target.row - self.row)?;
        } else if target.row < self.row {
            write!(writer, "\x1b[{}A", self.row - target.row)?;
        }

        if target.col == 0 {
            writer.write_all(b"\r")?;
        } else if target.col > self.col {
            write!(writer, "\x1b[{}C", target.col - self.col)?;
        } else if target.col < self.col {
            write!(writer, "\x1b[{}D", self.col - target.col)?;
        }

        self.row = target.row;
        self.col = target.col;
        Ok(())
    }

    fn set_style(&mut self, writer: &mut impl Write, style: Style) -> io::Result<()> {
        if self.style != style {
            writer.write_all(style.sgr().as_bytes())?;
            self.style = style;
        }
        Ok(())
    }
}
