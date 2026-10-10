// SPDX-License-Identifier: EUPL-1.2

use std::{
    io::{
        self,
        Write,
    },
    sync::{
        Arc,
        Mutex,
    },
};

use screw::{
    Align,
    Color,
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
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn emitted(widget: &impl screw::Widget, columns: usize, rows: usize) -> EmittedScreen {
    let mut renderer = Renderer::new(Vec::new()).width(columns + 1).height(rows);
    renderer.draw(widget).unwrap();
    let mut screen = EmittedScreen::new(columns + 1, rows + 1);
    screen.feed(&renderer.into_inner()).unwrap();
    screen.finish().unwrap();
    screen
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

    let screen = emitted(&line, 8, 1);

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

    let screen = emitted(&table, 8, 3);

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
            .feed(&std::mem::take(&mut *shared.0.lock().unwrap()))
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
    shared.0.lock().unwrap().clear();

    let mut screen = EmittedScreen::new(11, 3);
    screen.feed(b"r2\r\nr3\r\nr4").unwrap();
    renderer.resize_viewport(11, 3);
    renderer.draw(&frame.as_str()).unwrap();
    screen.feed(&shared.0.lock().unwrap()).unwrap();

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

#[test]
fn widths_match_unicode_width_on_whole_clusters() {
    use unicode_width::UnicodeWidthStr as _;

    for text in [
        "plain",
        "世界",
        "e\u{301}x",
        "\u{2764}\u{fe0f}",
        "\u{2764}\u{fe0f}x",
        "👩\u{200d}💻",
        "🇯🇵🇺🇸",
        "1\u{fe0f}\u{20e3}",
        "한국어",
    ] {
        assert_eq!(screw::width(text), text.width(), "{text:?}");
    }
}
