use std::{error::Error, fmt};

use screw::{Position, Style};
use unicode_width::UnicodeWidthChar as _;

/// A physical cell in the focused screen model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenCell {
    text: String,
    style: Style,
    width: usize,
    continuation: bool,
}

impl ScreenCell {
    fn blank(style: Style) -> Self {
        Self {
            text: " ".to_owned(),
            style,
            width: 1,
            continuation: false,
        }
    }

    /// Text stored in this cell. Wide-cell continuations contain no text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The style active when this cell was written or erased.
    #[must_use]
    pub const fn style(&self) -> Style {
        self.style
    }

    /// Display width of a leading cell, or zero for a continuation.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Whether this physical column continues a wide cell to its left.
    #[must_use]
    pub const fn is_continuation(&self) -> bool {
        self.continuation
    }
}

/// Input rejected by [`EmittedScreen`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScreenError {
    InvalidUtf8,
    IncompleteSequence,
    UnsupportedSequence(String),
}

impl fmt::Display for ScreenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => formatter.write_str("terminal output contains invalid UTF-8"),
            Self::IncompleteSequence => formatter
                .write_str("terminal output ends in an incomplete escape or UTF-8 sequence"),
            Self::UnsupportedSequence(sequence) => {
                write!(formatter, "unsupported terminal sequence {sequence:?}")
            },
        }
    }
}

impl Error for ScreenError {}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ScreenBuffer {
    cells: Vec<Vec<ScreenCell>>,
    cursor: Position,
    wrap_pending: bool,
}

impl ScreenBuffer {
    fn new(columns: usize, rows: usize) -> Self {
        Self {
            cells: blank_cells(columns, rows, Style::PLAIN),
            cursor: Position::default(),
            wrap_pending: false,
        }
    }

    fn columns(&self) -> usize {
        self.cells.first().map_or(1, Vec::len)
    }

    const fn rows(&self) -> usize {
        self.cells.len()
    }

