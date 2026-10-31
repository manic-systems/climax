// SPDX-License-Identifier: EUPL-1.2

#![expect(
    clippy::non_ascii_literal,
    reason = "terminal glyphs read better as themselves than as escapes"
)]

extern crate alloc;

use alloc::sync::Arc;
use core::{
    error::Error,
    mem,
};
use std::{
    io::{
        self,
        Write,
    },
    sync::Mutex,
};

use screw::{
    Align,
    Color,
    LayoutMode,
    Renderer,
    Span,
    Spans,
    Style,
    Table,
};
use screw_pty::EmittedScreen;

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_poison| io::Error::other("sink lock poisoned"))?
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn emitted(
    widget: &impl screw::Widget,
    columns: usize,
    rows: usize,
) -> Result<EmittedScreen, Box<dyn Error>> {
    let mut renderer = Renderer::new(Vec::new()).width(columns + 1).height(rows);
    renderer.draw(widget)?;
    let mut screen = EmittedScreen::new(columns + 1, rows + 1);
    screen.feed(&renderer.into_inner())?;
    screen.finish()?;
    Ok(screen)
}

#[test]
fn a_styled_spans_line_reaches_the_screen_with_each_span_style() {
    let bold_red = Style::new().fg(Color::Red).bold();
    let underlined = Style::new().bg(Color::Rgb(10, 20, 30)).underline();
    let line = Spans::new([
        Span::new("ab").style(bold_red),
        Span::new("世").style(underlined),
        Span::new("cd"),
    ]);

    let screen = emitted(&line, 8, 1).unwrap();

    assert_eq!(screen.trimmed_line(0).unwrap(), "ab世cd");
    assert_eq!(screen.cell(0, 0).unwrap().style(), bold_red);
    assert_eq!(screen.cell(0, 1).unwrap().style(), bold_red);
    assert_eq!(screen.cell(0, 2).unwrap().text(), "世");
    assert_eq!(screen.cell(0, 2).unwrap().width(), 2);
    assert_eq!(screen.cell(0, 2).unwrap().style(), underlined);
    assert!(screen.cell(0, 3).unwrap().is_continuation());
    assert_eq!(screen.cell(0, 4).unwrap().style(), Style::new());
}

#[test]
fn a_table_with_a_wide_cell_keeps_its_columns_aligned_on_screen() {
    let table = Table::new([["世界", "1"], ["ab", "22"]])
        .header(["name", "n"])
        .aligns([Align::Left, Align::Right]);

    let screen = emitted(&table, 8, 3).unwrap();

    assert_eq!(screen.trimmed_line(0).unwrap(), "name  n");
    assert_eq!(screen.trimmed_line(1).unwrap(), "世界  1");
    assert_eq!(screen.trimmed_line(2).unwrap(), "ab   22");
    assert!(screen.cell(0, 0).unwrap().style().bold);
    assert!(!screen.cell(1, 0).unwrap().style().bold);
}

#[test]
fn clusters_reach_the_screen_as_single_cells_and_survive_patching() {
    let rows = |tail: &str| {
        let mut surface = screw::Surface::new();
        surface.write("\u{26a0}\u{fe0f} a \u{2764}\u{fe0f}", Style::new());
        surface.newline();
        surface.write("👩\u{200d}💻 🇯🇵 e\u{301}", Style::new());
        surface.newline();
        surface.write("世界", Style::new());
        surface.write(tail, Style::new());
        surface
    };
    let shared = Shared::default();
    let mut renderer = Renderer::new(shared.clone()).width(13).height(4);
    let mut screen = EmittedScreen::new(13, 5);
    for tail in ["x", "y", ""] {
        renderer.draw_surface(rows(tail)).unwrap();
        screen
            .feed(&mem::take(&mut *shared.0.lock().expect("sink lock")))
            .unwrap();
        screen.finish().unwrap();

        assert_eq!(screen.cell(0, 0).unwrap().text(), "\u{26a0}\u{fe0f}");
        assert_eq!(screen.cell(0, 0).unwrap().width(), 2);
        assert!(screen.cell(0, 1).unwrap().is_continuation());
        assert_eq!(screen.cell(0, 3).unwrap().text(), "a");
        assert_eq!(screen.cell(0, 5).unwrap().text(), "\u{2764}\u{fe0f}");
        assert_eq!(screen.cell(1, 0).unwrap().text(), "👩\u{200d}💻");
        assert_eq!(screen.cell(1, 0).unwrap().width(), 2);
        assert_eq!(screen.cell(1, 3).unwrap().text(), "🇯🇵");
        assert_eq!(screen.cell(1, 3).unwrap().width(), 2);
        assert_eq!(screen.cell(1, 6).unwrap().text(), "e\u{301}");
        assert_eq!(screen.trimmed_line(2).unwrap(), format!("世界{tail}"));
    }
}

