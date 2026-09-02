use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

use crate::{
    RenderCtx, Surface, TickInterest, VerticalSize, Widget, WidgetRef, renderer::layout_surface,
    surface::append_surface, widget::combine_tick_interest,
};

/// The result of allocating logical children into a physical viewport.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewportReport {
    /// Children with at least one rendered row.
    pub visible: Range<usize>,
    /// Children rendered without clipping.
    pub fully_visible: Range<usize>,
    /// Requested start for the preceding physical page, if any.
    pub page_up: Option<usize>,
    /// Requested start for the following physical page, if any.
    pub page_down: Option<usize>,
}

/// A cloneable observer for the latest [`VerticalViewport`] allocation.
#[derive(Clone, Default)]
pub struct ViewportReportHandle {
    inner: Arc<Mutex<ViewportReport>>,
}

impl ViewportReportHandle {
    /// Return the most recent allocation report.
    #[must_use]
    pub fn report(&self) -> ViewportReport {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn set(&self, report: ViewportReport) {
        *self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = report;
    }
}

/// A vertically flexible collection of atomic child widgets.
///
/// Child height is measured after Screw applies wrapping. Whole children are
/// packed where possible; a child taller than the viewport is clipped. When
/// several flexible viewports share a [`crate::Stack`], the stack divides its
/// remaining height between them before they render.
#[derive(Clone)]
pub struct VerticalViewport<H = WidgetRef> {
    children: Box<[H]>,
    requested_start: usize,
    anchor: Option<usize>,
    max_children: Option<usize>,
    trailing: Option<H>,
    report: ViewportReportHandle,
}

impl<H> VerticalViewport<H> {
    /// Create a viewport over logical child widgets.
    pub fn new(children: impl Into<Vec<H>>) -> Self {
        Self {
            children: children.into().into_boxed_slice(),
            requested_start: 0,
            anchor: None,
            max_children: None,
            trailing: None,
            report: ViewportReportHandle::default(),
        }
    }

    /// Set the preferred first child.
    #[must_use]
    pub const fn requested_start(mut self, requested_start: usize) -> Self {
        self.requested_start = requested_start;
        self
    }

    /// Keep this child in view when possible.
    #[must_use]
    pub const fn anchor(mut self, anchor: Option<usize>) -> Self {
        self.anchor = anchor;
        self
    }

    /// Cap visible children independently of physical height.
    #[must_use]
    pub const fn max_children(mut self, max_children: Option<usize>) -> Self {
        self.max_children = match max_children {
            Some(0) => Some(1),
            Some(max_children) => Some(max_children),
            None => None,
        };
        self
    }

    /// Reserve and render fixed trailing content within the allocation.
    #[must_use]
    pub fn trailing(mut self, trailing: H) -> Self {
        self.trailing = Some(trailing);
        self
    }

    /// Obtain a handle that observes the latest allocation.
    #[must_use]
    pub fn report_handle(&self) -> ViewportReportHandle {
        self.report.clone()
    }
}

impl<H> Widget for VerticalViewport<H>
where
    H: Widget,
{
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let surfaces = self
            .children
            .iter()
            .map(|child| {
                let mut surface = Surface::new();
                child.render(ctx, &mut surface);
                layout_surface(surface, ctx.available_columns(), ctx.layout_mode())
            })
            .collect::<Vec<_>>();
        let heights = surfaces.iter().map(Surface::height).collect::<Vec<_>>();
        let trailing = self.trailing.as_ref().map(|trailing| {
            let mut surface = Surface::new();
            trailing.render(ctx, &mut surface);
            layout_surface(surface, ctx.available_columns(), ctx.layout_mode())
        });
        let remaining_height = ctx
            .available_rows()
            .map(|height| height.saturating_sub(out.height().saturating_sub(1)));
        let trailing_limit = remaining_height
            .map(|remaining| trailing.as_ref().map_or(0, Surface::height).min(remaining));
        let available = remaining_height
            .map(|remaining| remaining.saturating_sub(trailing_limit.unwrap_or_default()));
        let allocation = allocate(
            &heights,
            available,
            self.requested_start,
            self.anchor,
            self.max_children,
        );
        self.report.set(allocation.report.clone());

        let mut remaining = available.unwrap_or(usize::MAX);
        let first = allocation.report.visible.start;
        let end = allocation.report.visible.end;
        let mut wrote_child = false;
        for (index, surface) in surfaces.iter().enumerate().take(end).skip(first) {
            if remaining == 0 {
                break;
            }
            if index > first {
                out.newline();
            }
            let written = append_surface(out, surface, remaining);
            remaining = remaining.saturating_sub(written);
            wrote_child |= written > 0;
        }
        if let Some(trailing) = &trailing {
            let limit = trailing_limit.unwrap_or(usize::MAX);
            if wrote_child && limit > 0 {
                out.newline();
            }
            append_surface(out, trailing, limit);
        }
    }

    fn tick_interest(&self) -> TickInterest {
        combine_tick_interest(
            self.children
                .iter()
                .map(Widget::tick_interest)
                .chain(self.trailing.iter().map(Widget::tick_interest)),
        )
    }

    fn vertical_size(&self) -> VerticalSize {
        VerticalSize::Flexible
    }
}

#[derive(Debug)]
struct Allocation {
    report: ViewportReport,
}

