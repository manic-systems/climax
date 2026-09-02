// SPDX-License-Identifier: EUPL-1.2

use bang_core::{Event, Key, KeyEvent, Modifiers};

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

/// terminal byte decoder
#[derive(Debug, Default)]
pub struct Decoder {
    pending: Vec<u8>,
    paste: Vec<u8>,
    in_paste: bool,
}

impl Decoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
            paste: Vec::new(),
            in_paste: false,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();

        loop {
            if self.in_paste {
                if let Some(end) = find_subslice(&self.pending, PASTE_END) {
                    self.paste.extend_from_slice(&self.pending[..end]);
                    self.pending.drain(..end + PASTE_END.len());
                    self.in_paste = false;
                    events.push(Event::Paste(
                        String::from_utf8_lossy(&self.paste).into_owned(),
                    ));
                    self.paste.clear();
                    continue;
                }

                let keep = partial_suffix_len(&self.pending, PASTE_END);
                let paste_len = self.pending.len().saturating_sub(keep);
                self.paste.extend(self.pending.drain(..paste_len));
                break;
            }

            if self.pending.is_empty() {
                break;
            }

            if self.pending.starts_with(PASTE_START) {
                self.pending.drain(..PASTE_START.len());
                self.in_paste = true;
                continue;
            }

            let first = self.pending[0];
            match first {
                ESC => match self.decode_escape() {
                    EscapeResult::Event(event) => events.push(event),
                    EscapeResult::Pending => break,
                    EscapeResult::Unknown(bytes) => events.push(Event::UnknownEscape(bytes)),
                },
                CR | LF => {
                    self.pending.drain(..1);
                    events.push(Event::key(Key::Enter));
                },
                TAB => {
                    self.pending.drain(..1);
                    events.push(Event::key(Key::Tab));
                },
                BACKSPACE | CTRL_H => {
                    self.pending.drain(..1);
                    events.push(Event::key(Key::Backspace));
                },
                CTRL_C => {
                    self.pending.drain(..1);
                    events.push(control_char('c'));
                },
                CTRL_D => {
                    self.pending.drain(..1);
                    events.push(control_char('d'));
                },
                0x01..=0x1A => {
                    self.pending.drain(..1);
                    events.push(control_char(char::from(b'a' + first - 1)));
                },
                0x00..=0x1F => {
                    self.pending.drain(..1);
                },
                _ => match decode_utf8_prefix(&self.pending) {
                    Utf8Result::Char(value, len) => {
                        self.pending.drain(..len);
                        events.push(Event::char(value));
                    },
                    Utf8Result::Pending => break,
                    Utf8Result::Invalid => {
                        self.pending.drain(..1);
                    },
                },
            }
        }

        events
    }

    pub fn flush(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if self.in_paste {
            self.paste.append(&mut self.pending);
            self.in_paste = false;
            events.push(Event::Paste(
                String::from_utf8_lossy(&self.paste).into_owned(),
            ));
            self.paste.clear();
            return events;
        }

        while !self.pending.is_empty() {
            if self.pending[0] == ESC {
                self.pending.drain(..1);
                events.push(Event::key(Key::Esc));
                continue;
            }

            events.extend(self.feed(&[]));
            if !self.pending.is_empty() {
                self.pending.drain(..1);
            }
        }

        events
    }

    /// Whether the buffered input starts with an Escape whose meaning may be
    /// resolved by another byte.
    #[must_use]
    pub fn escape_pending(&self) -> bool {
        self.pending.as_slice() == [ESC]
    }

    /// Resolve only the leading ambiguous Escape, preserving unrelated
    /// partial UTF-8 or CSI input for future reads.
    pub fn flush_escape(&mut self) -> Option<Event> {
        self.escape_pending().then(|| {
            self.pending.drain(..1);
            Event::key(Key::Esc)
        })
    }

    fn decode_escape(&mut self) -> EscapeResult {
        if self.pending.len() == 1 {
            return EscapeResult::Pending;
        }

        match self.pending[1] {
            b'[' => self.decode_csi(),
            b'O' => self.decode_ss3(),
            ESC => self.consume_modified_key(2, Key::Esc, Modifiers::ALT),
            CR | LF => self.consume_modified_key(2, Key::Enter, Modifiers::ALT),
            TAB => self.consume_modified_key(2, Key::Tab, Modifiers::ALT),
            BACKSPACE | CTRL_H => self.consume_modified_key(2, Key::Backspace, Modifiers::ALT),
            0x01..=0x1A => {
                let value = char::from(b'a' + self.pending[1] - 1);
                self.consume_modified_key(2, Key::Char(value), Modifiers::ALT | Modifiers::CONTROL)
            },
            _ => match decode_utf8_prefix(&self.pending[1..]) {
                Utf8Result::Char(value, len) => {
                    self.consume_modified_key(len + 1, Key::Char(value), Modifiers::ALT)
                },
                Utf8Result::Pending => EscapeResult::Pending,
                Utf8Result::Invalid => EscapeResult::Unknown(self.pending.drain(..1).collect()),
            },
        }
    }

    fn decode_csi(&mut self) -> EscapeResult {
        let Some(final_index) = self.pending[2..]
            .iter()
            .position(|byte| (0x40..=0x7E).contains(byte))
            .map(|index| index + 2)
        else {
            return EscapeResult::Pending;
        };
        let final_byte = self.pending[final_index];
        let parameters = &self.pending[2..final_index];
        let Some((key, modifiers)) = csi_key(parameters, final_byte) else {
            return EscapeResult::Unknown(self.pending.drain(..=final_index).collect());
        };
        self.consume_modified_key(final_index + 1, key, modifiers)
    }

    fn decode_ss3(&mut self) -> EscapeResult {
        if self.pending.len() < 3 {
            return EscapeResult::Pending;
        }
        let key = match self.pending[2] {
            b'A' => Key::Up,
            b'B' => Key::Down,
            b'C' => Key::Right,
            b'D' => Key::Left,
            b'H' => Key::Home,
            b'F' => Key::End,
            _ => {
                return EscapeResult::Unknown(self.pending.drain(..3).collect());
            },
        };
        self.consume_key(3, key)
    }

    fn consume_modified_key(&mut self, len: usize, key: Key, modifiers: Modifiers) -> EscapeResult {
        if self.pending.len() < len {
            return EscapeResult::Pending;
        }
        if len == 0 {
            return EscapeResult::Unknown(self.pending.drain(..1).collect());
        }
        self.pending.drain(..len);
        EscapeResult::Event(Event::Key(KeyEvent::with_modifiers(key, modifiers)))
    }

    fn consume_key(&mut self, len: usize, key: Key) -> EscapeResult {
        self.pending.drain(..len);
        EscapeResult::Event(Event::key(key))
    }
}

