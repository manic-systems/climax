// SPDX-License-Identifier: EUPL-1.2

use core::{
    ops::Deref,
    str,
};

use bang_core::{
    Event,
    Key,
    KeyEvent,
    Modifiers,
};

const ESC: u8 = 0x1B;
const BACKSPACE: u8 = 0x7F;
const CTRL_H: u8 = 0x08;
const CTRL_C: u8 = 0x03;
const CTRL_D: u8 = 0x04;
const TAB: u8 = b'\t';
const LF: u8 = b'\n';
const CR: u8 = b'\r';
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";
const MAX_SEQUENCE_LEN: usize = 64;

/// incremental decoder from terminal bytes to bang input events
#[derive(Debug, Default)]
pub struct Decoder {
    pending:  Pending,
    paste:    Vec<u8>,
    in_paste: bool,
    skipping: bool,
}

impl Decoder {
    /// an empty decoder
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending:  Pending::new(),
            paste:    Vec::new(),
            in_paste: false,
            skipping: false,
        }
    }

    /// feeds bytes and returns complete events, invalid bytes become
    /// `UnknownEscape`
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.pending.extend(bytes);
        let mut events = Vec::new();

        loop {
            if self.in_paste {
                if let Some(end) = find_subslice(&self.pending, PASTE_END) {
                    self.paste.extend_from_slice(&self.pending[..end]);
                    self.pending.skip(end + PASTE_END.len());
                    self.in_paste = false;
                    events.push(Event::Paste(
                        String::from_utf8_lossy(&self.paste).into_owned(),
                    ));
                    self.paste.clear();
                    continue;
                }

                let keep = partial_suffix_len(&self.pending, PASTE_END);
                let paste_len = self.pending.len().saturating_sub(keep);
                self.paste.extend_from_slice(&self.pending[..paste_len]);
                self.pending.skip(paste_len);
                break;
            }

            if self.skipping {
                let end = self
                    .pending
                    .iter()
                    .position(|byte| !(0x20..=0x3F).contains(byte));
                let len = end.map_or(self.pending.len(), |at| {
                    at + usize::from((0x40..=0x7E).contains(&self.pending[at]))
                });
                self.pending.skip(len);
                self.skipping = end.is_none();
            }

            if self.pending.is_empty() {
                break;
            }

            if self.pending.starts_with(PASTE_START) {
                self.pending.skip(PASTE_START.len());
                self.in_paste = true;
                continue;
            }

            let first = self.pending[0];
            match first {
                ESC => {
                    match self.decode_escape() {
                        EscapeResult::Event(event) => events.push(event),
                        EscapeResult::Pending => break,
                        EscapeResult::Unknown(sequence) => {
                            events.push(Event::UnknownEscape(sequence));
                        },
                    }
                },
                CR | LF => {
                    self.pending.skip(1);
                    events.push(Event::key(Key::Enter));
                },
                TAB => {
                    self.pending.skip(1);
                    events.push(Event::key(Key::Tab));
                },
                BACKSPACE | CTRL_H => {
                    self.pending.skip(1);
                    events.push(Event::key(Key::Backspace));
                },
                CTRL_C => {
                    self.pending.skip(1);
                    events.push(control_char('c'));
                },
                CTRL_D => {
                    self.pending.skip(1);
                    events.push(control_char('d'));
                },
                0x00..=0x1F => {
                    self.pending.skip(1);
                    events.push(control_char(control_symbol(first)));
                },
                _ => {
                    match decode_utf8_prefix(&self.pending) {
                        Utf8Result::Char(value, len) => {
                            self.pending.skip(len);
                            events.push(Event::char(value));
                        },
                        Utf8Result::Pending => break,
                        Utf8Result::Invalid => {
                            events.push(Event::UnknownEscape(self.pending.take(1)));
                        },
                    }
                },
            }
        }

        self.pending.compact();
        events
    }

    /// drains buffered input at EOF, resolving ambiguous prefixes as a timeout
    /// would
    pub fn flush(&mut self) -> Vec<Event> {
        self.skipping = false;
        let mut events = Vec::new();
        if self.in_paste {
            self.paste.extend_from_slice(&self.pending);
            self.pending.clear();
            self.in_paste = false;
            events.push(Event::Paste(
                String::from_utf8_lossy(&self.paste).into_owned(),
            ));
            self.paste.clear();
            return events;
        }

        while !self.pending.is_empty() {
            if let Some(event) = self.flush_escape() {
                events.push(event);
                continue;
            }
            if self.pending[0] == ESC {
                self.pending.skip(1);
                events.push(Event::key(Key::Esc));
                continue;
            }

            events.extend(self.feed(&[]));
            if !self.pending.is_empty() {
                events.push(Event::UnknownEscape(self.pending.take(1)));
            }
        }

        events
    }

    #[must_use]
    pub(crate) const fn in_paste(&self) -> bool {
        self.in_paste
    }

    #[must_use]
    pub(crate) fn escape_state(&self) -> EscapeState {
        if self.in_paste || self.pending.first() != Some(&ESC) {
            return EscapeState::None;
        }
        match self.pending.get(1) {
            None => EscapeState::Ambiguous,
            Some(b'[' | b'O') if self.pending.len() == 2 => EscapeState::Ambiguous,
            // a CSI with parameters but no final byte yet can only be finished by bytes still in
            // flight
            Some(b'[' | b'O') => EscapeState::Incomplete,
            Some(&ESC) => {
                match self.pending.get(2) {
                    None => EscapeState::Ambiguous,
                    Some(b'[' | b'O') if self.pending.len() == 3 => EscapeState::Ambiguous,
                    Some(b'[' | b'O') => EscapeState::Incomplete,
                    Some(_) => EscapeState::None,
                }
            },
            Some(_) => {
                match decode_utf8_prefix(&self.pending[1..]) {
                    Utf8Result::Pending => EscapeState::Incomplete,
                    Utf8Result::Char(..) | Utf8Result::Invalid => EscapeState::None,
                }
            },
        }
    }

    /// resolves an ambiguous Escape after a timeout, `ESC [` and `ESC O` become
    /// Alt keys
    pub(crate) fn flush_escape(&mut self) -> Option<Event> {
        if self.escape_state() != EscapeState::Ambiguous {
            return None;
        }
        let event = match self.pending.get(1) {
            None => Event::key(Key::Esc),
            Some(&ESC) => {
                self.pending.skip(1);
                return Some(Event::key(Key::Esc));
            },
            Some(&byte) => {
                Event::Key(KeyEvent::with_modifiers(
                    Key::Char(char::from(byte)),
                    Modifiers::ALT,
                ))
            },
        };
        self.pending.clear();
        Some(event)
    }

    fn decode_escape(&mut self) -> EscapeResult {
        if self.pending.len() == 1 {
            return EscapeResult::Pending;
        }

        match self.pending[1] {
            b'[' => self.decode_csi(),
            b'O' => self.decode_ss3(),
            ESC => self.decode_double_escape(),
            CR | LF => self.consume_modified_key(2, Key::Enter, Modifiers::ALT),
            TAB => self.consume_modified_key(2, Key::Tab, Modifiers::ALT),
            BACKSPACE | CTRL_H => self.consume_modified_key(2, Key::Backspace, Modifiers::ALT),
            0x00..=0x1F => {
                let value = control_symbol(self.pending[1]);
                self.consume_modified_key(2, Key::Char(value), Modifiers::ALT | Modifiers::CONTROL)
            },
            _ => {
                match decode_utf8_prefix(&self.pending[1..]) {
                    Utf8Result::Char(value, len) => {
                        self.consume_modified_key(len + 1, Key::Char(value), Modifiers::ALT)
                    },
                    Utf8Result::Pending => EscapeResult::Pending,
                    Utf8Result::Invalid => EscapeResult::Unknown(self.pending.take(1)),
                }
            },
        }
    }

    fn decode_double_escape(&mut self) -> EscapeResult {
        match self.pending.get(2) {
            None => EscapeResult::Pending,
            Some(b'[' | b'O') => {
                self.pending.skip(1);
                match self.decode_escape() {
                    EscapeResult::Event(Event::Key(mut key)) => {
                        key.modifiers |= Modifiers::ALT;
                        EscapeResult::Event(Event::Key(key))
                    },
                    EscapeResult::Pending => {
                        self.pending.unskip(1);
                        EscapeResult::Pending
                    },
                    EscapeResult::Unknown(mut sequence) => {
                        sequence.insert(0, ESC);
                        EscapeResult::Unknown(sequence)
                    },
                    other @ EscapeResult::Event(_) => other,
                }
            },
            Some(_) => {
                self.pending.skip(1);
                EscapeResult::Event(Event::key(Key::Esc))
            },
        }
    }

    fn decode_csi(&mut self) -> EscapeResult {
        if self.pending.get(2) == Some(&b'[') {
            return self.decode_linux_function_key();
        }

        let mut index = 2;
        loop {
            if index == MAX_SEQUENCE_LEN {
                self.skipping = true;
                return EscapeResult::Unknown(self.pending.take(index));
            }
            let Some(&byte) = self.pending.get(index) else {
                return EscapeResult::Pending;
            };
            if (0x20..=0x3F).contains(&byte) {
                index += 1;
                continue;
            }
            if !(0x40..=0x7E).contains(&byte) {
                // not a parameter byte and not a final byte, this CSI has no
                // end, so stop before it and let the next loop
                // decode it fresh
                return EscapeResult::Unknown(self.pending.take(index));
            }
            let parameters = &self.pending[2..index];
            let Some((key, modifiers)) = csi_key(parameters, byte) else {
                return EscapeResult::Unknown(self.pending.take(index + 1));
            };
            return self.consume_modified_key(index + 1, key, modifiers);
        }
    }

    fn decode_linux_function_key(&mut self) -> EscapeResult {
        let Some(&byte) = self.pending.get(3) else {
            return EscapeResult::Pending;
        };
        let key = match byte {
            b'A'..=b'E' => Key::Function(byte - b'A' + 1),
            _ => return EscapeResult::Unknown(self.pending.take(4)),
        };
        self.consume_key(4, key)
    }

    fn decode_ss3(&mut self) -> EscapeResult {
        let mut index = 2;
        loop {
            if index == MAX_SEQUENCE_LEN {
                self.skipping = true;
                return EscapeResult::Unknown(self.pending.take(index));
            }
            let Some(&byte) = self.pending.get(index) else {
                return EscapeResult::Pending;
            };
            if (0x30..=0x3F).contains(&byte) {
                index += 1;
                continue;
            }
            if !(0x40..=0x7E).contains(&byte) {
                return EscapeResult::Unknown(self.pending.take(index));
            }
            let Some((key, modifiers)) = ss3_key(&self.pending[2..index], byte) else {
                return EscapeResult::Unknown(self.pending.take(index + 1));
            };
            return self.consume_modified_key(index + 1, key, modifiers);
        }
    }

    fn consume_modified_key(&mut self, len: usize, key: Key, modifiers: Modifiers) -> EscapeResult {
        if self.pending.len() < len {
            return EscapeResult::Pending;
        }
        if len == 0 {
            return EscapeResult::Unknown(self.pending.take(1));
        }
        self.pending.skip(len);
        EscapeResult::Event(Event::Key(KeyEvent::with_modifiers(key, modifiers)))
    }

    fn consume_key(&mut self, len: usize, key: Key) -> EscapeResult {
        self.pending.skip(len);
        EscapeResult::Event(Event::key(key))
    }
}

