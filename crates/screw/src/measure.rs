// SPDX-License-Identifier: EUPL-1.2

use alloc::borrow::Cow;
use core::iter;

use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

/// horizontal alignment of text inside a fixed number of columns
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Align {
    /// starts at the first column
    #[default]
    Left,
    /// ends at the last column
    Right,
    /// centred, with any odd column placed after
    Center,
}

/// distance between tab stops, a tab expands to spaces up to the next one
pub(crate) const TAB_STOP: usize = 8;

/// whether a character has no cell representation and is dropped on write
pub(crate) fn is_dropped_control(ch: char) -> bool {
    ch.is_control() && ch != '\n' && ch != '\t'
}

/// one unit of text as laid out on a row
#[derive(Clone, Copy)]
pub(crate) enum Segment<'a> {
    Newline,
    Tab,
    /// extended grapheme cluster, zero width means it joins the prior cell and
    /// `grow` adds width
    Cluster {
        text:  &'a str,
        width: usize,
        grow:  usize,
    },
}

impl Segment<'_> {
    /// the column after this segment when it starts at `col`
    pub(crate) const fn advance(&self, col: usize) -> usize {
        match self {
            Self::Newline => 0,
            Self::Tab => col + TAB_STOP - col % TAB_STOP,
            Self::Cluster { width, grow, .. } => col + *width + *grow,
        }
    }
}

/// characters of context that decide how a joining mark changes the width of
/// its base
const JOIN_WINDOW: usize = 8;

/// longest cell text, in bytes, that a later write may still extend into a
/// larger cluster
pub(crate) const JOIN_CELL_LIMIT: usize = 64;

/// byte offset where the last [`JOIN_WINDOW`] characters of `text` begin
pub(crate) fn join_window_start(text: &str) -> usize {
    text.char_indices()
        .rev()
        .nth(JOIN_WINDOW - 1)
        .map_or(0, |(index, _)| index)
}

/// the segments [`Surface::write`](crate::Surface::write) lays out, with byte
/// offsets
pub(crate) fn segments(text: &str) -> impl Iterator<Item = (usize, Segment<'_>)> {
    let mut base: Option<usize> = None;
    let mut window = String::new();
    text.grapheme_indices(true)
        .filter_map(move |(index, cluster)| {
            let segment = match cluster {
                "\n" | "\r\n" => {
                    base = None;
                    Segment::Newline
                },
                "\t" => {
                    base = None;
                    Segment::Tab
                },
                _ if cluster.chars().next().is_some_and(is_dropped_control) => return None,
                _ => {
                    let width = if cluster == "\u{ad}" {
                        1
                    } else {
                        cluster.width()
                    };
                    let mut grow = 0;
                    if width > 0 {
                        base = Some(width);
                        window.clear();
                        window.push_str(&cluster[join_window_start(cluster)..]);
                    } else if let Some(head_width) = base {
                        let before = window.width();
                        window.push_str(cluster);
                        grow = window.width().saturating_sub(before);
                        base = Some(head_width + grow);
                        window.drain(..join_window_start(&window));
                    }
                    Segment::Cluster {
                        text: cluster,
                        width,
                        grow,
                    }
                },
            };
            Some((index, segment))
        })
}

/// expands tabs in one row from column zero and drops the controls
/// `Surface::write` drops
pub(crate) fn expand_tabs(text: &str) -> Cow<'_, str> {
    if !text.contains('\t') && !text.chars().any(is_dropped_control) {
        return Cow::Borrowed(text);
    }
    let mut expanded = String::with_capacity(text.len());
    let mut col = 0;
    for (_, segment) in segments(text) {
        match segment {
            Segment::Newline => {},
            Segment::Tab => expanded.extend(iter::repeat_n(' ', segment.advance(col) - col)),
            Segment::Cluster { text: cluster, .. } => expanded.push_str(cluster),
        }
        col = segment.advance(col);
    }
    Cow::Owned(expanded)
}

/// byte offset just past the first visible segment of `text`, or its length
/// when it has none
pub(crate) fn first_segment_end(text: &str) -> usize {
    let mut visible = segments(text);
    visible.next();
    visible
        .find(|(_, segment)| !matches!(segment, Segment::Cluster { width: 0, .. }))
        .map_or(text.len(), |(index, _)| index)
}

/// display width of `text` in terminal columns, that of its widest row
///
/// tabs advance to the next multiple of eight columns, other controls are
/// ignored
#[must_use]
pub fn width(text: &str) -> usize {
    let mut widest = 0;
    let mut row = 0;
    for (_, segment) in segments(text) {
        row = segment.advance(row);
        widest = widest.max(row);
    }
    widest
}

