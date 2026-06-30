// SPDX-License-Identifier: EUPL-1.2

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

/// terminal byte decoder
#[derive(Debug, Default)]
pub struct Decoder {
    pending:  Vec<u8>,
    paste:    Vec<u8>,
    in_paste: bool,
}

impl Decoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending:  Vec::new(),
            paste:    Vec::new(),
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

                self.paste.append(&mut self.pending);
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
                ESC => {
                    match self.decode_escape() {
                        EscapeResult::Event(event) => events.push(event),
                        EscapeResult::Pending => break,
                        EscapeResult::Unknown => events.push(Event::key(Key::Esc)),
                    }
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
                _ => {
                    match decode_utf8_prefix(&self.pending) {
                        Utf8Result::Char(value, len) => {
                            self.pending.drain(..len);
                            events.push(Event::char(value));
                        },
                        Utf8Result::Pending => break,
                        Utf8Result::Invalid => {
                            self.pending.drain(..1);
                        },
                    }
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

    fn decode_escape(&mut self) -> EscapeResult {
        if self.pending.len() == 1 {
            return EscapeResult::Pending;
        }

        if self.pending[1] == b'[' {
            self.decode_csi()
        } else {
            self.pending.drain(..1);
            EscapeResult::Unknown
        }
    }

    fn decode_csi(&mut self) -> EscapeResult {
        if self.pending.len() < 3 {
            return EscapeResult::Pending;
        }

        match self.pending[2] {
            b'A' => self.consume_key(3, Key::Up),
            b'B' => self.consume_key(3, Key::Down),
            b'C' => self.consume_key(3, Key::Right),
            b'D' => self.consume_key(3, Key::Left),
            b'H' => self.consume_key(3, Key::Home),
            b'F' => self.consume_key(3, Key::End),
            b'Z' => self.consume_key(3, Key::Backtab),
            b'1' | b'7' => self.decode_tilde(3, Key::Home),
            b'3' => self.decode_tilde(3, Key::Delete),
            b'4' | b'8' => self.decode_tilde(3, Key::End),
            b'5' => self.decode_tilde(3, Key::PageUp),
            b'6' => self.decode_tilde(3, Key::PageDown),
            _ => {
                self.pending.drain(..1);
                EscapeResult::Unknown
            },
        }
    }

    fn decode_tilde(&mut self, marker_len: usize, key: Key) -> EscapeResult {
        let len = marker_len + 1;
        if self.pending.len() < len {
            return EscapeResult::Pending;
        }
        if self.pending[marker_len] != b'~' {
            self.pending.drain(..1);
            return EscapeResult::Unknown;
        }
        self.consume_key(len, key)
    }

    fn consume_key(&mut self, len: usize, key: Key) -> EscapeResult {
        self.pending.drain(..len);
        EscapeResult::Event(Event::key(key))
    }
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
    Unknown,
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
        Ok(value) => {
            value
                .chars()
                .next()
                .map_or(Utf8Result::Invalid, |value| Utf8Result::Char(value, width))
        },
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
