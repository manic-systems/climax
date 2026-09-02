use std::{
    marker::PhantomData,
    ops::{BitOr, BitOrAssign},
};

use crate::{
    CursorMerge, Fill, Insets, Rect, RenderCtx, Size, Surface, TickInterest, VerticalSize, Widget,
    renderer::layout_surface, surface::append_surface, widget::combine_tick_interest,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Edge(u8);

impl Edge {
    pub const TOP: Self = Self(1 << 0);
    pub const RIGHT: Self = Self(1 << 1);
    pub const BOTTOM: Self = Self(1 << 2);
    pub const LEFT: Self = Self(1 << 3);

    pub const fn contains(self, edge: Self) -> bool {
        self.0 & edge.0 == edge.0
    }
}

impl BitOr for Edge {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Edge {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Floating {
    edges: Edge,
    margin: Insets,
    max_size: Size,
    fill: Fill,
    cursor: CursorMerge,
}

impl Floating {
    pub const fn new(edges: Edge) -> Self {
        Self {
            edges,
            margin: Insets::new(0, 0, 0, 0),
            max_size: Size::new(usize::MAX, usize::MAX),
            fill: Fill::Transparent,
            cursor: CursorMerge::PreserveBase,
        }
    }

    #[must_use]
    pub const fn margin(mut self, margin: Insets) -> Self {
        self.margin = margin;
        self
    }

    #[must_use]
    pub const fn max_size(mut self, max_size: Size) -> Self {
        self.max_size = max_size;
        self
    }

    #[must_use]
    pub const fn fill(mut self, fill: Fill) -> Self {
        self.fill = fill;
        self
    }

    #[must_use]
    pub const fn cursor(mut self, cursor: CursorMerge) -> Self {
        self.cursor = cursor;
        self
    }
}

struct FloatingChild<H> {
    widget: H,
    policy: Floating,
}

pub struct Layers<'a, B, H = Box<dyn Widget + Send + Sync + 'a>> {
    base: B,
    floating: Vec<FloatingChild<H>>,
    lifetime: PhantomData<&'a ()>,
}

impl<'a, B> Layers<'a, B> {
    pub const fn new(base: B) -> Self {
        Self {
            base,
            floating: Vec::new(),
            lifetime: PhantomData,
        }
    }

    #[must_use]
    pub fn float<W>(mut self, child: W, policy: Floating) -> Self
    where
        W: Widget + Send + Sync + 'a,
    {
        self.floating.push(FloatingChild {
            widget: Box::new(child),
            policy,
        });
        self
    }
}

impl<'a, B> Layers<'a, B, Box<dyn Widget + 'a>> {
    /// Create a layer stack whose floating widgets remain on the current
    /// thread and may borrow local data.
    pub const fn local(base: B) -> Self {
        Self {
            base,
            floating: Vec::new(),
            lifetime: PhantomData,
        }
    }

    #[must_use]
    pub fn float<W>(mut self, child: W, policy: Floating) -> Self
    where
        W: Widget + 'a,
    {
        self.floating.push(FloatingChild {
            widget: Box::new(child),
            policy,
        });
        self
    }
}

impl<B, H> Widget for Layers<'_, B, H>
where
    B: Widget,
    H: Widget,
{
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let mut composed = Surface::new();
        self.base.render(ctx, &mut composed);
        composed = layout_surface(composed, ctx.available_columns(), ctx.layout_mode());
        if let Some(rows) = ctx.available_rows() {
            composed.fit_height(rows);
        }

        let canvas = ctx.viewport().map_or_else(
            || Rect::new(0, 0, composed.display_width(), composed.height()),
            crate::Viewport::rect,
        );
        for floating in &self.floating {
            render_floating(&mut composed, ctx, canvas, floating);
        }
        append_surface(out, &composed, composed.height());
    }

    fn tick_interest(&self) -> TickInterest {
        combine_tick_interest(
            std::iter::once(self.base.tick_interest()).chain(
                self.floating
                    .iter()
                    .map(|child| child.widget.tick_interest()),
            ),
        )
    }

    fn vertical_size(&self) -> VerticalSize {
        self.base.vertical_size()
    }
}

fn render_floating(
    composed: &mut Surface,
    parent: &RenderCtx,
    canvas: Rect,
    floating: &FloatingChild<impl Widget>,
) {
    let available = canvas.inset(floating.policy.margin);
    if available.is_empty() {
        return;
    }
    let stretch_x = floating.policy.edges.contains(Edge::LEFT | Edge::RIGHT);
    let stretch_y = floating.policy.edges.contains(Edge::TOP | Edge::BOTTOM);
    let constraint = Size {
        width: if stretch_x {
            available.size.width
        } else {
            available.size.width.min(floating.policy.max_size.width)
        },
        height: if stretch_y {
            available.size.height
        } else {
            available.size.height.min(floating.policy.max_size.height)
        },
    };
    if constraint.is_empty() {
        return;
    }

    let child_ctx = parent.with_constraints(Some(constraint.width), Some(constraint.height));
    let mut child = Surface::new();
    floating.widget.render(&child_ctx, &mut child);
    child = layout_surface(child, Some(constraint.width), parent.layout_mode());
    child.fit_height(constraint.height);

    let measured = Size {
        width: child.display_width().min(constraint.width),
        height: child.height().min(constraint.height),
    };
    if measured.is_empty() {
        return;
    }
    let horizontal = resolve_axis(
        available.origin.col,
        available.size.width,
        measured.width,
        floating.policy.edges.contains(Edge::LEFT),
        floating.policy.edges.contains(Edge::RIGHT),
    );
    let vertical = resolve_axis(
        available.origin.row,
        available.size.height,
        measured.height,
        floating.policy.edges.contains(Edge::TOP),
        floating.policy.edges.contains(Edge::BOTTOM),
    );
    composed.overlay(
        &child,
        Rect::new(vertical.0, horizontal.0, horizontal.1, vertical.1),
        canvas,
        floating.policy.fill,
        floating.policy.cursor,
    );
}

fn resolve_axis(
    origin: usize,
    available: usize,
    measured: usize,
    leading: bool,
    trailing: bool,
) -> (usize, usize) {
    if leading && trailing {
        return (origin, available);
    }
    let size = measured.min(available);
    let remaining = available.saturating_sub(size);
    let offset = if leading {
        0
    } else if trailing {
        remaining
    } else {
        remaining / 2
    };
    (origin.saturating_add(offset), size)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use crate::{Color, LayoutMode, Position, Style, Theme, Viewport};

    use super::*;

    #[derive(Clone)]
    struct Base;

    impl Widget for Base {
        fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
            out.write("base", Style::PLAIN);
            out.set_cursor(Position { row: 0, col: 2 });
        }

        fn vertical_size(&self) -> VerticalSize {
            VerticalSize::Flexible
        }
    }

    #[derive(Clone)]
    struct Probe(Arc<Mutex<Option<Viewport>>>);

    impl Widget for Probe {
        fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
            *self.0.lock().unwrap() = ctx.viewport();
            out.write("xy", Style::PLAIN);
        }
    }

    fn context(columns: usize, rows: usize) -> RenderCtx {
        RenderCtx::new()
            .with_constraints(Some(columns), Some(rows))
            .with_layout_mode(LayoutMode::Clip)
            .with_theme(Theme::DEFAULT)
    }

    #[test]
    fn opposed_edges_stretch_and_absent_edges_center() {
        let fill = Style::PLAIN.bg(Color::Blue);
        let layers = Layers::new(Base).float(
            "x",
            Floating::new(Edge::LEFT | Edge::RIGHT).fill(Fill::Opaque(fill)),
        );
        let mut surface = Surface::new();
        layers.render(&context(10, 5), &mut surface);

        assert_eq!(surface.height(), 3);
        assert_eq!(surface.row_width(2), 10);
        assert_eq!(surface.rows()[2].cells().last().unwrap().style, fill);
        assert_eq!(surface.cursor(), Some(Position { row: 0, col: 2 }));
    }

    struct LocalOverlay(Rc<RefCell<String>>);

    impl Widget for LocalOverlay {
        fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
            out.write(&*self.0.borrow(), Style::PLAIN);
        }
    }

    #[test]
    fn local_layers_accept_rc_refcell_widgets() {
        let value = Rc::new(RefCell::new("x".to_owned()));
        let layers = Layers::local("base").float(
            LocalOverlay(value.clone()),
            Floating::new(Edge::TOP | Edge::RIGHT),
        );
        let mut surface = Surface::new();
        layers.render(&context(8, 2), &mut surface);
        assert_eq!(surface.plain_text(), "base   x");

        *value.borrow_mut() = "yz".to_owned();
        let mut surface = Surface::new();
        layers.render(&context(8, 2), &mut surface);
        assert_eq!(surface.plain_text(), "base  yz");
    }

    struct BorrowedOverlay<'a>(&'a str);

    impl Widget for BorrowedOverlay<'_> {
        fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
            out.write(self.0, Style::PLAIN);
        }
    }

    #[test]
    fn ordinary_layers_retain_borrowed_thread_safe_children() {
        let value = String::from("xy");
        let layers = Layers::new("base").float(
            BorrowedOverlay(&value),
            Floating::new(Edge::TOP | Edge::RIGHT),
        );
        let mut surface = Surface::new();
        layers.render(&context(8, 2), &mut surface);
        assert_eq!(surface.plain_text(), "base  xy");
    }

    #[test]
    fn bottom_right_child_receives_exact_local_constraints() {
        let seen = Arc::new(Mutex::new(None));
        let layers = Layers::new(Base).float(
            Probe(seen.clone()),
            Floating::new(Edge::BOTTOM | Edge::RIGHT)
                .margin(Insets::bottom(1))
                .max_size(Size::new(6, 3)),
        );
        let mut surface = Surface::new();
        layers.render(&context(20, 8), &mut surface);

        assert_eq!(*seen.lock().unwrap(), Some(Viewport::new(6, 3)));
        assert_eq!(surface.height(), 7);
        assert_eq!(surface.rows()[6].cells().last().unwrap().text, "y");
        assert_eq!(surface.row_width(6), 20);
        assert_eq!(layers.vertical_size(), VerticalSize::Flexible);
    }

    type CapturedContext = (u64, Viewport, LayoutMode, Style);

    #[derive(Clone)]
    struct ContextProbe(Arc<Mutex<Option<CapturedContext>>>);

    impl Widget for ContextProbe {
        fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
            *self.0.lock().unwrap() = Some((
                ctx.frame(),
                ctx.viewport().unwrap(),
                ctx.layout_mode(),
                ctx.theme().style(crate::Role::Prompt),
            ));
            out.write("x", Style::PLAIN);
        }
    }

    #[test]
    fn child_context_inherits_frame_layout_and_theme() {
        let seen = Arc::new(Mutex::new(None));
        let theme = Theme::DEFAULT.with(crate::Role::Prompt, Style::PLAIN.bg(Color::Red));
        let ctx = RenderCtx::new()
            .with_frame(41)
            .with_constraints(Some(12), Some(7))
            .with_layout_mode(LayoutMode::Wrap)
            .with_theme(theme);
        let mut surface = Surface::new();
        Layers::new(Base)
            .float(
                ContextProbe(seen.clone()),
                Floating::new(Edge::RIGHT).max_size(Size::new(4, 3)),
            )
            .render(&ctx, &mut surface);
        assert_eq!(
            *seen.lock().unwrap(),
            Some((
                41,
                Viewport::new(4, 3),
                LayoutMode::Wrap,
                Style::PLAIN.bg(Color::Red),
            )),
        );
    }

    #[test]
    fn no_viewport_uses_measured_base_canvas() {
        let layers = Layers::new("abcd").float("x", Floating::new(Edge::BOTTOM | Edge::RIGHT));
        let mut surface = Surface::new();
        layers.render(&RenderCtx::new(), &mut surface);
        assert_eq!(surface.plain_text(), "abcx");
    }

    fn text_position(surface: &Surface, wanted: &str) -> Option<Position> {
        for (row, rendered) in surface.rows().iter().enumerate() {
            let mut col = 0;
            for cell in rendered.cells() {
                if cell.text == wanted {
                    return Some(Position { row, col });
                }
                col += cell.width;
            }
        }
        None
    }

    #[test]
    fn every_edge_combination_resolves_each_axis_independently() {
        let combinations = [
            (Edge::default(), Position { row: 2, col: 2 }),
            (Edge::TOP, Position { row: 0, col: 2 }),
            (Edge::RIGHT, Position { row: 2, col: 4 }),
            (Edge::BOTTOM, Position { row: 4, col: 2 }),
            (Edge::LEFT, Position { row: 2, col: 0 }),
            (Edge::TOP | Edge::RIGHT, Position { row: 0, col: 4 }),
            (Edge::TOP | Edge::LEFT, Position { row: 0, col: 0 }),
            (Edge::BOTTOM | Edge::RIGHT, Position { row: 4, col: 4 }),
            (Edge::BOTTOM | Edge::LEFT, Position { row: 4, col: 0 }),
            (Edge::LEFT | Edge::RIGHT, Position { row: 2, col: 0 }),
            (Edge::TOP | Edge::BOTTOM, Position { row: 0, col: 2 }),
            (
                Edge::TOP | Edge::LEFT | Edge::RIGHT,
                Position { row: 0, col: 0 },
            ),
            (
                Edge::BOTTOM | Edge::LEFT | Edge::RIGHT,
                Position { row: 4, col: 0 },
            ),
            (
                Edge::LEFT | Edge::TOP | Edge::BOTTOM,
                Position { row: 0, col: 0 },
            ),
            (
                Edge::RIGHT | Edge::TOP | Edge::BOTTOM,
                Position { row: 0, col: 4 },
            ),
            (
                Edge::TOP | Edge::RIGHT | Edge::BOTTOM | Edge::LEFT,
                Position { row: 0, col: 0 },
            ),
        ];
        for (edges, expected) in combinations {
            let mut surface = Surface::new();
            Layers::new("base")
                .float("X", Floating::new(edges))
                .render(&context(5, 5), &mut surface);
            assert_eq!(
                text_position(&surface, "X"),
                Some(expected),
                "edges={edges:?}"
            );
            assert!(surface.height() <= 5);
            assert!(surface.display_width() <= 5);
        }
    }

    #[test]
    fn later_layers_occlude_earlier_layers() {
        let mut surface = Surface::new();
        Layers::new("base")
            .float("first", Floating::new(Edge::TOP | Edge::LEFT))
            .float("X", Floating::new(Edge::TOP | Edge::LEFT))
            .render(&context(8, 3), &mut surface);
        assert_eq!(surface.plain_text(), "Xirst");
    }

    struct Animated(TickInterest, VerticalSize);

    impl Widget for Animated {
        fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
            out.write("x", Style::PLAIN);
        }

        fn tick_interest(&self) -> TickInterest {
            self.0
        }

        fn vertical_size(&self) -> VerticalSize {
            self.1
        }
    }

    #[test]
    fn floating_ticks_combine_but_vertical_size_stays_with_the_base() {
        let layers = Layers::new(Animated(
            TickInterest::Every(Duration::from_secs(2)),
            VerticalSize::Content,
        ))
        .float(
            Animated(TickInterest::EveryFrame, VerticalSize::Flexible),
            Floating::new(Edge::TOP),
        );
        assert_eq!(layers.tick_interest(), TickInterest::EveryFrame);
        assert_eq!(layers.vertical_size(), VerticalSize::Content);
    }

    #[test]
    fn oversized_children_stay_inside_the_canvas_in_clip_and_wrap_modes() {
        for mode in [LayoutMode::Clip, LayoutMode::Wrap] {
            let ctx = RenderCtx::new()
                .with_frame(7)
                .with_constraints(Some(5), Some(4))
                .with_layout_mode(mode)
                .with_theme(Theme::DEFAULT);
            let mut surface = Surface::new();
            Layers::new("abcdefghijk")
                .float(
                    "123456789\nabcdefghi",
                    Floating::new(Edge::BOTTOM | Edge::RIGHT)
                        .max_size(Size::new(3, 2))
                        .fill(Fill::Opaque(Style::PLAIN)),
                )
                .render(&ctx, &mut surface);
            assert!(surface.height() <= 4, "mode={mode:?}");
            assert!(surface.display_width() <= 5, "mode={mode:?}");
            assert_eq!(
                text_position(&surface, "1").map(|position| position.col),
                Some(2)
            );
        }
    }
}
