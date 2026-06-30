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

#[derive(Clone, Debug, Eq, PartialEq)]
struct RenderedFrame {
    logical:  Surface,
    physical: Surface,
}

pub struct Renderer<W> {
    writer:         W,
    previous:       Option<RenderedFrame>,
    frame:          u64,
    width:          Option<usize>,
    layout_mode:    LayoutMode,
    theme:          Theme,
    force_full:     bool,
    resize_pending: bool,
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
            layout_mode: LayoutMode::Clip,
            theme: Theme::DEFAULT,
            force_full: false,
            resize_pending: false,
        }
    }

    #[must_use]
    pub const fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    #[must_use]
    pub const fn layout_mode(mut self, mode: LayoutMode) -> Self {
        self.layout_mode = mode;
        self
    }

    pub const fn resize(&mut self, width: usize) {
        self.width = Some(width);
        self.resize_pending = true;
        if matches!(self.layout_mode, LayoutMode::Clip) {
            self.force_full = true;
        }
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
            &RenderCtx {
                frame: self.frame,
                width: self.width,
                theme: self.theme,
            },
            &mut next,
        );
        self.frame = self.frame.wrapping_add(1);
        self.draw_surface(next)
    }

    pub fn draw_surface(&mut self, next_logical: Surface) -> io::Result<RenderStats> {
        let next_physical = self.layout_surface(next_logical.clone());

        let previous_physical = self.previous.as_ref().map(|previous| {
            if self.resize_pending && matches!(self.layout_mode, LayoutMode::Wrap) {
                self.layout_surface(previous.logical.clone())
            } else {
                previous.physical.clone()
            }
        });

        if !self.force_full && previous_physical.as_ref() == Some(&next_physical) {
            self.previous = Some(RenderedFrame {
                logical:  next_logical,
                physical: next_physical,
            });
            self.resize_pending = false;
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
            move_to_top(&mut self.writer, final_position(previous), &mut cursor)?;
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
        self.writer.flush()?;
        self.previous = Some(RenderedFrame {
            logical:  next_logical,
            physical: next_physical,
        });
        self.resize_pending = false;
        Ok(stats)
    }

    pub fn clear(&mut self) -> io::Result<RenderStats> {
        let Some(previous) = self.previous.take() else {
            return Ok(RenderStats::default());
        };
        let previous_physical =
            if self.resize_pending && matches!(self.layout_mode, LayoutMode::Wrap) {
                self.layout_surface(previous.logical)
            } else {
                previous.physical
            };
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
        self.writer.flush()?;
        self.force_full = false;
        self.resize_pending = false;
        Ok(stats)
    }

    pub fn into_inner(self) -> W {
        self.writer
    }

    fn layout_surface(&self, surface: Surface) -> Surface {
        layout_surface(surface, self.width, self.layout_mode)
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

fn wrap_surface(surface: &Surface, terminal_width: usize) -> Surface {
    let max_columns = fitted_columns(terminal_width);
    let cursor = surface.cursor();
    let mut out = Surface::new();
    let mut first_physical_row = true;
    let mut physical_cursor = None;

    for (logical_row, row) in surface.rows().iter().enumerate() {
        if !first_physical_row {
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

fn fitted_columns(terminal_width: usize) -> usize {
    terminal_width.saturating_sub(1).max(1)
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
            (Some(old), Some(new)) if old == new => {},
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
    surface.cursor().unwrap_or_else(|| {
        Position {
            row: surface.height(),
            col: 0,
        }
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