fn allocate(
    heights: &[usize],
    available: Option<usize>,
    requested_start: usize,
    anchor: Option<usize>,
    max_children: Option<usize>,
) -> Allocation {
    if heights.is_empty() || available == Some(0) {
        return Allocation {
            report: ViewportReport::default(),
        };
    }

    let budget = available.unwrap_or(usize::MAX);
    let anchor = anchor.map(|anchor| anchor.min(heights.len() - 1));
    let mut start = requested_start.min(heights.len() - 1);
    if let Some(anchor) = anchor
        && anchor < start
    {
        start = anchor;
    }

    let mut end = packed_end(heights, start, budget, max_children);
    if let Some(anchor) = anchor {
        while anchor >= end && start < anchor {
            start += 1;
            end = packed_end(heights, start, budget, max_children);
        }
    }

    if end == heights.len() {
        let mut used = heights[start..end].iter().sum::<usize>();
        while start > 0
            && end.saturating_sub(start) < max_children.unwrap_or(usize::MAX)
            && used.saturating_add(heights[start - 1]) <= budget
        {
            start -= 1;
            used += heights[start];
        }
    }
    end = packed_end(heights, start, budget, max_children);

    let first_height = heights[start];
    let first_is_clipped = first_height > budget;
    let fully_visible = if first_is_clipped {
        start..start
    } else {
        start..end
    };
    let page_up = previous_start(heights, start, budget, max_children);
    let page_down = (end < heights.len()).then_some(end);

    Allocation {
        report: ViewportReport {
            visible: start..end,
            fully_visible,
            page_up,
            page_down,
        },
    }
}

fn packed_end(
    heights: &[usize],
    start: usize,
    budget: usize,
    max_children: Option<usize>,
) -> usize {
    let mut used = 0_usize;
    let mut end = start;
    while end < heights.len() && end - start < max_children.unwrap_or(usize::MAX) {
        let next = used.saturating_add(heights[end]);
        if next > budget {
            break;
        }
        used = next;
        end += 1;
    }
    if end == start && budget > 0 {
        start + 1
    } else {
        end
    }
}

fn previous_start(
    heights: &[usize],
    start: usize,
    budget: usize,
    max_children: Option<usize>,
) -> Option<usize> {
    if start == 0 {
        return None;
    }
    let mut previous = start;
    let mut used = 0_usize;
    while previous > 0 && start - previous < max_children.unwrap_or(usize::MAX) {
        let height = heights[previous - 1];
        if used > 0 && used.saturating_add(height) > budget {
            break;
        }
        previous -= 1;
        used = used.saturating_add(height);
        if used >= budget {
            break;
        }
    }
    Some(previous)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LayoutMode, Renderer, Stack, Text, widget};

    #[test]
    fn viewport_allocates_wrapped_children_around_an_anchor() {
        let children = vec![
            widget(Text::new("one")),
            widget(Text::new("abcdef")),
            widget(Text::new("three")),
            widget(Text::new("four")),
        ];
        let viewport = VerticalViewport::new(children)
            .requested_start(0)
            .anchor(Some(2));
        let report = viewport.report_handle();
        let mut output = Vec::new();
        Renderer::new(&mut output)
            .width(6)
            .height(3)
            .layout_mode(LayoutMode::Wrap)
            .draw(&viewport)
            .unwrap();

        assert_eq!(report.report().visible, 1..3);
        assert_eq!(report.report().fully_visible, 1..3);
    }

    #[test]
    fn viewport_reports_physical_page_targets() {
        let children = vec![
            widget(Text::new("one")),
            widget(Text::new("two\ncontinued")),
            widget(Text::new("three")),
            widget(Text::new("four")),
        ];
        let viewport = VerticalViewport::new(children).anchor(Some(0));
        let report = viewport.report_handle();
        let mut output = Vec::new();
        Renderer::new(&mut output)
            .height(3)
            .layout_mode(LayoutMode::Clip)
            .draw(&viewport)
            .unwrap();

        assert_eq!(report.report().visible, 0..2);
        assert_eq!(report.report().page_down, Some(2));
    }

    #[test]
    fn trailing_content_can_consume_the_entire_viewport() {
        let viewport = VerticalViewport::new(vec![widget(Text::new("one"))])
            .trailing(widget(Text::new("help")));
        let report = viewport.report_handle();
        let mut output = Vec::new();
        Renderer::new(&mut output)
            .height(1)
            .draw(&viewport)
            .unwrap();

        assert_eq!(report.report().visible, 0..0);
    }

    #[test]
    fn viewport_respects_rows_already_used_by_a_stack() {
        let viewport = VerticalViewport::new(vec![
            widget(Text::new("one")),
            widget(Text::new("two")),
            widget(Text::new("three")),
        ]);
        let report = viewport.report_handle();
        let root = Stack::new(vec![widget(Text::new("header")), widget(viewport)]);
        let mut output = Vec::new();
        Renderer::new(&mut output).height(3).draw(&root).unwrap();

        assert_eq!(report.report().visible, 0..2);
    }

    #[test]
    fn stack_shares_remaining_rows_between_viewports() {
        let first = VerticalViewport::new(vec![
            widget(Text::new("one")),
            widget(Text::new("two")),
            widget(Text::new("three")),
        ]);
        let first_report = first.report_handle();
        let second = VerticalViewport::new(vec![
            widget(Text::new("four")),
            widget(Text::new("five")),
            widget(Text::new("six")),
        ]);
        let second_report = second.report_handle();
        let root = Stack::new(vec![
            widget(Text::new("header")),
            widget(first),
            widget(second),
        ]);
        let mut output = Vec::new();
        Renderer::new(&mut output).height(5).draw(&root).unwrap();

        assert_eq!(first_report.report().visible, 0..2);
        assert_eq!(second_report.report().visible, 0..2);
    }
}
