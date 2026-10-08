// SPDX-License-Identifier: EUPL-1.2

use std::borrow::Cow;

use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

/// Horizontal alignment of text inside a fixed number of columns.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Align {
    /// Text starts at the first column.
    #[default]
    Left,
    /// Text ends at the last column.
    Right,
    /// Text is centred, with any odd column placed after it.
    Center,
}

/// Distance between tab stops. A tab is expanded to spaces up to the next one.
pub(crate) const TAB_STOP: usize = 8;

/// Whether a character has no cell representation and is dropped on write.
pub(crate) fn is_dropped_control(ch: char) -> bool {
    ch.is_control() && ch != '\n' && ch != '\t'
}

/// One unit of text as laid out on a row.
#[derive(Clone, Copy)]
pub(crate) enum Segment<'a> {
    Newline,
    Tab,
    /// An extended grapheme cluster. A zero width means it attaches to the prior cell.
    Cluster {
        text:  &'a str,
        width: usize,
    },
}

impl Segment<'_> {
    /// The column after this segment when it starts at `col`.
    pub(crate) const fn advance(&self, col: usize) -> usize {
        match self {
            Self::Newline => 0,
            Self::Tab => col + TAB_STOP - col % TAB_STOP,
            Self::Cluster { width, .. } => col + *width,
        }
    }
}

/// Splits `text` into the segments [`Surface::write`](crate::Surface::write) lays out, each with
/// its byte offset. Dropped controls produce nothing.
pub(crate) fn segments(text: &str) -> impl Iterator<Item = (usize, Segment<'_>)> {
    text.grapheme_indices(true).filter_map(|(index, cluster)| {
        let segment = match cluster {
            "\n" | "\r\n" => Segment::Newline,
            "\t" => Segment::Tab,
            _ if cluster.chars().next().is_some_and(is_dropped_control) => return None,
            _ => {
                Segment::Cluster {
                    text:  cluster,
                    width: cluster.width(),
                }
            },
        };
        Some((index, segment))
    })
}

/// Display width of `text` in terminal columns.
///
/// Measures exactly what [`Surface::write`](crate::Surface::write) would lay out. Each extended
/// grapheme cluster is measured whole with `unicode-width`, so an emoji with a variation selector
/// or a joined sequence takes the columns that crate reports and combining marks add nothing.
/// Terminals that do not shape such sequences draw them at another width, so the result follows a
/// policy that depends on the terminal. Tabs advance to the next multiple of
/// eight columns, other control characters are ignored, and a newline starts a
/// new row. The width of the widest row is returned.
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

/// The longest prefix of the first row of `text` that fits in `columns`.
///
/// A grapheme cluster or tab that would straddle the limit is left out, and
/// combining marks that follow the last cluster kept stay attached to it.
#[must_use]
pub fn truncate(text: &str, columns: usize) -> &str {
    let mut used = 0;
    for (index, segment) in segments(text) {
        if matches!(segment, Segment::Newline) {
            return &text[..index];
        }
        let next = segment.advance(used);
        if next > columns {
            return &text[..index];
        }
        used = next;
    }
    text
}

/// Pads `text` with spaces to `columns` according to `align`.
///
/// Text that is already `columns` wide or wider is returned unchanged. Use
/// [`truncate`] first to enforce a maximum.
#[must_use]
pub fn pad(text: &str, columns: usize, align: Align) -> Cow<'_, str> {
    let missing = columns.saturating_sub(width(text));
    if missing == 0 {
        return Cow::Borrowed(text);
    }
    let (before, after) = match align {
        Align::Left => (0, missing),
        Align::Right => (missing, 0),
        Align::Center => (missing / 2, missing - missing / 2),
    };
    Cow::Owned(format!("{}{text}{}", " ".repeat(before), " ".repeat(after)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Style, Surface};

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
    fn width_expands_tabs_to_the_next_stop() {
        assert_eq!(width("\t"), 8);
        assert_eq!(width("ab\tc"), 9);
        assert_eq!(width("12345678\tx"), 17);
        assert_eq!(width("ab\n\tc"), 9);
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
    fn pad_borrows_text_that_already_fits() {
        assert!(matches!(pad("abcd", 4, Align::Left), Cow::Borrowed("abcd")));
        assert!(matches!(pad("abcdef", 4, Align::Right), Cow::Borrowed("abcdef")));
    }
}