/// the longest prefix of the first row of `text` that fits in `columns`, never
/// splitting a cluster
#[must_use]
pub fn truncate(text: &str, columns: usize) -> &str {
    let mut used = 0;
    let mut start = 0;
    for (index, segment) in segments(text) {
        if matches!(segment, Segment::Newline) {
            return &text[..index];
        }
        let next = segment.advance(used);
        let widens = matches!(segment, Segment::Cluster {
            width: 0,
            grow: 1..,
            ..
        });
        if next > columns {
            return &text[..if widens { start } else { index }];
        }
        if !matches!(segment, Segment::Cluster { width: 0, .. }) {
            start = index;
        }
        used = next;
    }
    text
}

const fn is_line_break(ch: char) -> bool {
    matches!(ch, '\r' | '\x0b' | '\x0c' | '\u{85}')
}

/// `text` reduced to one printable line, dropped controls are removed and
/// breaks become spaces
///
/// ```
/// assert_eq!(screw::single_line("a\x1b[0m\nb\tc"), "a[0m b c");
/// assert_eq!(screw::single_line("plain"), "plain");
/// ```
#[must_use]
pub fn single_line(text: &str) -> Cow<'_, str> {
    if !text.chars().any(char::is_control) {
        return Cow::Borrowed(text);
    }
    let mut line = String::with_capacity(text.len());
    for (index, part) in text.replace("\r\n", "\n").split(is_line_break).enumerate() {
        if index > 0 {
            line.push(' ');
        }
        for (_, segment) in segments(part) {
            match segment {
                Segment::Newline | Segment::Tab => line.push(' '),
                Segment::Cluster { text: cluster, .. } => line.push_str(cluster),
            }
        }
    }
    Cow::Owned(line)
}

/// `missing` columns of padding split into the amounts before and after text
pub(crate) const fn split_padding(missing: usize, align: Align) -> (usize, usize) {
    match align {
        Align::Left => (0, missing),
        Align::Right => (missing, 0),
        Align::Center => (missing / 2, missing - missing / 2),
    }
}

/// pads each row of `text` with spaces to `columns` according to `align`
///
/// rows already `columns` wide are unchanged, use [`truncate`] first to enforce
/// a maximum
#[must_use]
pub fn pad(text: &str, columns: usize, align: Align) -> Cow<'_, str> {
    let rows: Vec<Cow<'_, str>> = text
        .split('\n')
        .map(|row| pad_row(row, columns, align))
        .collect();
    if rows.iter().all(|row| matches!(row, Cow::Borrowed(_))) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(rows.join("\n"))
}