fn csi_key(parameters: &[u8], final_byte: u8) -> Option<(Key, Modifiers)> {
    let values = parse_parameters(parameters)?;
    let modifier = values
        .get(1)
        .copied()
        .map_or(Some(Modifiers::empty()), xterm_modifiers)?;
    let key = match final_byte {
        b'Z' if values.is_empty() => return Some((Key::Backtab, Modifiers::empty())),
        b'~' => tilde_key(*values.first()?)?,
        _ => final_key(final_byte)?,
    };
    Some((key, modifier))
}

fn ss3_key(parameters: &[u8], final_byte: u8) -> Option<(Key, Modifiers)> {
    let values = parse_parameters(parameters)?;
    if values.len() > 1 {
        return None;
    }
    let modifier = values
        .first()
        .copied()
        .map_or(Some(Modifiers::empty()), xterm_modifiers)?;
    Some((final_key(final_byte)?, modifier))
}

const fn final_key(final_byte: u8) -> Option<Key> {
    Some(match final_byte {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        b'P'..=b'S' => Key::Function(final_byte - b'P' + 1),
        _ => return None,
    })
}

fn parse_parameters(bytes: &[u8]) -> Option<Vec<u16>> {
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    bytes
        .split(|byte| *byte == b';')
        .map(|part| str::from_utf8(part).ok()?.parse().ok())
        .collect()
}

