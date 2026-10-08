// SPDX-License-Identifier: EUPL-1.2

use std::{
    io,
    os::fd::AsFd,
};

use rustix::termios::tcgetwinsize;

use crate::Position;

/// a width and height in terminal cells
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Size {
    /// columns
    pub width:  usize,
    /// rows
    pub height: usize,
}

impl Size {
    /// creates a size from a width and a height
    pub const fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }

    /// whether either dimension is zero
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// a rectangle of terminal cells, anchored at its top-left origin
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    /// top-left cell
    pub origin: Position,
    /// extent in columns and rows
    pub size:   Size,
}

impl Rect {
    /// creates a rectangle from row, column, width and height, rows first
    /// unlike `Size::new`
    pub const fn new(row: usize, col: usize, width: usize, height: usize) -> Self {
        Self {
            origin: Position { row, col },
            size:   Size { width, height },
        }
    }

    /// the column just past the right edge
    pub const fn right(self) -> usize {
        self.origin.col.saturating_add(self.size.width)
    }

    /// the row just past the bottom edge
    pub const fn bottom(self) -> usize {
        self.origin.row.saturating_add(self.size.height)
    }

    /// whether the rectangle covers no cells
    pub const fn is_empty(self) -> bool {
        self.size.is_empty()
    }

    /// whether `position` lies inside the rectangle
    pub const fn contains(self, position: Position) -> bool {
        !self.is_empty()
            && position.row >= self.origin.row
            && position.row < self.bottom()
            && position.col >= self.origin.col
            && position.col < self.right()
    }

    /// the overlap of two rectangles, which is empty when they do not meet
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

    /// moves the rectangle down by `rows` and right by `columns`
    #[must_use]
    pub const fn translate(self, rows: usize, columns: usize) -> Self {
        Self {
            origin: Position {
                row: self.origin.row.saturating_add(rows),
                col: self.origin.col.saturating_add(columns),
            },
            size:   self.size,
        }
    }

    /// shrinks the rectangle by `insets` on each side, never below zero size
    #[must_use]
    pub const fn inset(self, insets: Insets) -> Self {
        let rows = insets.top.saturating_add(insets.bottom);
        let columns = insets.left.saturating_add(insets.right);
        Self {
            origin: Position {
                row: self.origin.row.saturating_add(insets.top),
                col: self.origin.col.saturating_add(insets.left),
            },
            size:   Size {
                width:  self.size.width.saturating_sub(columns),
                height: self.size.height.saturating_sub(rows),
            },
        }
    }
}

/// padding in cells on each side of a rectangle
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Insets {
    /// rows above
    pub top:    usize,
    /// columns to the right
    pub right:  usize,
    /// rows below
    pub bottom: usize,
    /// columns to the left
    pub left:   usize,
}

impl Insets {
    /// creates insets in top, right, bottom, left order
    pub const fn new(top: usize, right: usize, bottom: usize, left: usize) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// insets of `value` on every side
    pub const fn all(value: usize) -> Self {
        Self::new(value, value, value, value)
    }

    /// insets of `value` on the left and right only
    pub const fn horizontal(value: usize) -> Self {
        Self::new(0, value, 0, value)
    }

    /// insets of `value` below only
    pub const fn bottom(value: usize) -> Self {
        Self::new(0, 0, value, 0)
    }
}

/// the size of the visible terminal area
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Viewport {
    /// visible columns
    pub columns: usize,
    /// visible rows
    pub rows:    usize,
}

impl Viewport {
    /// the size assumed when a terminal cannot be measured, 80 columns by 24
    /// rows
    pub const FALLBACK: Self = Self::new(80, 24);

    /// creates a viewport from columns then rows
    pub const fn new(columns: usize, rows: usize) -> Self {
        Self { columns, rows }
    }

    /// measures the terminal behind `terminal`
    ///
    /// # Errors
    ///
    /// fails when the descriptor is not a terminal or reports a zero-sized
    /// viewport
    pub fn of(terminal: &impl AsFd) -> io::Result<Self> {
        let size = tcgetwinsize(terminal)?;
        if size.ws_col == 0 || size.ws_row == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "terminal reported a zero-sized viewport",
            ));
        }
        Ok(Self::new(
            usize::from(size.ws_col),
            usize::from(size.ws_row),
        ))
    }

    /// the viewport as a [`Size`]
    pub const fn size(self) -> Size {
        Size::new(self.columns, self.rows)
    }

    /// the viewport as a [`Rect`] anchored at the origin
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
        assert_eq!(rect.translate(9, 9).origin, Position {
            row: usize::MAX,
            col: usize::MAX,
        });
    }
}