fn csi_key(parameters: &[u8], final_byte: u8) -> Option<(Key, Modifiers)> {
    let parameters = parse_parameters(parameters)?;
    let modifier = parameters
        .get(1)
        .copied()
        .map_or(Some(Modifiers::empty()), xterm_modifiers)?;
    let key = match final_byte {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        b'Z' if parameters.is_empty() => return Some((Key::Backtab, Modifiers::SHIFT)),
        b'~' => tilde_key(*parameters.first()?)?,
        _ => return None,
    };
    Some((key, modifier))
}

fn parse_parameters(bytes: &[u8]) -> Option<Vec<u16>> {
    if bytes.is_empty() {
        return Some(Vec::new());
    }
    bytes
        .split(|byte| *byte == b';')
        .map(|part| std::str::from_utf8(part).ok()?.parse().ok())
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

const fn tilde_key(parameter: u16) -> Option<Key> {
    Some(match parameter {
        1 | 7 => Key::Home,
        3 => Key::Delete,
        4 | 8 => Key::End,
        5 => Key::PageUp,
        6 => Key::PageDown,
        _ => return None,
    })
}

#[must_use]
pub fn decode_all(bytes: &[u8]) -> Vec<Event> {
    let mut decoder = Decoder::new();
    let mut events = decoder.feed(bytes);
    events.extend(decoder.flush());
    events
}

const fn control_char(value: char) -> Event {
    Event::Key(KeyEvent::with_modifiers(
        Key::Char(value),
        Modifiers::CONTROL,
    ))
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
    if bytes.len() < width {
        return Utf8Result::Pending;
    }
    match std::str::from_utf8(&bytes[..width]) {
        Ok(value) => value
            .chars()
            .next()
            .map_or(Utf8Result::Invalid, |value| Utf8Result::Char(value, width)),
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
        assert!(decoder.escape_pending());
        assert_eq!(decoder.flush_escape(), Some(Event::key(Key::Esc)));
        assert!(!decoder.escape_pending());

        assert!(decoder.feed(&[0xE7]).is_empty());
        assert_eq!(decoder.flush_escape(), None);
        assert_eq!(decoder.feed(&[0x95, 0x8C]), vec![Event::char('界')]);
    }

    #[test]
    fn complete_unknown_sequences_are_not_reported_as_escape_keys() {
        for bytes in [b"\x1b[15~".as_slice(), b"\x1bOP".as_slice()] {
            assert_eq!(
                decode_all(bytes),
                vec![Event::UnknownEscape(bytes.to_vec())],
            );
        }
    }
}