fn xterm_modifiers(parameter: u16) -> Option<Modifiers> {
    let bits = match parameter {
        1 => Modifiers::empty(),
        2 => Modifiers::SHIFT,
        3 => Modifiers::ALT,
        4 => Modifiers::SHIFT | Modifiers::ALT,
        5 => Modifiers::CONTROL,
        6 => Modifiers::SHIFT | Modifiers::CONTROL,
        7 => Modifiers::ALT | Modifiers::CONTROL,
        8 => Modifiers::SHIFT | Modifiers::ALT | Modifiers::CONTROL,
        _ => return None,
    };
    Some(bits)
}

fn tilde_key(parameter: u16) -> Option<Key> {
    Some(match parameter {
        1 | 7 => Key::Home,
        2 => Key::Insert,
        3 => Key::Delete,
        4 | 8 => Key::End,
        5 => Key::PageUp,
        6 => Key::PageDown,
        11..=15 | 17..=21 | 23 | 24 => Key::Function(function_number(parameter)?),
        _ => return None,
    })
}

fn function_number(parameter: u16) -> Option<u8> {
    let gap = match parameter {
        11..=15 => 10,
        17..=21 => 11,
        _ => 12,
    };
    u8::try_from(parameter - gap).ok()
}

#[cfg(test)]
fn decode_all(bytes: &[u8]) -> Vec<Event> {
    let mut decoder = Decoder::new();
    let mut events = decoder.feed(bytes);
    events.extend(decoder.flush());
    events
}

