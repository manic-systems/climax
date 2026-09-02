use crate::Position;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Size {
    pub const fn new(width: usize, height: usize) -> Self {
        Self { width, height }
    }

    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub origin: Position,
    pub size: Size,
}

impl Rect {
    pub const fn new(row: usize, col: usize, width: usize, height: usize) -> Self {
        Self {
            origin: Position { row, col },
            size: Size { width, height },
        }
    }

    pub const fn right(self) -> usize {
        self.origin.col.saturating_add(self.size.width)
    }

    pub const fn bottom(self) -> usize {
        self.origin.row.saturating_add(self.size.height)
    }

    pub const fn is_empty(self) -> bool {
        self.size.is_empty()
    }

    pub const fn contains(self, position: Position) -> bool {
        !self.is_empty()
            && position.row >= self.origin.row
            && position.row < self.bottom()
            && position.col >= self.origin.col
            && position.col < self.right()
    }

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Insets {
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
    pub left: usize,
}

impl Insets {
    pub const fn new(top: usize, right: usize, bottom: usize, left: usize) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub const fn all(value: usize) -> Self {
        Self::new(value, value, value, value)
    }

    pub const fn horizontal(value: usize) -> Self {
        Self::new(0, value, 0, value)
    }

    pub const fn bottom(value: usize) -> Self {
        Self::new(0, 0, value, 0)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Viewport {
    pub columns: usize,
    pub rows: usize,
}

impl Viewport {
    pub const fn new(columns: usize, rows: usize) -> Self {
        Self { columns, rows }
    }

    pub const fn size(self) -> Size {
        Size::new(self.columns, self.rows)
    }

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