    const fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.wrap_pending = false;
    }

    fn line_feed(&mut self, style: Style) {
        self.wrap_pending = false;
        if self.cursor.row + 1 < self.rows() {
            self.cursor.row += 1;
        } else {
            self.cells.remove(0);
            self.cells
                .push(vec![ScreenCell::blank(style); self.columns()]);
        }
    }

    fn move_relative(&mut self, rows: isize, columns: isize) {
        self.cursor.row = self
            .cursor
            .row
            .saturating_add_signed(rows)
            .min(self.rows().saturating_sub(1));
        self.cursor.col = self
            .cursor
            .col
            .saturating_add_signed(columns)
            .min(self.columns().saturating_sub(1));
        self.wrap_pending = false;
    }

    fn put_char(&mut self, ch: char, style: Style) {
        let width = ch.width().unwrap_or(0);
        if width == 0 {
            self.append_combining(ch);
            return;
        }
        if width > self.columns() {
            return;
        }
        if self.wrap_pending || self.cursor.col + width > self.columns() {
            self.carriage_return();
            self.line_feed(style);
        }

        let row = self.cursor.row;
        let col = self.cursor.col;
        for physical_col in col..col + width {
            self.clear_footprint(row, physical_col, style);
        }
        self.cells[row][col] = ScreenCell {
            text: ch.to_string(),
            style,
            width,
            continuation: false,
        };
        for physical_col in col + 1..col + width {
            self.cells[row][physical_col] = ScreenCell {
                text: String::new(),
                style,
                width: 0,
                continuation: true,
            };
        }

        if col + width == self.columns() {
            self.cursor.col = self.columns().saturating_sub(1);
            self.wrap_pending = true;
        } else {
            self.cursor.col += width;
        }
    }

    fn append_combining(&mut self, ch: char) {
        let row = self.cursor.row;
        if self.cursor.col == 0 && !self.wrap_pending {
            return;
        }
        let mut col = if self.wrap_pending {
            self.cursor.col
        } else {
            self.cursor.col - 1
        };
        while col > 0 && self.cells[row][col].continuation {
            col -= 1;
        }
        if !self.cells[row][col].continuation && self.cells[row][col].text != " " {
            self.cells[row][col].text.push(ch);
        }
    }

    fn clear_footprint(&mut self, row: usize, col: usize, style: Style) {
        let mut lead = col;
        while lead > 0 && self.cells[row][lead].continuation {
            lead -= 1;
        }
        let width = self.cells[row][lead].width.max(1);
        for physical_col in lead..lead.saturating_add(width).min(self.columns()) {
            self.cells[row][physical_col] = ScreenCell::blank(style);
        }
    }

    fn erase_line(&mut self, mode: usize, style: Style) -> Result<(), ScreenError> {
        let (start, end) = match mode {
            0 => (self.cursor.col, self.columns()),
            2 => (0, self.columns()),
            _ => return Err(unsupported(format!("CSI {mode} K"))),
        };
        for col in start..end {
            self.clear_footprint(self.cursor.row, col, style);
        }
        Ok(())
    }

    fn line(&self, row: usize) -> Option<String> {
        self.cells.get(row).map(|cells| {
            cells
                .iter()
                .filter(|cell| !cell.continuation)
                .map(|cell| cell.text.as_str())
                .collect()
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ParserState {
    Ground,
    Escape,
    Csi(Vec<u8>),
}

/// A deliberately small terminal screen for interpreting bytes emitted by
/// Screw and Climax.
///
/// This is an acceptance-test model, not a general terminal emulator. Unknown
/// escape sequences are errors so additions to the renderer's output alphabet
/// are made consciously.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmittedScreen {
    primary: ScreenBuffer,
    alternate: ScreenBuffer,
    alternate_active: bool,
    cursor_visible: bool,
    bracketed_paste: bool,
    style: Style,
    parser: ParserState,
    utf8: Vec<u8>,
}

impl EmittedScreen {
    /// Create a fixed-size screen. Zero dimensions saturate to one cell.
    #[must_use]
    pub fn new(columns: usize, rows: usize) -> Self {
        let columns = columns.max(1);
        let rows = rows.max(1);
        Self {
            primary: ScreenBuffer::new(columns, rows),
            alternate: ScreenBuffer::new(columns, rows),
            alternate_active: false,
            cursor_visible: true,
            bracketed_paste: false,
            style: Style::PLAIN,
            parser: ParserState::Ground,
            utf8: Vec::new(),
        }
    }

    /// Feed one arbitrary output chunk into the model.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), ScreenError> {
        for &byte in bytes {
            self.feed_byte(byte)?;
        }
        Ok(())
    }

    /// Verify that the complete stream did not end partway through a sequence.
    pub const fn finish(&self) -> Result<(), ScreenError> {
        if self.utf8.is_empty() && matches!(self.parser, ParserState::Ground) {
            Ok(())
        } else {
            Err(ScreenError::IncompleteSequence)
        }
    }

    #[must_use]
    pub fn columns(&self) -> usize {
        self.active().columns()
    }

    #[must_use]
    pub const fn rows(&self) -> usize {
        self.active().rows()
    }

    #[must_use]
    pub const fn cursor(&self) -> Position {
        self.active().cursor
    }

    #[must_use]
    pub const fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    #[must_use]
    pub const fn bracketed_paste(&self) -> bool {
        self.bracketed_paste
    }

    #[must_use]
    pub const fn alternate_screen(&self) -> bool {
        self.alternate_active
    }

    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> Option<&ScreenCell> {
        self.active().cells.get(row)?.get(col)
    }

    /// Return a physical row, retaining trailing blank cells.
    #[must_use]
    pub fn line(&self, row: usize) -> Option<String> {
        self.active().line(row)
    }

    /// Return a physical row with terminal padding removed.
    #[must_use]
    pub fn trimmed_line(&self, row: usize) -> Option<String> {
        self.line(row)
            .map(|line| line.trim_end_matches(' ').to_owned())
    }

    const fn active(&self) -> &ScreenBuffer {
        if self.alternate_active {
            &self.alternate
        } else {
            &self.primary
        }
    }

    const fn active_mut(&mut self) -> &mut ScreenBuffer {
        if self.alternate_active {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    fn feed_byte(&mut self, byte: u8) -> Result<(), ScreenError> {
        match &mut self.parser {
            ParserState::Ground => self.feed_ground(byte),
            ParserState::Escape => {
                if byte == b'[' {
                    self.parser = ParserState::Csi(Vec::new());
                    Ok(())
                } else {
                    Err(unsupported(format!("ESC {}", char::from(byte))))
                }
            },
            ParserState::Csi(bytes) => {
                if (0x40..=0x7e).contains(&byte) {
                    let parameters = std::mem::take(bytes);
                    self.parser = ParserState::Ground;
                    self.apply_csi(&parameters, byte)
                } else if (0x20..=0x3f).contains(&byte) {
                    bytes.push(byte);
                    Ok(())
                } else {
                    Err(ScreenError::UnsupportedSequence(format!(
                        "CSI bytes {bytes:?} followed by {byte:#x}"
                    )))
                }
            },
        }
    }

    fn feed_ground(&mut self, byte: u8) -> Result<(), ScreenError> {
        if !self.utf8.is_empty() || byte >= 0x80 {
            self.utf8.push(byte);
            return match std::str::from_utf8(&self.utf8) {
                Ok(text) => {
                    let chars = text.chars().collect::<Vec<_>>();
                    self.utf8.clear();
                    for ch in chars {
                        let style = self.style;
                        self.active_mut().put_char(ch, style);
                    }
                    Ok(())
                },
                Err(error) if error.error_len().is_none() => Ok(()),
                Err(_) => Err(ScreenError::InvalidUtf8),
            };
        }

        match byte {
            0x1b => {
                self.parser = ParserState::Escape;
                Ok(())
            },
            b'\r' => {
                self.active_mut().carriage_return();
                Ok(())
            },
            b'\n' => {
                let style = self.style;
                self.active_mut().line_feed(style);
                Ok(())
            },
            0x20..=0x7e => {
                let style = self.style;
                self.active_mut().put_char(char::from(byte), style);
                Ok(())
            },
            _ => Err(unsupported(format!("control byte {byte:#x}"))),
        }
    }

    fn apply_csi(&mut self, bytes: &[u8], final_byte: u8) -> Result<(), ScreenError> {
        let (private, parameters) = if bytes.first() == Some(&b'?') {
            (true, &bytes[1..])
        } else {
            (false, bytes)
        };
        let parameters = parse_parameters(parameters)?;
        match (private, final_byte) {
            (false, b'A') => self.move_cursor(-parameter(&parameters, 1)?, 0),
            (false, b'B') => self.move_cursor(parameter(&parameters, 1)?, 0),
            (false, b'C') => self.move_cursor(0, parameter(&parameters, 1)?),
            (false, b'D') => self.move_cursor(0, -parameter(&parameters, 1)?),
            (false, b'K') => {
                let mode = parameters.first().copied().unwrap_or(0);
                let style = self.style;
                self.active_mut().erase_line(mode, style)?;
            },
            (false, b'm') => self.apply_sgr(&parameters)?,
            (true, b'h') => self.set_private_modes(&parameters, true)?,
            (true, b'l') => self.set_private_modes(&parameters, false)?,
            _ => {
                return Err(unsupported(format!(
                    "CSI {}{}{}",
                    if private { "?" } else { "" },
                    parameters
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(";"),
                    char::from(final_byte),
                )));
            },
        }
        Ok(())
    }

    fn move_cursor(&mut self, rows: isize, columns: isize) {
        self.active_mut().move_relative(rows, columns);
    }

    fn apply_sgr(&mut self, parameters: &[usize]) -> Result<(), ScreenError> {
        let parameters = if parameters.is_empty() {
            &[0][..]
        } else {
            parameters
        };
        for &code in parameters {
            match code {
                0 => self.style = Style::PLAIN,
                1 => self.style.bold = true,
                2 => self.style.dim = true,
                7 => self.style.reverse = true,
                22 => {
                    self.style.bold = false;
                    self.style.dim = false;
                },
                27 => self.style.reverse = false,
                30..=37 => self.style.fg = Some(color(code - 30)),
                39 => self.style.fg = None,
                40..=47 => self.style.bg = Some(color(code - 40)),
                49 => self.style.bg = None,
                _ => return Err(unsupported(format!("SGR {code}"))),
            }
        }
        Ok(())
    }

    fn set_private_modes(
        &mut self,
        parameters: &[usize],
        enabled: bool,
    ) -> Result<(), ScreenError> {
        for &mode in parameters {
            match mode {
                25 => self.cursor_visible = enabled,
                2004 => self.bracketed_paste = enabled,
                1049 if enabled => {
                    self.alternate = ScreenBuffer::new(self.columns(), self.rows());
                    self.alternate_active = true;
                },
                1049 => self.alternate_active = false,
                _ => return Err(unsupported(format!("DEC private mode {mode}"))),
            }
        }
        Ok(())
    }
}

fn blank_cells(columns: usize, rows: usize, style: Style) -> Vec<Vec<ScreenCell>> {
    vec![vec![ScreenCell::blank(style); columns]; rows]
}

fn parse_parameters(bytes: &[u8]) -> Result<Vec<usize>, ScreenError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    bytes
        .split(|byte| *byte == b';')
        .map(|part| {
            if part.is_empty() {
                Ok(0)
            } else {
                std::str::from_utf8(part)
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .ok_or_else(|| unsupported(format!("CSI parameters {bytes:?}")))
            }
        })
        .collect()
}

fn parameter(parameters: &[usize], default: usize) -> Result<isize, ScreenError> {
    let value = parameters.first().copied().unwrap_or(default).max(1);
    isize::try_from(value).map_err(|_| unsupported(format!("cursor distance {value}")))
}

const fn color(index: usize) -> screw::Color {
    match index {
        0 => screw::Color::Black,
        1 => screw::Color::Red,
        2 => screw::Color::Green,
        3 => screw::Color::Yellow,
        4 => screw::Color::Blue,
        5 => screw::Color::Magenta,
        6 => screw::Color::Cyan,
        _ => screw::Color::White,
    }
}

const fn unsupported(sequence: String) -> ScreenError {
    ScreenError::UnsupportedSequence(sequence)
}

#[cfg(test)]
mod tests {
    use screw::{Color, CursorVisibility, Renderer, Style, Surface};

    use super::*;

    #[test]
    fn utf8_styles_and_wide_cells_survive_every_chunk_size() {
        let output = "plain \x1b[1;31m界e\u{301}\x1b[0m".as_bytes();
        for chunk_size in 1..=output.len() {
            let mut screen = EmittedScreen::new(20, 2);
            for chunk in output.chunks(chunk_size) {
                screen.feed(chunk).unwrap();
            }
            screen.finish().unwrap();
            assert_eq!(screen.trimmed_line(0).unwrap(), "plain 界e\u{301}");
            let wide = screen.cell(0, 6).unwrap();
            assert_eq!(wide.text(), "界");
            assert_eq!(wide.width(), 2);
            assert!(screen.cell(0, 7).unwrap().is_continuation());
            assert_eq!(wide.style().fg, Some(Color::Red));
            assert!(wide.style().bold);
        }
    }

    #[test]
    fn cursor_movement_and_line_erasure_replace_retained_content() {
        let mut screen = EmittedScreen::new(10, 3);
        screen
            .feed(b"first\r\nsecond\x1b[2DXY\x1b[K\x1b[1A\rZ\x1b[K")
            .unwrap();
        assert_eq!(screen.trimmed_line(0).unwrap(), "Z");
        assert_eq!(screen.trimmed_line(1).unwrap(), "secoXY");
    }

    #[test]
    fn complete_emitted_cursor_erase_and_style_alphabet_is_modelled() {
        let mut screen = EmittedScreen::new(8, 3);
        screen
            .feed(b"top\x1b[1B\x1b[2C\x1b[2;7;44mX\x1b[1A\x1b[1D\x1b[2K")
            .unwrap();

        assert_eq!(screen.trimmed_line(0).unwrap(), "");
        let styled = screen.cell(1, 5).unwrap();
        assert_eq!(styled.text(), "X");
        assert!(styled.style().dim);
        assert!(styled.style().reverse);
        assert_eq!(styled.style().bg, Some(Color::Blue));
    }

    #[test]
    fn private_modes_track_alternate_screen_paste_and_cursor_state() {
        let mut screen = EmittedScreen::new(8, 2);
        screen.feed(b"primary\x1b[?1049h").unwrap();
        assert!(screen.alternate_screen());
        assert_eq!(screen.trimmed_line(0).unwrap(), "");
        screen.feed(b"alt\x1b[?25l\x1b[?2004h\x1b[?1049l").unwrap();
        assert!(!screen.alternate_screen());
        assert!(!screen.cursor_visible());
        assert!(screen.bracketed_paste());
        assert_eq!(screen.trimmed_line(0).unwrap(), "primary");
        screen.feed(b"\x1b[?25h\x1b[?2004l").unwrap();
        assert!(screen.cursor_visible());
        assert!(!screen.bracketed_paste());
    }

    #[test]
    fn screw_retained_output_replays_to_the_expected_screen() {
        fn surface(lines: &[&str], cursor: Option<Position>) -> Surface {
            let mut surface = Surface::new();
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    surface.newline();
                }
                surface.write(line, Style::PLAIN);
            }
            if let Some(cursor) = cursor {
                surface.set_cursor(cursor);
            }
            surface
        }

        let mut renderer = Renderer::new(Vec::new())
            .width(9)
            .height(4)
            .cursor_visibility(CursorVisibility::FromSurface);
        renderer
            .draw_surface(surface(&["alpha", "bravo"], None))
            .unwrap();
        renderer
            .draw_surface(surface(&["alpha", "x"], Some(Position { row: 1, col: 1 })))
            .unwrap();
        let output = renderer.into_inner();

        let mut screen = EmittedScreen::new(9, 5);
        for chunk in output.chunks(3) {
            screen.feed(chunk).unwrap();
        }
        screen.finish().unwrap();
        assert_eq!(screen.trimmed_line(0).unwrap(), "alpha");
        assert_eq!(screen.trimmed_line(1).unwrap(), "x");
        assert_eq!(screen.cursor(), Position { row: 1, col: 1 });
        assert!(screen.cursor_visible());
    }

    #[test]
    fn cursorless_full_height_frame_does_not_scroll() {
        let mut surface = Surface::new();
        surface.write("top", Style::PLAIN);
        surface.newline();
        surface.write("middle", Style::PLAIN);
        surface.newline();
        surface.write("bottom", Style::PLAIN);
        let mut renderer = Renderer::new(Vec::new()).width(9).height(3);
        renderer.draw_surface(surface).unwrap();

        let mut screen = EmittedScreen::new(9, 3);
        screen.feed(&renderer.into_inner()).unwrap();
        assert_eq!(screen.trimmed_line(0).unwrap(), "top");
        assert_eq!(screen.trimmed_line(1).unwrap(), "middle");
        assert_eq!(screen.trimmed_line(2).unwrap(), "bottom");
    }

    #[test]
    fn combining_mark_attaches_after_a_final_column_write() {
        let mut screen = EmittedScreen::new(3, 1);
        screen.feed("abe\u{301}".as_bytes()).unwrap();
        assert_eq!(screen.trimmed_line(0).unwrap(), "abe\u{301}");
    }

    #[test]
    fn unsupported_output_is_reported_instead_of_silently_ignored() {
        let mut screen = EmittedScreen::new(8, 2);
        assert!(matches!(
            screen.feed(b"\x1b[2J"),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
        let mut incomplete = EmittedScreen::new(8, 2);
        incomplete.feed(b"\x1b[").unwrap();
        assert_eq!(incomplete.finish(), Err(ScreenError::IncompleteSequence));
    }
}