fn pad_row(row: &str, columns: usize, align: Align) -> Cow<'_, str> {
    let missing = columns.saturating_sub(width(row));
    if missing == 0 {
        return Cow::Borrowed(row);
    }
    let (before, after) = split_padding(missing, align);
    let content = if before > 0 && row.contains('\t') {
        expand_tabs(row)
    } else {
        Cow::Borrowed(row)
    };
    Cow::Owned(format!(
        "{}{content}{}",
        " ".repeat(before),
        " ".repeat(after)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Style,
        Surface,
    };

    const SAMPLES: [&str; 16] = [
        "plain",
        "世界",
        "a世b",
        "😀x",
        "e\u{301}x",
        "tab\there\u{1b}[0m",
        "one\ntwelve\n3",
        "",
        "\u{2764}\u{fe0f}x",
        "\u{26a0}\u{fe0f} \u{2714}\u{fe0f}",
        "👩\u{200d}💻!",
        "🇯🇵🇺🇸",
        "1\u{fe0f}\u{20e3}",
        "한국어 e\u{301}\u{302}",
        "a\r\nb",
        "\u{200d}x",
    ];

    #[test]
    fn width_counts_columns_not_chars() {
        assert_eq!(width("plain"), 5);
        assert_eq!(width("世界"), 4);
        assert_eq!(width("😀x"), 3);
        assert_eq!(width("e\u{301}x"), 2);
        assert_eq!(width(""), 0);
    }

    #[test]
    fn width_ignores_dropped_controls_and_reports_the_widest_row() {
        assert_eq!(width("a\u{1b}b\u{7f}c"), 3);
        assert_eq!(width("ab\ncdef\ng"), 4);
    }

    #[test]
    fn width_matches_what_a_surface_lays_out() {
        for sample in SAMPLES {
            let mut surface = Surface::new();
            surface.write(sample, Style::PLAIN);
            assert_eq!(width(sample), surface.display_width(), "{sample:?}");
        }
    }

    #[test]
    fn width_measures_whole_grapheme_clusters() {
        assert_eq!(width("\u{2764}\u{fe0f}"), 2);
        assert_eq!(width("\u{26a0}\u{fe0f}"), 2);
        assert_eq!(width("\u{2714}\u{fe0f}"), 2);
        assert_eq!(width("👩\u{200d}💻"), 2);
        assert_eq!(width("🇯🇵"), 2);
        assert_eq!(width("🇯🇵🇺🇸"), 4);
        assert_eq!(width("1\u{fe0f}\u{20e3}"), 2);
        assert_eq!(width("e\u{301}\u{302}"), 1);
        assert_eq!(width("한국"), 4);
    }

    #[test]
    fn marks_join_their_base_across_dropped_controls() {
        let text = "\u{2764}\u{1b}\u{fe0f}X";
        let mut surface = Surface::new();
        surface.write(text, Style::PLAIN);
        assert_eq!(width(text), 3);
        assert_eq!(width(text), surface.display_width());
        assert_eq!(truncate(text, 1), "");
        assert_eq!(truncate(text, 2), "\u{2764}\u{1b}\u{fe0f}");
        assert_eq!(truncate(text, 3), text);
        assert_eq!(pad(text, 5, Align::Left), format!("{text}  "));
        assert_eq!(pad(text, 5, Align::Right), format!("  {text}"));
    }

    #[test]
    fn single_line_breaks_at_every_control_line_break() {
        assert_eq!(
            single_line("a\rb\x0bc\x0cd\u{85}e\nf\r\ng"),
            "a b c d e f g"
        );
        assert_eq!(single_line("a\r\n\r\nb"), "a  b");
    }

    #[test]
    fn a_soft_hyphen_takes_one_column_like_xterm_draws_it() {
        assert_eq!(width("co\u{ad}op"), 5);
        let mut surface = Surface::new();
        surface.write("co\u{ad}op", Style::PLAIN);
        assert_eq!(surface.display_width(), 5);
        assert_eq!(truncate("co\u{ad}op", 3), "co\u{ad}");
    }

    #[test]
    fn width_expands_tabs_to_the_next_stop() {
        assert_eq!(width("\t"), 8);
        assert_eq!(width("ab\tc"), 9);
        assert_eq!(width("12345678\tx"), 17);
        assert_eq!(width("ab\n\tc"), 9);
    }

    #[test]
    fn pad_expands_tabs_before_prepending_padding() {
        let right = pad("a\tb", 12, Align::Right);
        assert_eq!(width(&right), 12);
        assert_eq!(right, "   a       b");
        assert_eq!(width(&pad("a\tb", 12, Align::Center)), 12);
        assert_eq!(pad("a\tb", 12, Align::Left), "a\tb   ");
    }

    #[test]
    fn truncate_never_splits_a_cluster() {
        assert_eq!(truncate("\u{26a0}\u{fe0f}a", 1), "");
        assert_eq!(truncate("\u{26a0}\u{fe0f}a", 2), "\u{26a0}\u{fe0f}");
        assert_eq!(truncate("\u{26a0}\u{fe0f}a", 3), "\u{26a0}\u{fe0f}a");
        assert_eq!(truncate("a👩\u{200d}💻b", 2), "a");
        assert_eq!(truncate("a👩\u{200d}💻b", 3), "a👩\u{200d}💻");
        assert_eq!(truncate("🇯🇵🇺🇸", 3), "🇯🇵");
        assert_eq!(truncate("ab\tc", 5), "ab");
        assert_eq!(truncate("ab\tc", 8), "ab\t");
    }

    #[test]
    fn truncate_never_splits_a_wide_character() {
        assert_eq!(truncate("a世b", 2), "a");
        assert_eq!(truncate("a世b", 3), "a世");
        assert_eq!(truncate("世界", 3), "世");
        assert_eq!(truncate("😀😀", 1), "");
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("short", 0), "");
    }

    #[test]
    fn truncate_keeps_trailing_combining_marks_and_stops_at_newline() {
        assert_eq!(truncate("e\u{301}xyz", 1), "e\u{301}");
        assert_eq!(truncate("ab\ncd", 9), "ab");
    }

    #[test]
    fn truncated_text_fits_the_requested_columns() {
        for sample in SAMPLES {
            for columns in 0..6 {
                assert!(width(truncate(sample, columns)) <= columns, "{sample:?}");
            }
        }
    }

    #[test]
    fn pad_fills_to_the_requested_columns_by_display_width() {
        assert_eq!(pad("ab", 5, Align::Left), "ab   ");
        assert_eq!(pad("ab", 5, Align::Right), "   ab");
        assert_eq!(pad("ab", 5, Align::Center), " ab  ");
        assert_eq!(pad("世", 4, Align::Left), "世  ");
        assert_eq!(pad("e\u{301}", 3, Align::Right), "  e\u{301}");
    }

    #[test]
    fn pad_fills_every_row_of_multiline_text() {
        assert_eq!(pad("a\nbbb\nc", 5, Align::Left), "a    \nbbb  \nc    ");
        assert_eq!(pad("a\nbbb", 5, Align::Right), "    a\n  bbb");
        assert_eq!(pad("a\nbbb", 5, Align::Center), "  a  \n bbb ");
    }

    #[test]
    fn pad_borrows_text_that_already_fits() {
        assert!(matches!(pad("abcd", 4, Align::Left), Cow::Borrowed("abcd")));
        assert!(matches!(
            pad("abcdef", 4, Align::Right),
            Cow::Borrowed("abcdef")
        ));
    }
}
