// SPDX-License-Identifier: EUPL-1.2

//! column widths written out by hand, so the screen model does not agree with
//! screw by sharing its width crates. the tables cover the scripts and emoji
//! the tests use, anything else counts as one column

const ZERO: &[(u32, u32)] = &[
    (0x0300, 0x036F),
    (0x200B, 0x200F),
    (0x2060, 0x2064),
    (0x20D0, 0x20FF),
    (0xFE00, 0xFE0F),
    (0x1F3FB, 0x1F3FF),
];

const WIDE: &[(u32, u32)] = &[
    (0x1100, 0x115F),
    (0x231A, 0x231B),
    (0x23E9, 0x23EC),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267F, 0x267F),
    (0x2693, 0x2693),
    (0x26A1, 0x26A1),
    (0x26AA, 0x26AB),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x2705, 0x2705),
    (0x270A, 0x270B),
    (0x2728, 0x2728),
    (0x274C, 0x274C),
    (0x2753, 0x2755),
    (0x2B50, 0x2B50),
    (0x2E80, 0x303E),
    (0x3041, 0x33FF),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xA000, 0xA4CF),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE30, 0xFE6F),
    (0xFF00, 0xFF60),
    (0xFFE0, 0xFFE6),
    (0x1F300, 0x1F64F),
    (0x1F680, 0x1F6FF),
    (0x1F900, 0x1F9FF),
    (0x1FA70, 0x1FAFF),
    (0x20000, 0x3FFFD),
];

/// text-default symbols that an emoji presentation selector widens to two
/// columns
const EMOJI_BASES: &[(u32, u32)] = &[
    (0x0023, 0x0023),
    (0x002A, 0x002A),
    (0x0030, 0x0039),
    (0x00A9, 0x00A9),
    (0x00AE, 0x00AE),
    (0x203C, 0x203C),
    (0x2049, 0x2049),
    (0x2122, 0x2122),
    (0x2139, 0x2139),
    (0x2194, 0x2199),
    (0x21A9, 0x21AA),
    (0x2600, 0x2604),
    (0x260E, 0x260E),
    (0x2611, 0x2611),
    (0x2618, 0x2618),
    (0x261D, 0x261D),
    (0x2620, 0x2620),
    (0x2622, 0x2623),
    (0x2626, 0x2626),
    (0x262A, 0x262A),
    (0x2640, 0x2640),
    (0x2642, 0x2642),
    (0x2660, 0x2660),
    (0x2663, 0x2663),
    (0x2665, 0x2666),
    (0x2668, 0x2668),
    (0x267B, 0x267B),
    (0x2699, 0x2699),
    (0x26A0, 0x26A0),
    (0x2702, 0x2702),
    (0x2708, 0x2709),
    (0x2714, 0x2714),
    (0x2716, 0x2716),
    (0x2733, 0x2734),
    (0x2744, 0x2744),
    (0x2747, 0x2747),
    (0x2763, 0x2764),
    (0x27A1, 0x27A1),
    (0x2B05, 0x2B07),
];

const ZWJ: char = '\u{200d}';
const VS16: char = '\u{fe0f}';

fn within(table: &[(u32, u32)], ch: char) -> bool {
    let code = u32::from(ch);
    table
        .iter()
        .any(|&(low, high)| (low..=high).contains(&code))
}

fn regional_indicator(ch: char) -> bool {
    ('\u{1f1e6}'..='\u{1f1ff}').contains(&ch)
}

/// columns one character takes on its own, zero when it only decorates the cell
/// before it
pub fn columns(ch: char) -> usize {
    if ch == '\u{ad}' {
        1
    } else if ch.is_control() || within(ZERO, ch) {
        0
    } else if within(WIDE, ch) {
        2
    } else {
        1
    }
}

/// whether `ch` continues the cell holding `cell` instead of starting a new one
pub fn joins(cell: &str, ch: char) -> bool {
    let mut chars = cell.chars();
    let lone_indicator =
        matches!((chars.next(), chars.next()), (Some(first), None) if regional_indicator(first));
    columns(ch) == 0 || cell.ends_with(ZWJ) || (lone_indicator && regional_indicator(ch))
}

/// columns a whole cell takes, an emoji presentation selector makes a symbol
/// from [`EMOJI_BASES`] two wide and two regional indicators make one flag
pub fn cell(text: &str) -> usize {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return 0;
    };
    let flag = regional_indicator(first) && chars.clone().next().is_some_and(regional_indicator);
    if flag || (text.contains(VS16) && within(EMOJI_BASES, first)) {
        2
    } else {
        columns(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_characters_and_clusters_take_their_unicode_widths() {
        assert_eq!(columns('a'), 1);
        assert_eq!(columns('世'), 2);
        assert_eq!(columns('한'), 2);
        assert_eq!(columns('💻'), 2);
        assert_eq!(columns('\u{301}'), 0);
        assert_eq!(columns('\u{ad}'), 1);
        assert_eq!(cell("👩\u{200d}💻"), 2);
        assert_eq!(cell("🇯🇵"), 2);
        assert_eq!(cell("\u{2764}\u{fe0f}"), 2);
        assert_eq!(cell("1\u{fe0f}\u{20e3}"), 2);
        assert_eq!(cell("e\u{301}"), 1);
        assert_eq!(cell(" \u{fe0f}"), 1);
        assert_eq!(cell("a\u{fe0f}"), 1);
        assert_eq!(cell("ก\u{fe0f}"), 1);
    }

    #[test]
    fn the_emoji_bases_are_widened_by_screw_too() {
        for &(low, high) in EMOJI_BASES {
            for ch in (low..=high).filter_map(char::from_u32) {
                let text = format!("{ch}\u{fe0f}");
                assert_eq!(screw::width(&text), cell(&text), "{text:?}");
            }
        }
    }

    #[test]
    fn cells_continue_through_joiners_and_flag_pairs_only() {
        assert!(joins("e", '\u{301}'));
        assert!(joins("👩\u{200d}", '💻'));
        assert!(joins("🇯", '🇵'));
        assert!(!joins("🇯🇵", '🇺'));
        assert!(!joins("a", 'b'));
        assert!(!joins("世", '界'));
    }
}