fn control_symbol(byte: u8) -> char {
    match byte {
        0 => ' ',
        0x01..=0x1A => char::from(b'a' + byte - 1),
        _ => char::from(byte + 0x40),
    }
}

const fn control_char(value: char) -> Event {
    Event::Key(KeyEvent::with_modifiers(
        Key::Char(value),
        Modifiers::CONTROL,
    ))
}

/// undecoded bytes, consuming advances `head` instead of shifting the tail once
/// per event
#[derive(Debug, Default)]
struct Pending {
    bytes: Vec<u8>,
    head:  usize,
}

impl Pending {
    const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            head:  0,
        }
    }

    fn extend(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn skip(&mut self, len: usize) {
        self.head += len;
        if self.head == self.bytes.len() {
            self.clear();
        }
    }

    const fn unskip(&mut self, len: usize) {
        self.head -= len;
    }

    fn take(&mut self, len: usize) -> Vec<u8> {
        let taken = self[..len].to_vec();
        self.skip(len);
        taken
    }

    fn clear(&mut self) {
        self.bytes.clear();
        self.head = 0;
    }

    fn compact(&mut self) {
        self.bytes.drain(..self.head);
        self.head = 0;
    }
}

impl Deref for Pending {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes[self.head..]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EscapeState {
    /// nothing is buffered that a read deadline could resolve
    None,
    /// more bytes are required, a deadline would resolve nothing
    Incomplete,
    /// a read deadline should resolve this prefix into a key
    Ambiguous,
}

#[derive(Debug, Eq, PartialEq)]
enum EscapeResult {
    Event(Event),
    Pending,
    Unknown(Vec<u8>),
}

#[derive(Debug, Eq, PartialEq)]
enum Utf8Result {
    Char(char, usize),
    Pending,
    Invalid,
}

fn decode_utf8_prefix(bytes: &[u8]) -> Utf8Result {
    let width = utf8_width(bytes[0]);
    if width == 0 {
        return Utf8Result::Invalid;
    }
    match str::from_utf8(&bytes[..bytes.len().min(width)]) {
        Ok(value) => {
            value
                .chars()
                .next()
                .map_or(Utf8Result::Invalid, |first| Utf8Result::Char(first, width))
        },
        Err(error) if error.error_len().is_none() => Utf8Result::Pending,
        Err(_) => Utf8Result::Invalid,
    }
}

const fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 0,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn partial_suffix_len(haystack: &[u8], needle: &[u8]) -> usize {
    (1..needle.len().min(haystack.len() + 1))
        .rev()
        .find(|length| haystack.ends_with(&needle[..*length]))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modified(key: Key, modifiers: Modifiers) -> Event {
        Event::Key(KeyEvent::with_modifiers(key, modifiers))
    }

    fn decode_in_chunks(bytes: &[u8], chunk_size: usize) -> Vec<Event> {
        let mut decoder = Decoder::new();
        let mut events = Vec::new();
        for chunk in bytes.chunks(chunk_size) {
            events.extend(decoder.feed(chunk));
        }
        events.extend(decoder.flush());
        events
    }

    #[test]
    fn alt_characters_and_named_controls_decode_without_losing_utf8() {
        for (bytes, expected) in [
            (
                b"\x1bq".as_slice(),
                modified(Key::Char('q'), Modifiers::ALT),
            ),
            (
                "\u{1b}界".as_bytes(),
                modified(Key::Char('界'), Modifiers::ALT),
            ),
            (b"\x1b\r".as_slice(), modified(Key::Enter, Modifiers::ALT)),
            (
                b"\x1b\x03".as_slice(),
                modified(Key::Char('c'), Modifiers::ALT | Modifiers::CONTROL),
            ),
        ] {
            for chunk_size in 1..=bytes.len() {
                assert_eq!(decode_in_chunks(bytes, chunk_size), vec![expected.clone()]);
            }
        }
    }

    #[test]
    fn xterm_modifiers_cover_arrows_and_navigation_keys() {
        let cases = [
            (b"\x1b[1;2A".as_slice(), Key::Up, Modifiers::SHIFT),
            (b"\x1b[1;3B".as_slice(), Key::Down, Modifiers::ALT),
            (b"\x1b[1;5C".as_slice(), Key::Right, Modifiers::CONTROL),
            (b"\x1b[1;5H".as_slice(), Key::Home, Modifiers::CONTROL),
            (b"\x1b[1;5F".as_slice(), Key::End, Modifiers::CONTROL),
            (
                b"\x1b[1;8D".as_slice(),
                Key::Left,
                Modifiers::SHIFT | Modifiers::ALT | Modifiers::CONTROL,
            ),
            (
                b"\x1b[1;6H".as_slice(),
                Key::Home,
                Modifiers::SHIFT | Modifiers::CONTROL,
            ),
            (
                b"\x1b[3;7~".as_slice(),
                Key::Delete,
                Modifiers::ALT | Modifiers::CONTROL,
            ),
        ];
        for (bytes, key, modifiers) in cases {
            for chunk_size in 1..=bytes.len() {
                assert_eq!(
                    decode_in_chunks(bytes, chunk_size),
                    vec![modified(key.clone(), modifiers)],
                    "bytes={bytes:?}, chunk_size={chunk_size}",
                );
            }
        }
    }

    #[test]
    fn bracketed_paste_markers_survive_every_chunk_boundary() {
        let bytes = b"\x1b[200~hello \x1b[ world\x1b[201~";
        for chunk_size in 1..=bytes.len() {
            assert_eq!(
                decode_in_chunks(bytes, chunk_size),
                vec![Event::Paste("hello \x1b[ world".into())],
                "chunk_size={chunk_size}",
            );
        }
    }

    #[test]
    fn escape_deadline_flush_preserves_other_partial_input() {
        let mut decoder = Decoder::new();
        assert!(decoder.feed(b"\x1b").is_empty());
        assert_eq!(decoder.escape_state(), EscapeState::Ambiguous);
        assert_eq!(decoder.flush_escape(), Some(Event::key(Key::Esc)));
        assert_eq!(decoder.escape_state(), EscapeState::None);

        assert!(decoder.feed(&[0xE7]).is_empty());
        assert_eq!(decoder.flush_escape(), None);
        assert_eq!(decoder.feed(&[0x95, 0x8C]), vec![Event::char('界')]);
    }

    #[test]
    fn escape_state_stays_none_for_a_partial_paste_end_marker() {
        let mut decoder = Decoder::new();
        assert!(decoder.feed(b"\x1b[200~abc\x1b").is_empty());
        assert_eq!(decoder.escape_state(), EscapeState::None);
        assert_eq!(decoder.feed(b"[201~x\r"), vec![
            Event::Paste("abc".into()),
            Event::char('x'),
            Event::key(Key::Enter)
        ],);
    }

    #[test]
    fn partial_escape_prefixes_are_classified_by_whether_a_deadline_can_resolve_them() {
        let cases = [
            (b"\x1b".as_slice(), EscapeState::Ambiguous),
            (b"\x1b[".as_slice(), EscapeState::Ambiguous),
            (b"\x1bO".as_slice(), EscapeState::Ambiguous),
            (b"\x1b[1".as_slice(), EscapeState::Incomplete),
            (b"\x1b[1;".as_slice(), EscapeState::Incomplete),
            (b"\x1b[20".as_slice(), EscapeState::Incomplete),
            ("\u{1b}界".as_bytes(), EscapeState::None),
            (b"".as_slice(), EscapeState::None),
            (b"a".as_slice(), EscapeState::None),
        ];
        for (bytes, expected) in cases {
            let mut decoder = Decoder::new();
            let _events = decoder.feed(bytes);
            assert_eq!(decoder.escape_state(), expected, "bytes={bytes:?}");
        }
    }

    #[test]
    fn a_deadline_resolves_alt_key_prefixes_without_consuming_the_next_key() {
        for (bytes, expected) in [
            (
                b"\x1b[".as_slice(),
                modified(Key::Char('['), Modifiers::ALT),
            ),
            (
                b"\x1bO".as_slice(),
                modified(Key::Char('O'), Modifiers::ALT),
            ),
            (b"\x1b".as_slice(), Event::key(Key::Esc)),
        ] {
            let mut decoder = Decoder::new();
            assert!(decoder.feed(bytes).is_empty());
            assert_eq!(decoder.flush_escape(), Some(expected));
            assert_eq!(decoder.feed(b"a"), vec![Event::char('a')]);
        }
    }

    #[test]
    fn an_incomplete_sequence_waits_for_its_final_byte_instead_of_timing_out() {
        let mut decoder = Decoder::new();
        assert!(decoder.feed(b"\x1b[1;5").is_empty());
        assert_eq!(decoder.escape_state(), EscapeState::Incomplete);
        assert_eq!(decoder.flush_escape(), None);
        assert_eq!(decoder.feed(b"C"), vec![modified(
            Key::Right,
            Modifiers::CONTROL
        )],);
    }

    #[test]
    fn eof_drains_partial_sequences_the_way_a_deadline_would() {
        assert_eq!(decode_all(b"\x1b["), vec![modified(
            Key::Char('['),
            Modifiers::ALT
        )],);
        assert_eq!(decode_all(b"\x1bO"), vec![modified(
            Key::Char('O'),
            Modifiers::ALT
        )],);
        assert_eq!(decode_all(b"\x1b"), vec![Event::key(Key::Esc)]);
        assert_eq!(decode_all(b"\xe2\x82"), vec![
            Event::UnknownEscape(vec![0xE2]),
            Event::UnknownEscape(vec![0x82]),
        ]);
        assert_eq!(decode_all(b"\x1b[1;"), vec![
            Event::key(Key::Esc),
            Event::char('['),
            Event::char('1'),
            Event::char(';'),
        ],);
    }

    #[test]
    fn complete_unknown_sequences_are_not_reported_as_escape_keys() {
        for bytes in [b"\x1b[16~".as_slice(), b"\x1bOZ".as_slice()] {
            assert_eq!(decode_all(bytes), vec![Event::UnknownEscape(
                bytes.to_vec()
            )],);
        }
    }

    #[test]
    fn a_csi_ends_at_the_first_byte_outside_the_parameter_range() {
        for chunk_size in 1..=3 {
            assert_eq!(
                decode_in_chunks(b"\x1b[\r", chunk_size),
                vec![
                    Event::UnknownEscape(b"\x1b[".to_vec()),
                    Event::key(Key::Enter),
                ],
                "chunk_size={chunk_size}",
            );
        }
    }

    #[test]
    fn an_unterminated_sequence_is_capped_into_an_unknown_escape() {
        for introducer in [b"\x1b[".as_slice(), b"\x1bO".as_slice()] {
            let mut bytes = introducer.to_vec();
            bytes.extend([b'1'; MAX_SEQUENCE_LEN]);
            let capped = Event::UnknownEscape(bytes[..MAX_SEQUENCE_LEN].to_vec());
            assert_eq!(decode_in_chunks(&bytes, 1), vec![capped.clone()]);
            bytes.extend(b"m1");
            assert_eq!(decode_in_chunks(&bytes, 1), vec![capped, Event::char('1')]);
        }
    }

    #[test]
    fn xterm_function_keys_insert_and_modified_ss3_decode() {
        let none = Modifiers::empty();
        let cases = [
            (b"\x1bOP".as_slice(), Key::Function(1), none),
            (b"\x1bOS".as_slice(), Key::Function(4), none),
            (b"\x1b[11~".as_slice(), Key::Function(1), none),
            (b"\x1b[15~".as_slice(), Key::Function(5), none),
            (b"\x1b[17~".as_slice(), Key::Function(6), none),
            (b"\x1b[21~".as_slice(), Key::Function(10), none),
            (b"\x1b[23~".as_slice(), Key::Function(11), none),
            (b"\x1b[24~".as_slice(), Key::Function(12), none),
            (b"\x1b[2~".as_slice(), Key::Insert, none),
            (b"\x1b[Z".as_slice(), Key::Backtab, none),
            (
                b"\x1b[1;5P".as_slice(),
                Key::Function(1),
                Modifiers::CONTROL,
            ),
            (b"\x1b[15;2~".as_slice(), Key::Function(5), Modifiers::SHIFT),
            (b"\x1bO5A".as_slice(), Key::Up, Modifiers::CONTROL),
            (b"\x1bO2Q".as_slice(), Key::Function(2), Modifiers::SHIFT),
        ];
        for (bytes, key, modifiers) in cases {
            for chunk_size in 1..=bytes.len() {
                assert_eq!(
                    decode_in_chunks(bytes, chunk_size),
                    vec![modified(key.clone(), modifiers)],
                    "bytes={bytes:?}, chunk_size={chunk_size}",
                );
            }
        }
    }

    #[test]
    fn invalid_utf8_is_reported_without_holding_back_the_next_key() {
        for (bytes, expected) in [
            (b"\xe2a".as_slice(), vec![
                Event::UnknownEscape(vec![0xE2]),
                Event::char('a'),
            ]),
            (b"\xf0\x9fa".as_slice(), vec![
                Event::UnknownEscape(vec![0xF0]),
                Event::UnknownEscape(vec![0x9F]),
                Event::char('a'),
            ]),
            (b"\x80a".as_slice(), vec![
                Event::UnknownEscape(vec![0x80]),
                Event::char('a'),
            ]),
        ] {
            for chunk_size in 1..=bytes.len() {
                assert_eq!(
                    decode_in_chunks(bytes, chunk_size),
                    expected,
                    "bytes={bytes:?}, chunk_size={chunk_size}",
                );
            }
        }
    }

    #[test]
    fn double_escape_is_alt_for_a_following_csi_and_two_escapes_otherwise() {
        let bytes = b"\x1b\x1b[A";
        for chunk_size in 1..=bytes.len() {
            assert_eq!(
                decode_in_chunks(bytes, chunk_size),
                vec![modified(Key::Up, Modifiers::ALT)],
                "chunk_size={chunk_size}",
            );
        }
        assert_eq!(decode_all(b"\x1b\x1b"), vec![
            Event::key(Key::Esc),
            Event::key(Key::Esc)
        ]);
        assert_eq!(decode_all(b"\x1b\x1bx"), vec![
            Event::key(Key::Esc),
            modified(Key::Char('x'), Modifiers::ALT)
        ]);

        let mut decoder = Decoder::new();
        assert!(decoder.feed(b"\x1b\x1b").is_empty());
        assert_eq!(decoder.escape_state(), EscapeState::Ambiguous);
        assert_eq!(decoder.flush_escape(), Some(Event::key(Key::Esc)));
        assert_eq!(decoder.escape_state(), EscapeState::Ambiguous);
        assert_eq!(decoder.flush_escape(), Some(Event::key(Key::Esc)));
        assert_eq!(decoder.escape_state(), EscapeState::None);
    }

    #[test]
    fn nul_and_the_last_control_bytes_are_ctrl_keys() {
        let ctrl = |value| modified(Key::Char(value), Modifiers::CONTROL);
        let alt_ctrl = |value| modified(Key::Char(value), Modifiers::ALT | Modifiers::CONTROL);
        for (bytes, expected) in [
            (b"\x00".as_slice(), ctrl(' ')),
            (b"\x1c".as_slice(), ctrl('\\')),
            (b"\x1d".as_slice(), ctrl(']')),
            (b"\x1e".as_slice(), ctrl('^')),
            (b"\x1f".as_slice(), ctrl('_')),
            (b"\x1b\x00".as_slice(), alt_ctrl(' ')),
            (b"\x1b\x1f".as_slice(), alt_ctrl('_')),
        ] {
            assert_eq!(decode_all(bytes), vec![expected], "bytes={bytes:?}");
        }
    }

    #[test]
    fn linux_console_function_keys_decode_without_swallowing_the_next_byte() {
        for (byte, key) in [
            (b'A', Key::Function(1)),
            (b'B', Key::Function(2)),
            (b'C', Key::Function(3)),
            (b'D', Key::Function(4)),
            (b'E', Key::Function(5)),
        ] {
            let bytes = [ESC, b'[', b'[', byte, b'a'];
            for chunk_size in 1..=bytes.len() {
                assert_eq!(
                    decode_in_chunks(&bytes, chunk_size),
                    vec![Event::key(key.clone()), Event::char('a')],
                    "byte={byte}, chunk_size={chunk_size}",
                );
            }
        }
    }
}
