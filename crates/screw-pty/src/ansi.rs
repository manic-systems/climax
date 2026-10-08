// SPDX-License-Identifier: EUPL-1.2

use std::{error::Error, fmt};

use screw::{Position, Style};
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

/// A physical cell in the focused screen model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenCell {
    text: String,
    style: Style,
    width: usize,
    continuation: bool,
    written: bool,
}

impl ScreenCell {
    fn blank(style: Style) -> Self {
        Self {
            text: " ".to_owned(),
            style,
            width: 1,
            continuation: false,
            written: false,
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
#[non_exhaustive]
pub enum ScreenError {
    /// The bytes are not valid UTF-8.
    InvalidUtf8,
    /// The output ended in the middle of an escape or UTF-8 sequence.
    IncompleteSequence,
    /// A sequence that screw does not emit, described by its text.
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
    columns: usize,
    cursor: Position,
    wrap_pending: bool,
}

impl ScreenBuffer {
    fn new(columns: usize, rows: usize) -> Self {
        Self {
            cells: blank_cells(columns, rows, Style::PLAIN),
            columns,
            cursor: Position::default(),
            wrap_pending: false,
        }
    }

    const fn columns(&self) -> usize {
        self.columns
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
            self.cells.push(vec![ScreenCell::blank(style); self.columns]);
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

    fn put_char(&mut self, ch: char, style: Style) -> Result<(), ScreenError> {
        let width = ch.width().unwrap_or(0);
        if let Some(lead) = self.previous_lead() {
            let joined = format!("{}{ch}", self.cells[self.cursor.row][lead].text);
            if width == 0 || joined.graphemes(true).count() == 1 {
                return self.join_cluster(lead, joined);
            }
        }
        if width == 0 {
            return Ok(());
        }
        if width > self.columns() {
            return Err(unsupported(format!(
                "{width}-column character on a {}-column screen",
                self.columns()
            )));
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
            written: true,
        };
        for physical_col in col + 1..col + width {
            self.cells[row][physical_col] = ScreenCell {
                text: String::new(),
                style,
                width: 0,
                continuation: true,
                written: true,
            };
        }

        if col + width == self.columns() {
            self.cursor.col = self.columns().saturating_sub(1);
            self.wrap_pending = true;
        } else {
            self.cursor.col += width;
        }
        Ok(())
    }

    fn previous_lead(&self) -> Option<usize> {
        let row = self.cursor.row;
        if self.cursor.col == 0 && !self.wrap_pending {
            return None;
        }
        let mut col = if self.wrap_pending {
            self.cursor.col
        } else {
            self.cursor.col - 1
        };
        while col > 0 && self.cells[row][col].continuation {
            col -= 1;
        }
        let cell = &self.cells[row][col];
        (!cell.continuation && cell.written).then_some(col)
    }

    fn join_cluster(&mut self, lead: usize, joined: String) -> Result<(), ScreenError> {
        let row = self.cursor.row;
        let old = self.cells[row][lead].width;
        let new = joined.width().max(old);
        if new > old {
            let end = lead + new;
            if end > self.columns() {
                return Err(unsupported(format!(
                    "{new}-column cluster {joined:?} does not fit at column {lead}"
                )));
            }
            let style = self.cells[row][lead].style;
            for physical_col in lead + old..end {
                self.clear_footprint(row, physical_col, style);
                self.cells[row][physical_col] = ScreenCell {
                    text: String::new(),
                    style,
                    width: 0,
                    continuation: true,
                    written: true,
                };
            }
            if self.cursor.col == lead + old {
                if end == self.columns() {
                    self.cursor.col = end - 1;
                    self.wrap_pending = true;
                } else {
                    self.cursor.col = end;
                }
            }
        }
        let cell = &mut self.cells[row][lead];
        cell.text = joined;
        cell.width = new;
        Ok(())
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

    fn erase_line(&mut self, mode: usize, final_byte: u8, style: Style) -> Result<(), ScreenError> {
        let (start, end) = match mode {
            0 => (self.cursor.col, self.columns()),
            2 => (0, self.columns()),
            _ => return Err(unsupported(csi_sequence(false, &[mode], final_byte))),
        };
        for col in start..end {
            self.clear_footprint(self.cursor.row, col, style);
        }
        Ok(())
    }

    fn erase_below(&mut self, mode: usize, final_byte: u8, style: Style) -> Result<(), ScreenError> {
        if mode != 0 {
            return Err(unsupported(csi_sequence(false, &[mode], final_byte)));
        }
        self.erase_line(0, b'K', style)?;
        let columns = self.columns();
        for row in &mut self.cells[self.cursor.row + 1..] {
            *row = vec![ScreenCell::blank(style); columns];
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
    saved_style: Option<Style>,
    parser: ParserState,
    utf8: Vec<u8>,
    error: Option<ScreenError>,
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
            saved_style: None,
            parser: ParserState::Ground,
            utf8: Vec::new(),
            error: None,
        }
    }

    /// Feed one arbitrary output chunk into the model.
    ///
    /// Once a feed reports an error, the model stops interpreting bytes and
    /// every later `feed` or `finish` call returns that same error again.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), ScreenError> {
        if let Some(error) = self.error.clone() {
            return Err(error);
        }
        for &byte in bytes {
            if let Err(error) = self.feed_byte(byte) {
                self.error = Some(error.clone());
                return Err(error);
            }
        }
        Ok(())
    }

    /// Verify that the complete stream did not end partway through a sequence.
    pub fn finish(&self) -> Result<(), ScreenError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.utf8.is_empty() && matches!(self.parser, ParserState::Ground) {
            Ok(())
        } else {
            Err(ScreenError::IncompleteSequence)
        }
    }

    #[must_use]
    pub const fn columns(&self) -> usize {
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
                    Err(unsupported(format!(
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
                        if is_c1_control(ch) {
                            return Err(unsupported(format!("C1 control U+{:04X}", ch as u32)));
                        }
                        let style = self.style;
                        self.active_mut().put_char(ch, style)?;
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
                self.active_mut().put_char(char::from(byte), style)
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
        let parameters = parse_parameters(parameters, private, final_byte)?;
        match (private, final_byte) {
            (false, b'A') => self.move_cursor(-distance(&parameters, final_byte)?, 0),
            (false, b'B') => self.move_cursor(distance(&parameters, final_byte)?, 0),
            (false, b'C') => self.move_cursor(0, distance(&parameters, final_byte)?),
            (false, b'D') => self.move_cursor(0, -distance(&parameters, final_byte)?),
            (false, b'K') => {
                let mode = single_parameter(&parameters, final_byte)?.unwrap_or(0);
                let style = self.style;
                self.active_mut().erase_line(mode, final_byte, style)?;
            },
            (false, b'J') => {
                let mode = single_parameter(&parameters, final_byte)?.unwrap_or(0);
                let style = self.style;
                self.active_mut().erase_below(mode, final_byte, style)?;
            },
            (false, b'm') => self.apply_sgr(&parameters, final_byte)?,
            (true, b'h') => self.set_private_modes(&parameters, true)?,
            (true, b'l') => self.set_private_modes(&parameters, false)?,
            _ => {
                return Err(unsupported(csi_sequence(private, &parameters, final_byte)));
            },
        }
        Ok(())
    }

    fn move_cursor(&mut self, rows: isize, columns: isize) {
        self.active_mut().move_relative(rows, columns);
    }

    fn apply_sgr(&mut self, parameters: &[usize], final_byte: u8) -> Result<(), ScreenError> {
        let parameters = if parameters.is_empty() {
            &[0][..]
        } else {
            parameters
        };
        let mut remaining = parameters.iter().copied();
        while let Some(code) = remaining.next() {
            match code {
                0 => self.style = Style::PLAIN,
                1 => self.style.bold = true,
                2 => self.style.dim = true,
                3 => self.style.italic = true,
                4 => self.style.underline = true,
                7 => self.style.reverse = true,
                9 => self.style.strikethrough = true,
                30..=37 => self.style.fg = Some(color(code - 30)),
                40..=47 => self.style.bg = Some(color(code - 40)),
                90..=97 => self.style.fg = Some(bright_color(code - 90)),
                100..=107 => self.style.bg = Some(bright_color(code - 100)),
                38 | 48 => {
                    let color = extended_color(&mut remaining)
                        .ok_or_else(|| unsupported(csi_sequence(false, parameters, final_byte)))?;
                    if code == 38 {
                        self.style.fg = Some(color);
                    } else {
                        self.style.bg = Some(color);
                    }
                },
                _ => return Err(unsupported(csi_sequence(false, &[code], final_byte))),
            }
        }
        Ok(())
    }

    fn set_private_modes(
        &mut self,
        parameters: &[usize],
        enabled: bool,
    ) -> Result<(), ScreenError> {
        if parameters.is_empty() {
            return Err(unsupported(csi_sequence(
                true,
                parameters,
                if enabled { b'h' } else { b'l' },
            )));
        }
        for &mode in parameters {
            match mode {
                25 => self.cursor_visible = enabled,
                2004 => self.bracketed_paste = enabled,
                // 1049 implies save and restore of cursor and attributes
                1049 if enabled => {
                    if !self.alternate_active {
                        self.saved_style = Some(self.style);
                    }
                    let columns = self.columns();
                    let rows = self.rows();
                    let mut cursor = self.primary.cursor;
                    cursor.row = cursor.row.min(rows.saturating_sub(1));
                    cursor.col = cursor.col.min(columns.saturating_sub(1));
                    self.alternate = ScreenBuffer::new(columns, rows);
                    self.alternate.cursor = cursor;
                    self.alternate_active = true;
                },
                1049 => {
                    self.alternate_active = false;
                    if let Some(style) = self.saved_style.take() {
                        self.style = style;
                    }
                },
                _ => {
                    return Err(unsupported(csi_sequence(
                        true,
                        &[mode],
                        if enabled { b'h' } else { b'l' },
                    )));
                },
            }
        }
        Ok(())
    }
}

fn blank_cells(columns: usize, rows: usize, style: Style) -> Vec<Vec<ScreenCell>> {
    vec![vec![ScreenCell::blank(style); columns]; rows]
}

fn parse_parameters(
    bytes: &[u8],
    private: bool,
    final_byte: u8,
) -> Result<Vec<usize>, ScreenError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    bytes
        .split(|byte| *byte == b';')
        .map(|part| {
            if part.is_empty() {
                Ok(0)
            } else if part.iter().all(u8::is_ascii_digit) {
                std::str::from_utf8(part)
                    .ok()
                    .and_then(|value| value.parse().ok())
                    .ok_or_else(|| unsupported(raw_csi_sequence(private, bytes, final_byte)))
            } else {
                Err(unsupported(raw_csi_sequence(private, bytes, final_byte)))
            }
        })
        .collect()
}

fn csi_sequence(private: bool, parameters: &[usize], final_byte: u8) -> String {
    let parameters = parameters
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(";");
    raw_csi_sequence(private, parameters.as_bytes(), final_byte)
}

/// Build an unsupported-sequence message from the raw parameter bytes, so a
/// parse failure names the actual final byte instead of the parameters'
/// decimal byte values.
fn raw_csi_sequence(private: bool, parameters: &[u8], final_byte: u8) -> String {
    let marker = if private { "?" } else { "" };
    let parameters = String::from_utf8_lossy(parameters);
    format!("CSI {marker}{parameters}{}", char::from(final_byte))
}

fn single_parameter(parameters: &[usize], final_byte: u8) -> Result<Option<usize>, ScreenError> {
    match parameters {
        [] => Ok(None),
        [value] => Ok(Some(*value)),
        _ => Err(unsupported(csi_sequence(false, parameters, final_byte))),
    }
}

fn distance(parameters: &[usize], final_byte: u8) -> Result<isize, ScreenError> {
    let value = single_parameter(parameters, final_byte)?.unwrap_or(1).max(1);
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

const fn bright_color(index: usize) -> screw::Color {
    match index {
        0 => screw::Color::BrightBlack,
        1 => screw::Color::BrightRed,
        2 => screw::Color::BrightGreen,
        3 => screw::Color::BrightYellow,
        4 => screw::Color::BrightBlue,
        5 => screw::Color::BrightMagenta,
        6 => screw::Color::BrightCyan,
        _ => screw::Color::BrightWhite,
    }
}

fn extended_color(parameters: &mut impl Iterator<Item = usize>) -> Option<screw::Color> {
    let mut channel = || u8::try_from(parameters.next()?).ok();
    match channel()? {
        5 => Some(screw::Color::Indexed(channel()?)),
        2 => Some(screw::Color::Rgb(channel()?, channel()?, channel()?)),
        _ => None,
    }
}

const fn unsupported(sequence: String) -> ScreenError {
    ScreenError::UnsupportedSequence(sequence)
}

fn is_c1_control(ch: char) -> bool {
    ('\u{80}'..='\u{9f}').contains(&ch)
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
    fn every_style_attribute_and_colour_round_trips_through_the_renderer() {
        let colours = [
            screw::Color::Black,
            screw::Color::Red,
            screw::Color::Green,
            screw::Color::Yellow,
            screw::Color::Blue,
            screw::Color::Magenta,
            screw::Color::Cyan,
            screw::Color::White,
            screw::Color::BrightBlack,
            screw::Color::BrightRed,
            screw::Color::BrightGreen,
            screw::Color::BrightYellow,
            screw::Color::BrightBlue,
            screw::Color::BrightMagenta,
            screw::Color::BrightCyan,
            screw::Color::BrightWhite,
            screw::Color::Indexed(0),
            screw::Color::Indexed(200),
            screw::Color::Rgb(0, 0, 0),
            screw::Color::Rgb(12, 34, 255),
        ];
        let mut styles = vec![
            Style::new().bold(),
            Style::new().dim(),
            Style::new().italic(),
            Style::new().underline(),
            Style::new().strikethrough(),
            Style::new().reverse(),
            Style::new().bold().italic().underline().strikethrough().dim().reverse(),
        ];
        for colour in colours {
            styles.push(Style::new().fg(colour));
            styles.push(Style::new().bg(colour));
            styles.push(Style::new().fg(colour).bg(screw::Color::Rgb(9, 8, 7)).italic());
        }

        let mut surface = Surface::new();
        for style in &styles {
            surface.write("x", *style);
        }
        let mut renderer = Renderer::new(Vec::new()).width(styles.len() + 1).height(2);
        renderer.draw_surface(surface).unwrap();

        let mut screen = EmittedScreen::new(styles.len() + 1, 3);
        screen.feed(&renderer.into_inner()).unwrap();
        screen.finish().unwrap();
        for (col, style) in styles.iter().enumerate() {
            let cell = screen.cell(0, col).unwrap();
            assert_eq!(cell.text(), "x");
            assert_eq!(cell.style(), *style);
        }
    }

    #[test]
    fn malformed_extended_colours_are_unsupported() {
        for sequence in [
            &b"\x1b[38m"[..],
            b"\x1b[38;5m",
            b"\x1b[38;5;256m",
            b"\x1b[48;2;1;2m",
            b"\x1b[38;3;1m",
        ] {
            let mut screen = EmittedScreen::new(4, 2);
            assert!(matches!(
                screen.feed(sequence),
                Err(ScreenError::UnsupportedSequence(_)),
            ));
        }
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
    fn combining_mark_attaches_to_an_explicitly_written_space() {
        let mut screen = EmittedScreen::new(3, 1);
        screen.feed("a \u{301}".as_bytes()).unwrap();
        assert_eq!(screen.trimmed_line(0).unwrap(), "a \u{301}");
    }

    #[test]
    fn plus_prefixed_and_empty_private_mode_parameters_are_unsupported() {
        let mut plus = EmittedScreen::new(8, 2);
        assert!(matches!(
            plus.feed(b"\x1b[+2B"),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
        let mut set = EmittedScreen::new(8, 2);
        assert!(matches!(
            set.feed(b"\x1b[?h"),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
        let mut reset = EmittedScreen::new(8, 2);
        assert!(matches!(
            reset.feed(b"\x1b[?l"),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
    }

    #[test]
    fn c1_controls_and_unemitted_sgr_codes_are_unsupported() {
        let mut c1 = EmittedScreen::new(8, 2);
        assert!(matches!(
            c1.feed(&[0xc2, 0x9b]),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
        let mut sgr = EmittedScreen::new(8, 2);
        assert!(matches!(
            sgr.feed(b"\x1b[22m"),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
    }

    #[test]
    fn a_wide_character_too_wide_for_the_screen_is_unsupported() {
        let mut screen = EmittedScreen::new(1, 1);
        assert!(matches!(
            screen.feed("界".as_bytes()),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
    }

    #[test]
    fn an_error_is_latched_for_every_later_feed_and_finish() {
        let mut screen = EmittedScreen::new(8, 2);
        let error = screen.feed(b"\x1b[2J").unwrap_err();
        assert_eq!(screen.feed(b"more text").unwrap_err(), error);
        assert_eq!(screen.finish().unwrap_err(), error);
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

    #[test]
    fn scrolling_a_single_row_screen_preserves_its_width() {
        let mut screen = EmittedScreen::new(10, 1);
        screen.feed(b"0123456789\r\n").unwrap();
        assert_eq!(screen.columns(), 10);
        assert_eq!(screen.trimmed_line(0).unwrap(), "");
        screen.feed(b"hi").unwrap();
        assert_eq!(screen.trimmed_line(0).unwrap(), "hi");
    }

    #[test]
    fn extra_cursor_parameters_are_unsupported() {
        let mut screen = EmittedScreen::new(8, 2);
        assert!(matches!(
            screen.feed(b"\x1b[1;99A"),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
        let mut erasure = EmittedScreen::new(8, 2);
        assert!(matches!(
            erasure.feed(b"\x1b[0;1K"),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
    }

    #[test]
    fn alternate_screen_exit_restores_the_entry_rendition() {
        let mut screen = EmittedScreen::new(8, 2);
        screen
            .feed(b"\x1b[31m\x1b[?1049h\x1b[34m\x1b[?1049lX")
            .unwrap();
        let cell = screen.cell(0, 0).unwrap();
        assert_eq!(cell.text(), "X");
        assert_eq!(cell.style().fg, Some(Color::Red));
    }

    #[test]
    fn alternate_screen_entry_keeps_the_primary_cursor_position() {
        let mut screen = EmittedScreen::new(4, 3);
        screen.feed(b"a\r\nb\r\nc").unwrap();
        screen.feed(b"\x1b[?1049h").unwrap();
        screen.feed(b"X").unwrap();
        assert_eq!(screen.cell(2, 1).unwrap().text(), "X");

        screen.feed(b"\x1b[?1049l").unwrap();
        assert_eq!(screen.trimmed_line(0).unwrap(), "a");
        assert_eq!(screen.trimmed_line(1).unwrap(), "b");
        assert_eq!(screen.trimmed_line(2).unwrap(), "c");
        assert_eq!(screen.cursor(), Position { row: 2, col: 1 });
    }

    #[test]
    fn a_variation_selector_widens_the_previous_cell() {
        let mut screen = EmittedScreen::new(6, 2);
        screen.feed("\u{26a0}\u{fe0f}a".as_bytes()).unwrap();
        assert_eq!(screen.cell(0, 0).unwrap().text(), "\u{26a0}\u{fe0f}");
        assert_eq!(screen.cell(0, 0).unwrap().width(), 2);
        assert!(screen.cell(0, 1).unwrap().is_continuation());
        assert_eq!(screen.cell(0, 2).unwrap().text(), "a");
        assert_eq!(screen.cursor(), Position { row: 0, col: 3 });
    }

    #[test]
    fn joiners_and_flags_extend_one_cell() {
        let mut screen = EmittedScreen::new(8, 2);
        screen.feed("👩\u{200d}💻🇯🇵x".as_bytes()).unwrap();
        assert_eq!(screen.cell(0, 0).unwrap().text(), "👩\u{200d}💻");
        assert_eq!(screen.cell(0, 0).unwrap().width(), 2);
        assert_eq!(screen.cell(0, 2).unwrap().text(), "🇯🇵");
        assert_eq!(screen.cell(0, 2).unwrap().width(), 2);
        assert_eq!(screen.cell(0, 4).unwrap().text(), "x");
    }

    #[test]
    fn a_cluster_that_cannot_widen_at_the_last_column_is_unsupported() {
        let mut screen = EmittedScreen::new(2, 2);
        assert!(matches!(
            screen.feed("a\u{26a0}\u{fe0f}".as_bytes()),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
    }

    #[test]
    fn erase_below_clears_from_the_cursor_to_the_end_of_screen() {
        let mut screen = EmittedScreen::new(4, 3);
        screen.feed(b"abcd\r\nefgh\r\nijkl\r\x1b[1A\x1b[2C\x1b[J").unwrap();
        assert_eq!(screen.trimmed_line(0).unwrap(), "abcd");
        assert_eq!(screen.trimmed_line(1).unwrap(), "ef");
        assert_eq!(screen.trimmed_line(2).unwrap(), "");
        assert!(matches!(
            screen.feed(b"\x1b[2J"),
            Err(ScreenError::UnsupportedSequence(_)),
        ));
    }
}
