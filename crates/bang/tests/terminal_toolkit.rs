// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use std::io::{
    self,
    Write as _,
};

use bang::{
    advanced::{
        Event,
        Key,
    },
    terminal::{
        CursorPolicy,
        Decoder,
        RawModeOptions,
        ScreenGuard,
        ScreenKind,
        ScreenOptions,
        TerminalEvents,
        TerminalPoll,
    },
};

#[test]
fn the_terminal_toolkit_is_named_through_bang() {
    let (reader, mut writer) = io::pipe().expect("create pipe");
    writer.write_all(b"a").expect("write byte");
    drop(writer);
    let mut events = TerminalEvents::pollable(reader).expect("pollable pipe");
    let TerminalPoll::Event(event) = events.next_event().expect("first event") else {
        panic!("expected a key event");
    };
    assert_eq!(event, Event::key(Key::Char('a')));
    assert_eq!(events.next_event().expect("end"), TerminalPoll::End);

    let mut decoder = Decoder::new();
    assert_eq!(decoder.feed(b"\r").len(), 1);

    let _options = RawModeOptions::blocking();
}

#[test]
fn a_screen_guard_enters_and_leaves_through_bang() {
    let mut output = Vec::new();
    let options = ScreenOptions::full_screen()
        .cursor(CursorPolicy::Preserve)
        .bracketed_paste(true);
    assert_eq!(ScreenKind::default(), ScreenKind::Inline);
    let guard = ScreenGuard::enter(&mut output, options).expect("enter");
    guard.leave().expect("leave");
    let written = String::from_utf8(output).expect("escape sequences are text");
    assert!(written.contains("\x1b[?1049h"));
    assert!(written.contains("\x1b[?1049l"));
    assert!(!written.contains("\x1b[?25l"));
}
