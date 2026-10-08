// SPDX-License-Identifier: EUPL-1.2

use std::{io, os::fd::AsFd};

use rustix::termios::tcgetwinsize;

use crate::Position;

/// A width and height in terminal cells.
///
/// This is the extent of any rectangle, such as a [`Rect`] or a floating layer. It holds the same
/// two numbers as a [`Viewport`], which names them columns and rows and describes the visible
/// terminal area rather than an arbitrary rectangle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Size {
    /// Columns.
    pub width: usize,
    /// Rows.
    pub height: usize,
}

impl Size {
    /// Creates a size from a width and a height.
    pub const fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }

    /// Whether either dimension is zero.
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// A rectangle of terminal cells, anchored at its top-left origin.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    /// Top-left cell.
    pub origin: Position,
    /// Extent in columns and rows.
    pub size: Size,
}

impl Rect {
    /// Creates a rectangle from its origin row and column, then its width and height.
    /// The argument order is row first, unlike `Size::new` and `Viewport::new` which put columns
    /// first.
    pub const fn new(row: usize, col: usize, width: usize, height: usize) -> Self {
        Self {
            origin: Position { row, col },
            size: Size { width, height },
        }
    }

    /// The column just past the right edge.
    pub const fn right(self) -> usize {
        self.origin.col.saturating_add(self.size.width)
    }

    /// The row just past the bottom edge.
    pub const fn bottom(self) -> usize {
        self.origin.row.saturating_add(self.size.height)
    }

    /// Whether the rectangle covers no cells.
    pub const fn is_empty(self) -> bool {
        self.size.is_empty()
    }

    /// Whether `position` lies inside the rectangle.
    pub const fn contains(self, position: Position) -> bool {
        !self.is_empty()
            && position.row >= self.origin.row
            && position.row < self.bottom()
            && position.col >= self.origin.col
            && position.col < self.right()
    }

    /// The overlap of two rectangles, which is empty when they do not meet.
    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        let top = if self.origin.row > other.origin.row {
            self.origin.row
        } else {
            other.origin.row
        };
        let left = if self.origin.col > other.origin.col {
            self.origin.col
        } else {
            other.origin.col
        };
        let bottom = if self.bottom() < other.bottom() {
            self.bottom()
        } else {
            other.bottom()
        };
        let right = if self.right() < other.right() {
            self.right()
        } else {
            other.right()
        };
        Self::new(
            top,
            left,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        )
    }

    /// Moves the rectangle down by `rows` and right by `columns`.
    #[must_use]
    pub const fn translate(self, rows: usize, columns: usize) -> Self {
        Self {
            origin: Position {
                row: self.origin.row.saturating_add(rows),
                col: self.origin.col.saturating_add(columns),
            },
            size: self.size,
        }
    }

    /// Shrinks the rectangle by `insets` on each side, never below zero size.
    #[must_use]
    pub const fn inset(self, insets: Insets) -> Self {
        let rows = insets.top.saturating_add(insets.bottom);
        let columns = insets.left.saturating_add(insets.right);
        Self {
            origin: Position {
                row: self.origin.row.saturating_add(insets.top),
                col: self.origin.col.saturating_add(insets.left),
            },
            size: Size {
                width: self.size.width.saturating_sub(columns),
                height: self.size.height.saturating_sub(rows),
            },
        }
    }
}

/// Padding in cells on each side of a rectangle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Insets {
    /// Rows above.
    pub top: usize,
    /// Columns to the right.
    pub right: usize,
    /// Rows below.
    pub bottom: usize,
    /// Columns to the left.
    pub left: usize,
}

impl Insets {
    /// Creates insets in top, right, bottom, left order.
    pub const fn new(top: usize, right: usize, bottom: usize, left: usize) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Insets of `value` on every side.
    pub const fn all(value: usize) -> Self {
        Self::new(value, value, value, value)
    }

    /// Insets of `value` on the left and right only.
    pub const fn horizontal(value: usize) -> Self {
        Self::new(0, value, 0, value)
    }

    /// Insets of `value` below only.
    pub const fn bottom(value: usize) -> Self {
        Self::new(0, 0, value, 0)
    }
}

/// The size of the visible terminal area.
///
/// [`RenderCtx::viewport`](crate::RenderCtx::viewport) reports it. It holds the same two numbers as
/// a [`Size`] but names them columns and rows, as terminal APIs do. Use [`Viewport::size`] or
/// [`Viewport::rect`] to lay out inside it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Viewport {
    /// Visible columns.
    pub columns: usize,
    /// Visible rows.
    pub rows: usize,
}

impl Viewport {
    /// The size assumed when a terminal cannot be measured, 80 columns by 24 rows.
    pub const FALLBACK: Self = Self::new(80, 24);

    /// Creates a viewport from columns then rows.
    pub const fn new(columns: usize, rows: usize) -> Self {
        Self { columns, rows }
    }

    /// Measures the terminal behind `terminal`.
    ///
    /// Fails when the descriptor is not a terminal or reports a zero-sized viewport.
    pub fn of(terminal: &impl AsFd) -> io::Result<Self> {
        let size = tcgetwinsize(terminal)?;
        if size.ws_col == 0 || size.ws_row == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "terminal reported a zero-sized viewport",
            ));
        }
        Ok(Self::new(usize::from(size.ws_col), usize::from(size.ws_row)))
    }

    /// The viewport as a [`Size`].
    pub const fn size(self) -> Size {
        Size::new(self.columns, self.rows)
    }

    /// The viewport as a [`Rect`] anchored at the origin.
    pub const fn rect(self) -> Rect {
        Rect::new(0, 0, self.columns, self.rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_intersection_and_insets_saturate() {
        let rect = Rect::new(2, 3, 8, 5);
        assert_eq!(
            rect.intersection(Rect::new(4, 1, 5, 8)),
            Rect::new(4, 3, 3, 3)
        );
        assert_eq!(
            rect.inset(Insets::new(20, 20, 20, 20)),
            Rect::new(22, 23, 0, 0)
        );
    }

    #[test]
    fn rectangle_edges_and_translation_saturate() {
        let rect = Rect::new(usize::MAX - 1, usize::MAX - 1, 8, 8);
        assert_eq!(rect.right(), usize::MAX);
        assert_eq!(rect.bottom(), usize::MAX);
        assert_eq!(
            rect.translate(9, 9).origin,
            Position {
                row: usize::MAX,
                col: usize::MAX,
            }
        );
    }
}
