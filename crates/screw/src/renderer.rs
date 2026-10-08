// SPDX-License-Identifier: EUPL-1.2

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
    terminal::stderr_size,
};

/// Counters describing one draw.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RenderStats {
    /// Rows that were rewritten, zero when the frame matched the previous one.
    pub changed_rows: usize,
}

/// How a surface wider than the terminal is fitted.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LayoutMode {
    /// Cut each row at the terminal width.
    #[default]
    Clip,
    /// Continue overlong rows on the next line, preserving them as one logical line.
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

/// Draws widgets to a terminal stream, writing only the rows that changed since the last draw.
///
/// The renderer is synchronous and driven by the caller. Use a [`Runtime`](crate::Runtime) to get
/// frame pacing and
/// a background thread. The writer should be a terminal, because frames are updated in place with
/// cursor movement.
#[derive(Debug)]
pub struct Renderer<W> {
    writer:         W,
    previous:       Option<Surface>,
    frame:          u64,
    width:          Option<usize>,
    height: Option<usize>,
    layout_mode:    LayoutMode,
    theme:          Theme,
    cursor_visibility: CursorVisibility,
    cursor_visible: Option<bool>,
    pending_cursor: Option<bool>,
    force_full: bool,
    rendition_uncertain: bool,
}

impl<W> Renderer<W>
where
    W: Write,
{
    /// Creates a renderer over `writer` with no width or height limit, clipping, the default theme
    /// and untouched cursor visibility.
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
            pending_cursor: None,
            force_full: false,
            rendition_uncertain: false,
        }
    }

    /// Sets the terminal width in columns, which rows are clipped or wrapped to.
    #[must_use]
    pub const fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets the terminal height in rows, which the frame is clipped to.
    #[must_use]
    pub const fn height(mut self, height: usize) -> Self {
        self.height = Some(height);
        self
    }

    /// Chooses between clipping and wrapping overlong rows.
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

    /// Changes the width, and redraws the whole frame on the next draw because terminals reflow
    /// earlier output unpredictably.
    pub const fn resize(&mut self, width: usize) {
        if matches!(self.width, Some(current) if current == width) {
            return;
        }
        self.width = Some(width);
        // Terminal resize reflow is emulator- and mode-dependent, so the retained
        // frame's rewrapped position at the new width is only ever an estimate.
        self.force_full = true;
    }

    /// Changes the width and the height. The next draw repaints the whole frame when the width
    /// changed or the retained frame is taller than the new height, because a shorter terminal
    /// scrolls the top of a tall frame into scrollback.
    pub fn resize_viewport(&mut self, width: usize, height: usize) {
        if self.previous.as_ref().is_some_and(|previous| previous.height() > height) {
            self.force_full = true;
        }
        self.height = Some(height);
        self.resize(width);
    }

    /// Sets the theme that resolves [`Role`](crate::Role) styles.
    #[must_use]
    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Renders `widget` and writes the difference from the previous frame.
    ///
    /// The frame counter seen by widgets through [`RenderCtx::frame`] advances on every call.
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

    /// Writes an already rendered surface after fitting it to the width and height.
    pub fn draw_surface(&mut self, next_logical: Surface) -> io::Result<RenderStats> {
        let next_physical = self.layout_surface(next_logical);

        if !self.force_full && self.previous.as_ref() == Some(&next_physical) {
            self.previous = Some(next_physical);
            return Ok(RenderStats::default());
        }

        // A write failure here leaves the terminal cursor somewhere inside an
        // unfinished frame, so the retained anchor can no longer be trusted to
        // climb from on the next draw.
        match self.write_frame(&next_physical) {
            Ok(stats) => {
                self.previous = Some(next_physical);
                Ok(stats)
            },
            Err(error) => {
                self.previous = None;
                self.rendition_uncertain = true;
                Err(error)
            },
        }
    }

    /// After a failed write the terminal may still hold the style of a half written cell, so the
    /// next write starts by resetting it.
    fn settle_rendition(&mut self) -> io::Result<()> {
        if self.rendition_uncertain {
            self.writer.write_all(Style::default().sgr().as_bytes())?;
        }
        Ok(())
    }

    fn write_frame(&mut self, next_physical: &Surface) -> io::Result<RenderStats> {
        self.settle_rendition()?;
        let mut cursor = Cursor::default();
        let mut stats = RenderStats::default();

        if self.force_full {
            if let Some(previous) = self.previous.as_ref() {
                clear_reflowed(previous, self.width, &mut self.writer, &mut cursor, &mut stats)?;
            }
            write_initial_surface(next_physical, &mut self.writer, &mut cursor, &mut stats)?;
            self.force_full = false;
        } else if let Some(previous) = self.previous.as_ref() {
            let from = extend_for_growth(previous, next_physical, &mut self.writer, &mut cursor)?;
            move_to_top(&mut self.writer, from, &mut cursor)?;
            diff_surfaces(
                previous,
                next_physical,
                &mut self.writer,
                &mut cursor,
                &mut stats,
            )?;
        } else {
            write_initial_surface(next_physical, &mut self.writer, &mut cursor, &mut stats)?;
        }

        cursor.move_to(&mut self.writer, final_position(next_physical))?;
        self.update_cursor_visibility(next_physical.cursor().is_some())?;
        self.flush_retrying_interrupts()?;
        Ok(stats)
    }

    fn flush_retrying_interrupts(&mut self) -> io::Result<()> {
        loop {
            match self.writer.flush() {
                Ok(()) => {
                    self.rendition_uncertain = false;
                    if let Some(visible) = self.pending_cursor.take() {
                        self.cursor_visible = Some(visible);
                    }
                    return Ok(());
                },
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
                Err(error) => return Err(error),
            }
        }
    }

    /// Erases the last frame from the terminal and forgets it.
    pub fn clear(&mut self) -> io::Result<RenderStats> {
        let reset_pending = self.rendition_uncertain;
        let result = self.settle_rendition().and_then(|()| self.clear_frame(reset_pending));
        if result.is_err() {
            self.rendition_uncertain = true;
        }
        result
    }

    fn clear_frame(&mut self, reset_written: bool) -> io::Result<RenderStats> {
        let Some(previous_physical) = self.previous.take() else {
            if self.cursor_may_be_hidden() {
                self.restore_cursor()?;
            } else if reset_written {
                self.flush_retrying_interrupts()?;
            }
            return Ok(RenderStats::default());
        };
        let mut cursor = Cursor::default();
        let mut stats = RenderStats::default();

        if self.force_full {
            clear_reflowed(
                &previous_physical,
                self.width,
                &mut self.writer,
                &mut cursor,
                &mut stats,
            )?;
        } else {
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
        }
        self.restore_cursor()?;
        self.flush_retrying_interrupts()?;
        self.force_full = false;
        Ok(stats)
    }

    /// Returns the writer without touching the terminal.
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

    /// Show the cursor again if this renderer hid it under `FromSurface`.
    pub fn restore_cursor(&mut self) -> io::Result<()> {
        let settled = self.rendition_uncertain;
        self.settle_rendition()?;
        if self.cursor_may_be_hidden() {
            self.update_cursor_visibility(true)?;
            self.flush_retrying_interrupts()?;
        } else if settled {
            self.flush_retrying_interrupts()?;
        }
        Ok(())
    }

    fn cursor_may_be_hidden(&self) -> bool {
        matches!(self.cursor_visibility, CursorVisibility::FromSurface)
            && (self.cursor_visible == Some(false) || self.pending_cursor == Some(false))
    }

    fn update_cursor_visibility(&mut self, visible: bool) -> io::Result<()> {
        if !matches!(self.cursor_visibility, CursorVisibility::FromSurface)
            || (self.pending_cursor.is_none() && self.cursor_visible == Some(visible))
        {
            return Ok(());
        }
        self.writer
            .write_all(if visible { b"\x1b[?25h" } else { b"\x1b[?25l" })?;
        self.pending_cursor = Some(visible);
        Ok(())
    }
}