#[test]
fn a_height_only_resize_repaints_the_frame() {
    let rows: Vec<String> = (0..5).map(|row| format!("r{row}")).collect();
    let frame = rows.join("\n");
    let shared = Shared::default();
    let mut renderer = Renderer::new(shared.clone()).width(11).height(5);
    renderer.draw(&frame.as_str()).unwrap();
    shared.0.lock().expect("sink lock").clear();

    let mut screen = EmittedScreen::new(11, 3);
    screen.feed(b"r2\r\nr3\r\nr4").unwrap();
    renderer.resize_viewport(11, 3);
    renderer.draw(&frame.as_str()).unwrap();
    screen.feed(&shared.0.lock().expect("sink lock")).unwrap();

    let shown: Vec<String> = (0..3)
        .map(|row| screen.trimmed_line(row).unwrap())
        .collect();
    assert_eq!(shown, ["r0", "r1", "r2"]);
}

#[test]
fn clusters_split_across_writes_patch_like_a_fresh_draw() {
    let frame = |changed: &str| {
        Spans::new([
            Span::new("\u{2764}"),
            Span::new("\u{fe0f}"),
            Span::new("🇯"),
            Span::new("🇵"),
            Span::new("👩\u{200d}"),
            Span::new("💻"),
            Span::new(changed),
            Span::new("b"),
        ])
    };
    let mut incremental = Renderer::new(Vec::new()).width(13).height(1);
    incremental.draw(&frame("a")).unwrap();
    incremental.draw(&frame("c")).unwrap();
    let mut fresh = Renderer::new(Vec::new()).width(13).height(1);
    fresh.draw(&frame("c")).unwrap();

    let mut patched = EmittedScreen::new(13, 2);
    patched.feed(&incremental.into_inner()).unwrap();
    patched.finish().unwrap();
    let mut drawn = EmittedScreen::new(13, 2);
    drawn.feed(&fresh.into_inner()).unwrap();
    drawn.finish().unwrap();

    assert_eq!(patched.trimmed_line(0), drawn.trimmed_line(0));
    assert_eq!(
        patched.trimmed_line(0).unwrap(),
        "\u{2764}\u{fe0f}🇯🇵👩\u{200d}💻cb"
    );
    assert_eq!(patched.cell(0, 6).unwrap().text(), "c");
}

struct Session {
    renderer: Renderer<Shared>,
    screen:   EmittedScreen,
    shared:   Shared,
}

impl Session {
    fn new(columns: usize, rows: usize, frame_height: usize) -> Self {
        let shared = Shared::default();
        Self {
            renderer: Renderer::new(shared.clone())
                .width(columns)
                .height(frame_height),
            screen: EmittedScreen::new(columns, rows),
            shared,
        }
    }

    fn wrapping(columns: usize, rows: usize) -> Self {
        let shared = Shared::default();
        Self {
            renderer: Renderer::new(shared.clone())
                .width(columns)
                .height(rows)
                .layout_mode(LayoutMode::Wrap),
            screen: EmittedScreen::new(columns, rows),
            shared,
        }
    }

