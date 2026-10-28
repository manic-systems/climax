// SPDX-License-Identifier: EUPL-1.2

use core::{
    cell::RefCell,
    ops::Range,
};

use screw::{
    LocalWidgetRef,
    RenderCtx,
    Role,
    Span,
    Spans,
    Surface,
    Text,
    VerticalViewport,
    ViewportReport,
    Widget as ScrewWidget,
    local_widget,
};

use crate::{
    KeyEvent,
    Modifiers,
    Reaction,
};

pub(super) const DEFAULT_PAGE_SIZE: usize = 9;

pub(super) fn move_index(current: usize, len: usize, delta: isize, wrap: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }

    let bounded = current.min(len - 1);
    if wrap {
        let span = isize::try_from(len).ok()?;
        let from = isize::try_from(bounded).ok()?;
        let next = (from + delta).rem_euclid(span);
        return usize::try_from(next).ok();
    }

    let next = bounded.saturating_add_signed(delta).min(len - 1);
    Some(next)
}

pub(super) fn visible_delta(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

pub(super) const fn no_modifiers(key: &KeyEvent) -> bool {
    key.modifiers.bits() == 0
}

pub(super) const fn only_control(key: &KeyEvent) -> bool {
    key.modifiers.bits() == Modifiers::CONTROL.bits()
}

pub(super) const fn chorded(key: &KeyEvent) -> bool {
    let chord = Modifiers::CONTROL.bits() | Modifiers::ALT.bits() | Modifiers::SUPER.bits();
    key.modifiers.bits() & chord != 0
}

/// lands a page key on `target`, moving both the scroll offset and the
/// selection
pub(super) fn page_move(
    top: &mut usize,
    selected: &mut usize,
    target: usize,
    len: usize,
) -> Reaction {
    if len == 0 {
        return Reaction::Ignored;
    }
    let clamped = target.min(len - 1);
    if *selected == clamped && *top == clamped {
        return Reaction::Ignored;
    }
    *selected = clamped;
    *top = clamped;
    Reaction::Changed
}

/// the rows of the full logical list a widget builds, one page of margin on
/// each side
pub(super) fn window_range(top: usize, window_size: usize, len: usize) -> Range<usize> {
    if len == 0 {
        return 0..0;
    }
    let margin = window_size.max(1);
    let first = top.min(len - 1);
    let start = first.saturating_sub(margin);
    let end = first.saturating_add(margin.saturating_mul(2)).min(len);
    start..end
}

/// the layout measured the last time the list rendered, indexed in the full
/// logical list
#[derive(Clone, Debug, Default)]
pub(super) struct PageLayout {
    page: RefCell<Option<Page>>,
}

#[derive(Clone, Debug)]
struct Page {
    visible: Range<usize>,
    up:      Option<usize>,
    down:    Option<usize>,
}

impl PageLayout {
    fn record(&self, report: &ViewportReport, offset: usize) {
        let visible = (report.visible.start + offset)..(report.visible.end + offset);
        *self.page.borrow_mut() = Some(Page {
            visible,
            up: report.page_up.map(|index| index + offset),
            down: report.page_down.map(|index| index + offset),
        });
    }

    pub(super) fn sync_top(&self, top: &mut usize) {
        if let Some(page) = &*self.page.borrow() {
            *top = page.visible.start;
        }
    }

    /// the page jump targets of the last render, forgotten once read because a
    /// key press makes them stale
    pub(super) fn take_targets(&self) -> PageTargets {
        self.page
            .borrow_mut()
            .take()
            .map_or_else(PageTargets::default, |page| {
                PageTargets {
                    up:   page.up,
                    down: page.down,
                }
            })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PageTargets {
    up:   Option<usize>,
    down: Option<usize>,
}

impl PageTargets {
    pub(super) const fn target(self, down: bool) -> Option<usize> {
        if down { self.down } else { self.up }
    }
}

/// one logical row of a list, before it is clipped or wrapped
pub(super) struct ListRow {
    pub spans:    Vec<(String, Role)>,
    pub selected: bool,
    pub checked:  Option<bool>,
}

impl ScrewWidget for ListRow {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let theme = ctx.theme();
        let marker = theme.style(if self.selected {
            Role::Selected
        } else {
            Role::Dim
        });
        out.write(if self.selected { "> " } else { "  " }, marker);
        if let Some(checked) = self.checked {
            out.write(if checked { "[x] " } else { "[ ] " }, marker);
        }
        let continuation = if self.checked.is_some() {
            "      "
        } else {
            "  "
        };
        for (text, role) in &self.spans {
            let style = theme.style(*role);
            for part in text.split_inclusive('\n') {
                let (line, newline) = part
                    .strip_suffix('\n')
                    .map_or((part, false), |line| (line, true));
                out.write(line, style);
                if newline {
                    out.newline();
                    out.write(continuation, theme.style(Role::Dim));
                }
            }
        }
    }
}

pub(super) struct ListFrame<'a> {
    pub header:        &'a [Span],
    pub rows:          Vec<ListRow>,
    /// where `rows` begins in the full logical list
    pub window_offset: usize,
    /// selected row, indexed in the full logical list
    pub selected:      Option<usize>,
    /// scroll position, indexed in the full logical list
    pub top:           usize,
    pub max_visible:   usize,
    pub help:          &'a str,
}

pub(super) fn render_list(
    frame: ListFrame<'_>,
    layout: &PageLayout,
    ctx: &RenderCtx,
    out: &mut Surface,
) {
    if !frame.header.is_empty() {
        Spans::new(frame.header.iter().cloned()).render(ctx, out);
        out.newline();
    }
    let offset = frame.window_offset;
    let rows: Vec<LocalWidgetRef<'static>> = frame.rows.into_iter().map(local_widget).collect();
    let viewport = VerticalViewport::new(rows)
        .requested_start(frame.top.saturating_sub(offset))
        .anchor(
            frame
                .selected
                .map(|selected| selected.saturating_sub(offset)),
        )
        .max_children(Some(frame.max_visible))
        .trailing(local_widget(Text::new(frame.help).role(Role::Dim)));
    let report = viewport.report_handle();
    viewport.render(ctx, out);
    layout.record(&report.report(), offset);
}

pub(super) fn visible_len(len: usize, page_size: usize) -> usize {
    page_size.min(len).max(1)
}

pub(super) fn ensure_visible(selected: &mut usize, top: &mut usize, len: usize, page_size: usize) {
    if len == 0 {
        *selected = 0;
        *top = 0;
        return;
    }

    *selected = (*selected).min(len - 1);
    let visible = visible_len(len, page_size);
    if *selected < *top {
        *top = *selected;
    } else if *selected >= *top + visible {
        *top = *selected + 1 - visible;
    }

    let max_top = len.saturating_sub(visible);
    *top = (*top).min(max_top);
}

pub(super) fn move_to(
    selected: &mut usize,
    top: &mut usize,
    target: usize,
    len: usize,
    page_size: usize,
) -> Reaction {
    if len == 0 {
        return Reaction::Ignored;
    }
    let clamped = target.min(len - 1);
    if clamped == *selected {
        return Reaction::Ignored;
    }
    *selected = clamped;
    ensure_visible(selected, top, len, page_size);
    Reaction::Changed
}

pub(super) enum PageAction {
    JumpTo(usize),
    ScrollBy(isize),
}

pub(super) fn page_action(target: Option<usize>, visible_len: usize, down: bool) -> PageAction {
    target.map_or_else(
        || {
            let delta = visible_delta(visible_len);
            PageAction::ScrollBy(if down { delta } else { -delta })
        },
        PageAction::JumpTo,
    )
}