impl Renderer<io::Stderr> {
    /// Creates a renderer on standard error sized to the terminal, or 80 columns when the size is
    /// unknown.
    pub fn stderr() -> Self {
        let (width, height) = stderr_size();
        let mut renderer = Self::new(io::stderr()).width(width);
        renderer.height = height;
        renderer
    }
}

pub(crate) fn layout_surface(mut surface: Surface, width: Option<usize>, mode: LayoutMode) -> Surface {
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
            if let Some(cursor) = cursor_on_row {
                physical_cursor = Some(Position {
                    row: out.height().saturating_sub(1),
                    col: cursor.col.min(max_columns),
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

pub(crate) const fn usable_columns(terminal_columns: usize) -> usize {
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

/// Reflowing terminals rewrap each hard-broken row from its original width, so
/// after any sequence of resizes the frame top is the sum of those rewraps
/// above the anchor. The erase below catches lines the estimate misses.
fn clear_reflowed(
    previous: &Surface,
    width: Option<usize>,
    writer: &mut impl Write,
    cursor: &mut Cursor,
    stats: &mut RenderStats,
) -> io::Result<()> {
    let anchor = final_position(previous);
    let row = width.filter(|&width| width > 0).map_or(anchor.row, |width| {
        previous
            .rows()
            .iter()
            .take(anchor.row)
            .map(|row| wrapped_lines(row.cells(), width))
            .sum::<usize>()
            + previous
                .rows()
                .get(anchor.row)
                .map_or(0, |row| wraps_before_column(row.cells(), width, anchor.col))
    });
    move_to_top(writer, Position { row, col: 0 }, cursor)?;
    writer.write_all(b"\x1b[J")?;
    stats.changed_rows += previous.height();
    Ok(())
}

fn wrapped_lines(cells: &[Cell], width: usize) -> usize {
    1 + wraps_before_column(cells, width, usize::MAX)
}

/// Count the wraps of `cells` at `width` before `column`, so a wide cell that straddles a
/// wrap boundary is not rounded away by dividing the column by `width`.
fn wraps_before_column(cells: &[Cell], width: usize, column: usize) -> usize {
    let mut wraps = 0;
    let mut used = 0;
    let mut consumed = 0;
    for cell in cells {
        if consumed >= column {
            break;
        }
        if used + cell.width > width {
            wraps += 1;
            used = 0;
        }
        used += cell.width;
        consumed += cell.width;
    }
    wraps
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

#[cfg(test)]
mod tests {
    use std::io;

    use crate::{
        CursorMerge, CursorVisibility, Edge, Fill, Floating, Insets, Layers, LayoutMode, Position,
        Renderer, Size, Style, Surface, Widget, renderer::layout_surface,
    };

    fn surface(lines: &[&str], cursor: Option<Position>) -> Surface {
        let mut surface = Surface::new();
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                surface.newline();
            }
            surface.write(line, Style::default());
        }
        if let Some(cursor) = cursor {
            surface.set_cursor(cursor);
        }
        surface
    }

    #[test]
    fn a_hard_break_after_an_empty_soft_continuation_row_starts_a_new_row() {
        let mut logical = Surface::new();
        logical.write("ab", Style::default());
        logical.soft_wrap();
        logical.newline();
        logical.write("c", Style::default());
        let laid_out = layout_surface(logical, Some(20), LayoutMode::Wrap);
        assert_eq!(laid_out.plain_text(), "ab\nc");
    }

    #[test]
    fn clear_without_a_retained_frame_still_restores_the_cursor() {
        let mut renderer = Renderer::new(Vec::new()).cursor_visibility(CursorVisibility::FromSurface);
        renderer.cursor_visible = Some(false);
        renderer.clear().unwrap();
        assert_eq!(renderer.into_inner(), b"\x1b[?25h");
    }

    #[test]
    fn growing_frame_allocates_rows_before_diffing_them() {
        let mut renderer = Renderer::new(Vec::new());
        renderer
            .draw_surface(surface(&["one", "two"], None))
            .unwrap();
        let before = renderer.writer.len();

        renderer
            .draw_surface(surface(&["one", "two", "three", "four"], None))
            .unwrap();

        let update = &renderer.writer[before..];
        assert!(
            update.starts_with(b"\r\r\n\r\n\r\x1b[3A"),
            "taller diff must create two rows before moving to the top: {update:?}"
        );
    }

    #[test]
    fn shrinking_cursor_anchored_frame_clears_every_removed_row() {
        let mut renderer = Renderer::new(Vec::new());
        renderer
            .draw_surface(surface(
                &["search: ", "alpha", "bravo", "charlie", "help"],
                Some(Position { row: 0, col: 8 }),
            ))
            .unwrap();
        let before = renderer.writer.len();

        renderer
            .draw_surface(surface(
                &["search: c", "charlie", "help"],
                Some(Position { row: 0, col: 9 }),
            ))
            .unwrap();

        let update = &renderer.writer[before..];
        assert_eq!(
            update
                .windows(b"\x1b[2K".len())
                .filter(|part| *part == b"\x1b[2K")
                .count(),
            2,
            "both removed result rows must be erased: {update:?}"
        );
    }

    #[test]
    fn cursor_visibility_can_follow_surface_intent() {
        let mut renderer =
            Renderer::new(Vec::new()).cursor_visibility(CursorVisibility::FromSurface);
        renderer
            .draw_surface(surface(&["search: "], Some(Position { row: 0, col: 8 })))
            .unwrap();
        renderer.draw_surface(surface(&["done"], None)).unwrap();

        assert_eq!(
            renderer
                .writer
                .windows(b"\x1b[?25h".len())
                .filter(|part| *part == b"\x1b[?25h")
                .count(),
            1
        );
        assert_eq!(
            renderer
                .writer
                .windows(b"\x1b[?25l".len())
                .filter(|part| *part == b"\x1b[?25l")
                .count(),
            1
        );
    }

    #[test]
    fn cursor_visibility_is_preserved_by_default() {
        let mut renderer = Renderer::new(Vec::new());
        renderer
            .draw_surface(surface(&["search: "], Some(Position { row: 0, col: 8 })))
            .unwrap();

        assert!(!renderer.writer.windows(6).any(|part| part == b"\x1b[?25"));
    }

    #[test]
    fn renderer_height_clips_rows_and_an_outside_cursor() {
        let renderer = Renderer::new(Vec::new()).height(2);
        let physical = renderer.layout_surface(surface(
            &["one", "two", "three"],
            Some(Position { row: 2, col: 1 }),
        ));

        assert_eq!(physical.height(), 2);
        assert_eq!(physical.cursor(), None);
    }

    #[test]
    fn usable_width_reserves_the_terminal_final_column_once() {
        assert_eq!(super::usable_columns(0), 1);
        assert_eq!(super::usable_columns(1), 1);
        assert_eq!(super::usable_columns(2), 1);
        assert_eq!(super::usable_columns(80), 79);

        let renderer = Renderer::new(Vec::new()).width(6);
        let physical = renderer.layout_surface(surface(&["abcdef"], None));
        assert_eq!(physical.plain_text(), "abcde");
    }

    #[test]
    fn removing_a_floating_pane_clears_its_materialized_canvas_rows() {
        let mut renderer = Renderer::new(Vec::new()).width(21).height(6);
        renderer.draw(&Layers::new("document")).unwrap();
        renderer
            .draw(
                &Layers::new("document").float(
                    "panel",
                    Floating::new(Edge::BOTTOM | Edge::RIGHT)
                        .margin(Insets::bottom(1))
                        .max_size(Size::new(10, 3))
                        .fill(Fill::Opaque(Style::PLAIN))
                        .cursor(CursorMerge::PreserveBase),
                ),
            )
            .unwrap();
        let before = renderer.writer.len();

        let stats = renderer.draw(&Layers::new("document")).unwrap();
        let update = &renderer.writer[before..];
        assert_eq!(stats.changed_rows, 4);
        assert_eq!(
            update
                .windows(b"\x1b[2K".len())
                .filter(|part| *part == b"\x1b[2K")
                .count(),
            4
        );
    }

    #[test]
    fn unchanged_floating_frame_uses_the_retained_fast_path() {
        let pane = || {
            Layers::new("document").float(
                "panel",
                Floating::new(Edge::BOTTOM | Edge::RIGHT).fill(Fill::Opaque(Style::PLAIN)),
            )
        };
        let mut renderer = Renderer::new(Vec::new()).width(21).height(6);
        renderer.draw(&pane()).unwrap();
        assert_eq!(renderer.draw(&pane()).unwrap().changed_rows, 0);
    }

    #[test]
    fn shrinking_a_pane_restores_every_vacated_cell() {
        let mut renderer = Renderer::new(Vec::new()).width(21).height(5);
        renderer
            .draw(&Layers::new("underlying document").float(
                "large pane",
                Floating::new(Edge::BOTTOM | Edge::RIGHT).fill(Fill::Opaque(Style::PLAIN)),
            ))
            .unwrap();
        let before = renderer.writer.len();
        let stats = renderer
            .draw(&Layers::new("underlying document").float(
                "x",
                Floating::new(Edge::BOTTOM | Edge::RIGHT).fill(Fill::Opaque(Style::PLAIN)),
            ))
            .unwrap();

        let physical = renderer.previous.as_ref().unwrap();
        assert!(!physical.plain_text().contains("large pane"));
        assert_eq!(text_at(physical, Position { row: 4, col: 19 }), Some("x"));
        assert_eq!(stats.changed_rows, 1);
        assert!(renderer.writer.len() > before);
    }

    #[test]
    fn resize_remeasures_and_reanchors_a_floating_child() {
        let pane = || {
            Layers::new("document").float(
                "panel",
                Floating::new(Edge::BOTTOM | Edge::RIGHT).fill(Fill::Opaque(Style::PLAIN)),
            )
        };
        let mut renderer = Renderer::new(Vec::new()).width(21).height(6);
        renderer.draw(&pane()).unwrap();
        assert_eq!(
            text_at(
                renderer.previous.as_ref().unwrap(),
                Position { row: 5, col: 15 },
            ),
            Some("p"),
        );

        renderer.resize_viewport(11, 4);
        renderer.draw(&pane()).unwrap();
        let smaller = renderer.previous.as_ref().unwrap();
        assert!(smaller.height() <= 4);
        assert!(smaller.display_width() <= 10);
        assert_eq!(text_at(smaller, Position { row: 3, col: 5 }), Some("p"));

        renderer.resize_viewport(31, 8);
        renderer.draw(&pane()).unwrap();
        let larger = renderer.previous.as_ref().unwrap();
        assert!(larger.height() <= 8);
        assert!(larger.display_width() <= 30);
        assert_eq!(text_at(larger, Position { row: 7, col: 25 }), Some("p"));
    }

    #[test]
    fn moving_a_floating_pane_restores_its_old_footprint() {
        let base = "01234567890123456789\nabcdefghijklmnopqrst\nABCDEFGHIJKLMNOPQRST";
        let mut renderer = Renderer::new(Vec::new()).width(21).height(3);
        renderer
            .draw(&Layers::new(base).float(
                "pane",
                Floating::new(Edge::BOTTOM | Edge::RIGHT).fill(Fill::Opaque(Style::PLAIN)),
            ))
            .unwrap();

        let stats = renderer
            .draw(&Layers::new(base).float(
                "pane",
                Floating::new(Edge::BOTTOM | Edge::LEFT).fill(Fill::Opaque(Style::PLAIN)),
            ))
            .unwrap();
        let physical = renderer.previous.as_ref().unwrap();

        assert_eq!(stats.changed_rows, 1);
        assert_eq!(text_at(physical, Position { row: 2, col: 0 }), Some("p"));
        assert_eq!(text_at(physical, Position { row: 2, col: 16 }), Some("Q"));
        assert_eq!(
            physical.plain_text(),
            "01234567890123456789\nabcdefghijklmnopqrst\npaneEFGHIJKLMNOPQRST"
        );
    }

    #[test]
    fn resize_clips_and_restores_surface_cursor_visibility() {
        let mut renderer = Renderer::new(Vec::new())
            .width(10)
            .height(2)
            .cursor_visibility(CursorVisibility::FromSurface);
        let frame = || surface(&["search:", "value"], Some(Position { row: 1, col: 5 }));

        renderer.draw_surface(frame()).unwrap();
        renderer.resize_viewport(10, 1);
        renderer.draw_surface(frame()).unwrap();
        renderer.resize_viewport(10, 2);
        renderer.draw_surface(frame()).unwrap();

        assert_eq!(
            renderer
                .writer
                .windows(b"\x1b[?25h".len())
                .filter(|part| *part == b"\x1b[?25h")
                .count(),
            2,
            "cursor transitions: {:?}",
            String::from_utf8_lossy(&renderer.writer),
        );
        assert_eq!(
            renderer
                .writer
                .windows(b"\x1b[?25l".len())
                .filter(|part| *part == b"\x1b[?25l")
                .count(),
            1,
        );
        assert_eq!(
            renderer.previous.as_ref().unwrap().cursor(),
            Some(Position { row: 1, col: 5 }),
        );
    }

    #[test]
    fn repeated_viewport_resizes_rewrap_logical_content_to_each_new_width() {
        let mut renderer = Renderer::new(Vec::new())
            .width(6)
            .height(6)
            .layout_mode(crate::LayoutMode::Wrap);
        let logical = || surface(&["abcdefghij"], None);

        renderer.draw_surface(logical()).unwrap();
        assert_eq!(
            renderer.previous.as_ref().unwrap().plain_text(),
            "abcde\nfghij",
        );

        renderer.resize_viewport(4, 6);
        renderer.draw_surface(logical()).unwrap();
        assert_eq!(
            renderer.previous.as_ref().unwrap().plain_text(),
            "abc\ndef\nghi\nj",
        );

        renderer.resize_viewport(7, 6);
        let before = renderer.writer.len();
        let stats = renderer.draw_surface(logical()).unwrap();
        assert_eq!(
            renderer.previous.as_ref().unwrap().plain_text(),
            "abcdef\nghij",
        );
        assert!(stats.changed_rows > 0);
        assert!(renderer.writer.len() > before);
    }

    #[test]
    fn rewrapping_consumes_soft_boundaries_but_preserves_hard_ones() {
        let mut prewrapped = surface(&["abc"], None);
        prewrapped.soft_wrap();
        prewrapped.write("def", Style::PLAIN);
        assert_eq!(
            layout_surface(prewrapped, Some(10), LayoutMode::Wrap).plain_text(),
            "abcdef",
        );

        let hard = surface(&["abc", "def"], None);
        assert_eq!(
            layout_surface(hard, Some(10), LayoutMode::Wrap).plain_text(),
            "abc\ndef",
        );
    }

    struct CursorDocument;

    impl Widget for CursorDocument {
        fn render(&self, _ctx: &crate::RenderCtx, out: &mut Surface) {
            out.write("document", Style::PLAIN);
            out.set_cursor(Position { row: 0, col: 3 });
        }
    }

    #[test]
    fn display_only_pane_keeps_cursor_visibility_stable() {
        let mut renderer = Renderer::new(Vec::new())
            .width(21)
            .height(5)
            .cursor_visibility(CursorVisibility::FromSurface);
        for text in ["one", "different pane", "x"] {
            renderer
                .draw(
                    &Layers::new(CursorDocument).float(
                        text,
                        Floating::new(Edge::BOTTOM | Edge::RIGHT)
                            .fill(Fill::Opaque(Style::PLAIN))
                            .cursor(CursorMerge::PreserveBase),
                    ),
                )
                .unwrap();
            assert_eq!(
                renderer.previous.as_ref().unwrap().cursor(),
                Some(Position { row: 0, col: 3 }),
            );
        }
        assert_eq!(
            renderer
                .writer
                .windows(b"\x1b[?25h".len())
                .filter(|part| *part == b"\x1b[?25h")
                .count(),
            1,
        );
        assert!(!renderer.writer.windows(6).any(|part| part == b"\x1b[?25l"));
    }

    #[test]
    fn explicit_teardown_restores_a_hidden_surface_cursor() {
        let mut renderer =
            Renderer::new(Vec::new()).cursor_visibility(CursorVisibility::FromSurface);
        renderer
            .draw_surface(surface(&["search: "], Some(Position { row: 0, col: 8 })))
            .unwrap();
        renderer.draw_surface(surface(&["done"], None)).unwrap();
        let hidden = renderer
            .writer
            .windows(b"\x1b[?25l".len())
            .filter(|part| *part == b"\x1b[?25l")
            .count();
        assert_eq!(hidden, 1);

        renderer.clear().unwrap();
        let shown = renderer
            .writer
            .windows(b"\x1b[?25h".len())
            .filter(|part| *part == b"\x1b[?25h")
            .count();
        assert_eq!(
            shown, 2,
            "teardown must return the terminal to a visible cursor"
        );
    }

    #[test]
    fn explicit_teardown_leaves_preserved_visibility_alone() {
        let mut renderer = Renderer::new(Vec::new());
        renderer
            .draw_surface(surface(&["search: "], Some(Position { row: 0, col: 8 })))
            .unwrap();
        renderer.clear().unwrap();
        assert!(!renderer.writer.windows(6).any(|part| part == b"\x1b[?25"));
    }

    #[test]
    fn narrowing_climbs_to_the_rewrapped_frame_top() {
        let mut renderer = Renderer::new(Vec::new()).width(10);
        let frame = || surface(&["abcdefghij", "xy"], None);
        renderer.draw_surface(frame()).unwrap();
        renderer.resize(4);
        let before = renderer.writer.len();
        renderer.draw_surface(frame()).unwrap();
        let update = &renderer.writer[before..];
        assert!(update.starts_with(b"\r\x1b[3A\x1b[J"), "{update:?}");
        assert!(!update.windows(4).any(|part| part == b"\x1b[2K"));
    }

    #[test]
    fn widening_keeps_the_hard_broken_row_count() {
        let mut renderer = Renderer::new(Vec::new()).width(4);
        let frame = || surface(&["abcd", "xy"], None);
        renderer.draw_surface(frame()).unwrap();
        renderer.resize(10);
        let before = renderer.writer.len();
        renderer.draw_surface(frame()).unwrap();
        assert!(renderer.writer[before..].starts_with(b"\r\x1b[1A\x1b[J"));
    }

    #[test]
    fn unchanged_width_resize_keeps_diffing() {
        let mut renderer = Renderer::new(Vec::new()).width(10);
        renderer.draw_surface(surface(&["abc"], None)).unwrap();
        renderer.resize_viewport(10, 5);
        let stats = renderer.draw_surface(surface(&["abc"], None)).unwrap();
        assert_eq!(stats.changed_rows, 0);
    }

    fn text_at(surface: &Surface, wanted: Position) -> Option<&str> {
        let row = surface.rows().get(wanted.row)?;
        let mut col = 0;
        for cell in row.cells() {
            if col == wanted.col {
                return Some(cell.text.as_str());
            }
            col += cell.width;
        }
        None
    }

    #[derive(Default)]
    struct FlakyWriter {
        buffer: Vec<u8>,
        writes_until_failure: Option<usize>,
        interrupted_flushes: usize,
    }

    impl io::Write for FlakyWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if let Some(remaining) = &mut self.writes_until_failure {
                if *remaining == 0 {
                    self.writes_until_failure = None;
                    return Err(io::Error::other("simulated write failure"));
                }
                *remaining -= 1;
            }
            self.buffer.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.interrupted_flushes > 0 {
                self.interrupted_flushes -= 1;
                return Err(io::Error::new(io::ErrorKind::Interrupted, "simulated"));
            }
            Ok(())
        }
    }

    #[test]
    fn write_error_mid_frame_forces_a_fresh_frame_on_the_next_draw() {
        let mut renderer = Renderer::new(FlakyWriter::default()).width(10);
        renderer
            .draw_surface(surface(&["one", "two"], None))
            .unwrap();
        renderer.resize(4);

        renderer.writer.writes_until_failure = Some(1);
        assert!(
            renderer
                .draw_surface(surface(&["abcdefghij", "xy"], None))
                .is_err()
        );
        assert!(renderer.previous.is_none());

        let before = renderer.writer.buffer.len();
        renderer.draw_surface(surface(&["abc"], None)).unwrap();
        let update = &renderer.writer.buffer[before..];
        assert_eq!(
            update, b"\x1b[0mabc\x1b[K",
            "a redraw after a failed write must start below the cursor, not climb: {update:?}"
        );
    }

    fn styled_surface(text: &str) -> Surface {
        let mut surface = Surface::new();
        surface.write(text, Style::new().bold());
        surface
    }

    #[test]
    fn a_failed_write_after_an_sgr_is_reset_before_the_next_draw() {
        let mut renderer = Renderer::new(FlakyWriter::default());
        renderer.writer.writes_until_failure = Some(1);
        assert!(renderer.draw_surface(styled_surface("red")).is_err());
        assert!(renderer.writer.buffer.ends_with(b"1m"));

        let before = renderer.writer.buffer.len();
        renderer.draw_surface(surface(&["plain"], None)).unwrap();
        assert!(renderer.writer.buffer[before..].starts_with(b"\x1b[0m"));
    }

    #[test]
    fn clear_and_restore_cursor_reset_a_style_left_by_a_failed_write() {
        let mut renderer = Renderer::new(FlakyWriter::default());
        renderer.writer.writes_until_failure = Some(1);
        assert!(renderer.draw_surface(styled_surface("red")).is_err());

        let before = renderer.writer.buffer.len();
        renderer.restore_cursor().unwrap();
        assert_eq!(&renderer.writer.buffer[before..], b"\x1b[0m");

        renderer.writer.writes_until_failure = Some(1);
        assert!(renderer.draw_surface(styled_surface("red")).is_err());
        let before = renderer.writer.buffer.len();
        renderer.clear().unwrap();
        assert!(renderer.writer.buffer[before..].starts_with(b"\x1b[0m"));
    }

    #[test]
    fn clear_flushes_the_rendition_reset_when_no_frame_is_retained() {
        let mut renderer = Renderer::new(io::BufWriter::new(Vec::new()));
        renderer.rendition_uncertain = true;
        renderer.clear().unwrap();
        assert_eq!(renderer.writer.get_ref(), b"\x1b[0m");
    }

    #[test]
    fn clear_flushes_the_erased_frame_when_the_cursor_is_preserved() {
        let mut renderer = Renderer::new(io::BufWriter::with_capacity(1 << 16, Vec::new()));
        renderer.draw_surface(surface(&["one", "two"], None)).unwrap();
        let drawn = renderer.writer.get_ref().len();
        renderer.clear().unwrap();
        assert!(renderer.writer.get_ref().len() > drawn);
    }

    #[derive(Default)]
    struct FlushOnceFailing {
        buffer: Vec<u8>,
        failing_flushes: usize,
    }

    impl io::Write for FlushOnceFailing {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.failing_flushes > 0 {
                self.failing_flushes -= 1;
                return Err(io::Error::other("simulated flush failure"));
            }
            Ok(())
        }
    }

    #[test]
    fn restore_cursor_retries_after_a_failed_flush() {
        let mut renderer = Renderer::new(FlushOnceFailing::default())
            .cursor_visibility(CursorVisibility::FromSurface);
        renderer.draw_surface(surface(&["a"], None)).unwrap();
        assert!(renderer.writer.buffer.ends_with(b"\x1b[?25l"));

        renderer.writer.failing_flushes = 1;
        assert!(renderer.restore_cursor().is_err());

        let before = renderer.writer.buffer.len();
        renderer.restore_cursor().unwrap();
        assert_eq!(&renderer.writer.buffer[before..], b"\x1b[?25h");
        let before = renderer.writer.buffer.len();
        renderer.restore_cursor().unwrap();
        assert_eq!(renderer.writer.buffer.len(), before);
    }

    #[test]
    fn rendition_reset_is_rewritten_after_a_failed_flush() {
        let mut renderer = Renderer::new(FlushOnceFailing::default());
        renderer.rendition_uncertain = true;
        renderer.writer.failing_flushes = 1;
        assert!(renderer.restore_cursor().is_err());

        let before = renderer.writer.buffer.len();
        renderer.restore_cursor().unwrap();
        assert_eq!(&renderer.writer.buffer[before..], b"\x1b[0m");
    }

    #[test]
    fn interrupted_flush_is_retried_instead_of_desyncing_the_renderer() {
        let mut renderer = Renderer::new(FlakyWriter::default());
        renderer.writer.interrupted_flushes = 1;

        let stats = renderer.draw_surface(surface(&["ok"], None)).unwrap();
        assert_eq!(stats.changed_rows, 1);
        assert!(renderer.previous.is_some());
        assert_eq!(renderer.writer.buffer, b"ok\x1b[K");
    }

    #[test]
    fn narrowing_estimates_the_anchor_rows_wrap_around_a_wide_character() {
        let mut renderer = Renderer::new(Vec::new()).width(10);
        let mut frame = Surface::new();
        frame.write("a", Style::PLAIN);
        frame.write("界", Style::PLAIN);
        frame.write("bc", Style::PLAIN);
        frame.set_cursor(Position { row: 0, col: 3 });
        renderer.draw_surface(frame.clone()).unwrap();

        renderer.resize(3);
        let before = renderer.writer.len();
        renderer.draw_surface(frame).unwrap();
        let update = &renderer.writer[before..];
        assert!(
            update.starts_with(b"\r\x1b[J"),
            "the wide anchor cell fills the reflowed row exactly, so no climb is needed: {update:?}"
        );
    }

    #[test]
    fn wrap_surface_keeps_a_cursor_on_a_blank_row_past_column_zero() {
        let mut logical = Surface::new();
        logical.newline();
        logical.set_cursor(Position { row: 1, col: 5 });

        let wrapped = super::wrap_surface(&logical, 10);
        assert_eq!(wrapped.cursor(), Some(Position { row: 1, col: 5 }));
    }
}