    fn unbounded(columns: usize, rows: usize) -> Self {
        let shared = Shared::default();
        Self {
            renderer: Renderer::new(shared.clone()),
            screen: EmittedScreen::new(columns, rows),
            shared,
        }
    }

    fn draw(&mut self, surface: screw::Surface) -> Result<(), Box<dyn Error>> {
        self.renderer.draw_surface(surface)?;
        self.feed()
    }

    fn feed(&mut self) -> Result<(), Box<dyn Error>> {
        let bytes = mem::take(
            &mut *self
                .shared
                .0
                .lock()
                .map_err(|_poison| "sink lock poisoned")?,
        );
        self.screen.feed(&bytes)?;
        self.screen.finish()?;
        Ok(())
    }

    fn lines(&self) -> Vec<String> {
        (0..self.screen.rows())
            .map(|row| self.screen.trimmed_line(row).unwrap_or_default())
            .collect()
    }
}

fn rows(lines: &[&str], anchor: Option<screw::Position>) -> screw::Surface {
    let mut surface = screw::Surface::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            surface.newline();
        }
        surface.write(line, Style::new());
    }
    if let Some(position) = anchor {
        surface.set_cursor(position);
    }
    surface
}

#[test]
fn a_cursor_below_the_last_row_at_the_screen_bottom_keeps_later_frames_aligned() {
    for mut session in [
        Session::unbounded(10, 5),
        Session::new(10, 5, 5),
        Session::wrapping(10, 5),
    ] {
        session.screen.feed(b"x\r\ny\r\n").unwrap();
        let below = Some(screw::Position { row: 3, col: 0 });
        session.draw(rows(&["a", "b", "c"], None)).unwrap();
        session.draw(rows(&["a", "b", "c"], below)).unwrap();
        session.draw(rows(&["A", "b", "c"], below)).unwrap();
        session.draw(rows(&["A", "B", "c"], below)).unwrap();
        assert_eq!(session.lines(), ["y", "A", "B", "c", ""]);
    }
}

#[test]
fn a_one_column_terminal_keeps_the_characters_it_draws() {
    let mut session = Session::new(1, 3, 3);
    session.draw(rows(&["a", "b"], None)).unwrap();
    assert_eq!(session.lines(), ["a", "b", ""]);
    session.draw(rows(&["c", "b", "d"], None)).unwrap();
    assert_eq!(session.lines(), ["c", "b", "d"]);
    session.draw(rows(&["c", "", "d"], None)).unwrap();
    assert_eq!(session.lines(), ["c", "", "d"]);
}

#[test]
fn screw_and_the_screen_model_agree_on_literal_widths() {
    for (text, columns) in [
        ("plain", 5),
        ("世界", 4),
        ("한국어", 6),
        ("😀", 2),
        ("👩\u{200d}💻", 2),
        ("🇯🇵🇺🇸", 4),
        ("\u{2764}\u{fe0f}", 2),
        ("1\u{fe0f}\u{20e3}", 2),
        ("e\u{301}", 1),
        ("co\u{ad}op", 5),
    ] {
        assert_eq!(screw::width(text), columns, "{text:?}");
        let screen = emitted(&text, 10, 1).unwrap();
        assert_eq!(screen.cursor().col, columns, "{text:?} on screen");
        assert_eq!(screen.trimmed_line(0).unwrap(), text);
    }
}

#[test]
fn a_zwj_sequence_split_across_spans_takes_two_columns_on_screen() {
    let spans = Spans::new([Span::new("👩"), Span::new("\u{200d}💻"), Span::new("!")]);
    let screen = emitted(&spans, 6, 1).unwrap();
    assert_eq!(screen.cell(0, 0).unwrap().text(), "👩\u{200d}💻");
    assert_eq!(screen.cell(0, 0).unwrap().width(), 2);
    assert_eq!(screen.cell(0, 2).unwrap().text(), "!");
}
