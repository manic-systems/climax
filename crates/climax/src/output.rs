// SPDX-License-Identifier: EUPL-1.2

//! output: routing of application results and notices.

use std::{
    fmt,
    io::{
        self,
        Write,
    },
    sync::{
        Arc,
        Mutex,
    },
};

#[cfg(feature = "structured")] use serde::Serialize;

use crate::{
    Error,
    Result,
    error::ErrorKind,
    sync::lock,
};

/// how output is written, chosen with
/// [`Context::set_output_format`](crate::Context::set_output_format)
#[derive(Default, Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// plain text
    #[default]
    Text,
    /// the displayed form as a JSON string, or natural JSON for registered
    /// results
    Json,
}

/// destination-independent output writer
///
/// handles come from [`crate::Context::output`] and
/// [`crate::Context::diagnostic`]
///
/// ```compile_fail
/// climax::output::Output::new(climax::output::Format::Text);
/// ```
#[derive(Clone)]
pub struct Output {
    format:        Format,
    writer:        SharedWriter,
    notices:       SharedWriter,
    notice_format: Format,
    #[cfg(feature = "structured")]
    state:         Arc<Emission>,
    #[cfg(all(feature = "render", feature = "structured"))]
    route:         PresentationRoute,
}

/// where an immediate write goes when a live status may share the terminal
#[cfg(all(feature = "render", feature = "structured"))]
#[derive(Clone)]
pub(crate) enum PresentationRoute {
    /// straight to the destination
    Direct,
    /// clear the live region, write through, let the next frame redraw
    Around(crate::status::StatusCoordinator),
    /// through the coordinator's notice path
    Queued(crate::status::StatusCoordinator),
}

impl Output {
    #[must_use]
    #[cfg_attr(not(feature = "structured"), allow(clippy::missing_const_for_fn))]
    pub(crate) fn new(format: Format) -> Self {
        Self {
            format,
            writer: SharedWriter::stdout(),
            notices: SharedWriter::stderr(),
            notice_format: format,
            #[cfg(feature = "structured")]
            state: Arc::new(Emission::default()),
            #[cfg(all(feature = "render", feature = "structured"))]
            route: PresentationRoute::Direct,
        }
    }

    #[must_use]
    /// format this handle writes
    pub const fn format(&self) -> Format {
        self.format
    }

    #[must_use]
    /// switch the format, which also decides whether notices are suppressed
    pub const fn with_format(mut self, format: Format) -> Self {
        self.format = format;
        self.notice_format = format;
        self
    }

    #[must_use]
    /// write finite results and streams to `writer` instead of stdout
    pub fn with_writer(mut self, writer: impl Write + Send + 'static) -> Self {
        self.writer = SharedWriter::new(writer);
        #[cfg(all(feature = "render", feature = "structured"))]
        {
            self.route = PresentationRoute::Direct;
        }
        self
    }

    pub(crate) fn with_shared_writer(mut self, writer: SharedWriter) -> Self {
        self.writer = writer;
        self
    }

    #[cfg(all(feature = "render", feature = "structured"))]
    pub(crate) fn with_route(mut self, route: PresentationRoute) -> Self {
        self.route = route;
        self
    }

    /// apply `route` only while the handle writes to a process stream
    #[cfg(all(feature = "render", feature = "structured"))]
    pub(crate) fn with_stream_route(mut self, route: PresentationRoute) -> Self {
        if matches!(self.writer, SharedWriter::Stdout | SharedWriter::Stderr) {
            self.route = route;
        }
        self
    }

    #[cfg(not(feature = "render"))]
    pub(crate) fn with_notice_writer(mut self, writer: SharedWriter) -> Self {
        self.notices = writer;
        self
    }

    #[cfg(feature = "render")]
    pub(crate) fn with_transient(mut self, notices: crate::status::TransientNotice) -> Self {
        self.notices = SharedWriter::Transient(notices);
        self
    }

    pub(crate) fn with_notices_from(mut self, other: &Self) -> Self {
        self.notices = other.notices.clone();
        self.notice_format = other.notice_format;
        self
    }

    /// write a message in the configured format
    pub fn write_message(&self, mut writer: impl Write, message: impl fmt::Display) -> Result<()> {
        writer
            .write_all(self.format_message(message).as_bytes())
            .map_err(|error| Error::with_source(ErrorKind::Output, error))?;
        writer
            .flush()
            .map_err(|error| Error::with_source(ErrorKind::Output, error))?;
        Ok(())
    }

    /// write human-facing context that stays out of the application result
    ///
    /// suppressed in structured output modes. while a prompt or terminal
    /// application holds the terminal the line is queued until release and
    /// the call returns at once
    pub fn notice(&self, message: impl fmt::Display) -> Result<()> {
        if self.notice_format == Format::Text {
            write_text(self.notices.clone(), message)?;
        }
        Ok(())
    }

    #[cfg(feature = "structured")]
    /// register the invocation's single finite result
    ///
    /// the projection is buffered until the handler succeeds
    ///
    /// derive with `#[climax::serde(Serialize)]`, which must come before any
    /// other derive sharing serde's helper attributes
    ///
    /// ```
    /// use climax::prelude::*;
    ///
    /// # #[cfg(feature = "derive")]
    /// #[climax::serde(Serialize)]
    /// #[serde(rename_all = "kebab-case")]
    /// struct Report {
    ///     file_count: usize,
    /// }
    /// ```
    ///
    /// without `derive`, write `#[derive(climax::serde::Serialize)]` with
    /// `#[serde(crate = "climax::serde")]`, or rustc reports `E0658` about
    /// `rustc_private`
    ///
    /// ```
    /// use climax::prelude::*;
    ///
    /// #[derive(climax::serde::Serialize)]
    /// #[serde(crate = "climax::serde")]
    /// struct Report {
    ///     files: usize,
    /// }
    /// ```
    pub const fn result<'a, T>(&'a self, value: &'a T) -> ResultBuilder<'a, T, MissingText>
    where
        T: Serialize + ?Sized,
    {
        ResultBuilder {
            output: self,
            value,
            text: MissingText,
            mode: EmissionMode::Finite,
        }
    }

    #[cfg(feature = "structured")]
    /// begin an immediate result stream
    ///
    /// JSON mode writes one value per line. a stream cannot mix with a finite
    /// result in the same invocation, and derives follow [`Self::result`]
    ///
    /// a write on a [`crate::Context::diagnostic`] handle behaves like a
    /// notice. any other write that must wait for a prompt on the calling
    /// thread fails after ten seconds and is dropped, so a thread holding
    /// the terminal must not join a streaming worker
    pub const fn stream<'a, T>(&'a self, value: &'a T) -> ResultBuilder<'a, T, MissingText>
    where
        T: Serialize + ?Sized,
    {
        ResultBuilder {
            output: self,
            value,
            text: MissingText,
            mode: EmissionMode::Stream,
        }
    }

    /// format a message without writing it
    #[must_use]
    pub fn format_message(&self, message: impl fmt::Display) -> String {
        let message = message.to_string();
        match self.format {
            Format::Text => format!("{message}\n"),
            Format::Json => format!("\"{}\"\n", escape_json(&message)),
        }
    }

    #[cfg(feature = "structured")]
    fn register(&self, mode: EmissionMode, bytes: Vec<u8>) -> Result<()> {
        {
            let mut lifecycle = lock(&self.state.lifecycle);
            match (mode, &mut lifecycle.state) {
                (EmissionMode::Finite, state @ EmissionState::Empty) => {
                    *state = EmissionState::Finite(PendingResult {
                        bytes,
                        writer: self.writer.clone(),
                    });
                    return Ok(());
                },
                (EmissionMode::Finite, EmissionState::Finite(_)) => {
                    return Err(output_policy("a finite result is already registered"));
                },
                (EmissionMode::Finite, EmissionState::Streaming) => {
                    return Err(output_policy(
                        "cannot emit a finite result after streaming output",
                    ));
                },
                (EmissionMode::Stream, state @ EmissionState::Empty) => {
                    *state = EmissionState::Streaming;
                },
                (EmissionMode::Stream, EmissionState::Streaming) => {},
                (EmissionMode::Stream, EmissionState::Finite(_)) => {
                    return Err(output_policy(
                        "cannot stream output after registering a finite result",
                    ));
                },
                (EmissionMode::Finite | EmissionMode::Stream, EmissionState::Closed) => {
                    return Err(output_policy("the output lifecycle is already complete"));
                },
            }
            lifecycle.in_flight += 1;
        }
        let _write = InFlight(&self.state);
        self.write_bytes(&bytes)
    }

    #[cfg(feature = "structured")]
    pub(crate) fn commit(&self) -> Result<()> {
        let pending = {
            let state = std::mem::replace(
                &mut lock(&self.state.lifecycle).state,
                EmissionState::Closed,
            );
            drop(
                self.state
                    .drained
                    .wait_while(lock(&self.state.lifecycle), |lifecycle| {
                        lifecycle.in_flight > 0
                    })
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            match state {
                EmissionState::Finite(pending) => Some(pending),
                EmissionState::Empty | EmissionState::Streaming | EmissionState::Closed => None,
            }
        };
        if let Some(mut pending) = pending {
            pending
                .writer
                .write_record(&pending.bytes)
                .map_err(|error| Error::with_source(ErrorKind::Output, error))?;
        }
        Ok(())
    }

    #[cfg(not(feature = "structured"))]
    #[expect(
        clippy::unused_self,
        clippy::unnecessary_wraps,
        reason = "without `structured` nothing is registered to commit"
    )]
    pub(crate) const fn commit(&self) -> Result<()> {
        Ok(())
    }

    #[cfg(feature = "structured")]
    pub(crate) fn discard(&self) {
        lock(&self.state.lifecycle).state = EmissionState::Closed;
    }

    #[cfg(not(feature = "structured"))]
    #[expect(
        clippy::unused_self,
        reason = "without `structured` nothing is registered to discard"
    )]
    pub(crate) const fn discard(&self) {}

    #[cfg(feature = "structured")]
    fn write_bytes(&self, bytes: &[u8]) -> Result<()> {
        #[cfg(feature = "render")]
        match &self.route {
            PresentationRoute::Direct => self.write_bytes_direct(bytes),
            PresentationRoute::Around(coordinator) => {
                coordinator.write_around(self.writer.clone(), bytes.to_vec())
            },
            PresentationRoute::Queued(coordinator) => coordinator.notice(bytes),
        }
        #[cfg(not(feature = "render"))]
        self.write_bytes_direct(bytes)
    }

    #[cfg(feature = "structured")]
    fn write_bytes_direct(&self, bytes: &[u8]) -> Result<()> {
        self.writer
            .clone()
            .write_record(bytes)
            .map_err(|error| Error::with_source(ErrorKind::Output, error))
    }
}

/// emission state plus stream writes in flight, so `commit` can wait for them
#[cfg(feature = "structured")]
#[derive(Default)]
struct Emission {
    lifecycle: Mutex<Lifecycle>,
    drained:   std::sync::Condvar,
}

#[cfg(feature = "structured")]
#[derive(Default)]
struct Lifecycle {
    state:     EmissionState,
    in_flight: usize,
}

#[cfg(feature = "structured")]
struct InFlight<'a>(&'a Emission);

#[cfg(feature = "structured")]
impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        lock(&self.0.lifecycle).in_flight -= 1;
        self.0.drained.notify_all();
    }
}

#[cfg(feature = "structured")]
#[derive(Default)]
enum EmissionState {
    #[default]
    Empty,
    Finite(PendingResult),
    Streaming,
    Closed,
}

#[cfg(feature = "structured")]
struct PendingResult {
    bytes:  Vec<u8>,
    writer: SharedWriter,
}

#[cfg(feature = "structured")]
#[derive(Clone, Copy)]
enum EmissionMode {
    Finite,
    Stream,
}

#[cfg(feature = "structured")]
#[doc(hidden)]
pub struct MissingText;

/// result or stream value being built, supply [`ResultBuilder::text`] then call
/// [`ResultBuilder::emit`]
#[cfg(feature = "structured")]
#[must_use]
pub struct ResultBuilder<'a, T: ?Sized, F> {
    output: &'a Output,
    value:  &'a T,
    text:   F,
    mode:   EmissionMode,
}

#[cfg(feature = "structured")]
impl<'a, T> ResultBuilder<'a, T, MissingText>
where
    T: Serialize + ?Sized,
{
    /// human-readable projection of the value
    pub const fn text<F, D>(self, text: F) -> ResultBuilder<'a, T, F>
    where
        F: FnOnce(&'a T) -> D,
        D: fmt::Display,
    {
        ResultBuilder {
            output: self.output,
            value: self.value,
            text,
            mode: self.mode,
        }
    }
}

#[cfg(feature = "structured")]
impl<'a, T, F> ResultBuilder<'a, T, F>
where
    T: Serialize + ?Sized,
{
    /// encode the projection and register or write it
    pub fn emit<D>(self) -> Result<()>
    where
        F: FnOnce(&'a T) -> D,
        D: fmt::Display,
    {
        let bytes = match self.output.format {
            Format::Text => format_text((self.text)(self.value)).into_bytes(),
            Format::Json => {
                let mut bytes = serde_json::to_vec(self.value)
                    .map_err(|error| Error::with_source(ErrorKind::Output, error))?;
                bytes.push(b'\n');
                bytes
            },
        };
        self.output.register(self.mode, bytes)
    }
}

impl fmt::Debug for Output {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Output")
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub(crate) enum SharedWriter {
    Stdout,
    Stderr,
    #[cfg(feature = "render")]
    Transient(crate::status::TransientNotice),
    Custom(Arc<Mutex<Box<dyn Write + Send>>>),
}

impl SharedWriter {
    pub(crate) fn new(writer: impl Write + Send + 'static) -> Self {
        Self::Custom(Arc::new(Mutex::new(Box::new(writer))))
    }

    pub(crate) const fn stdout() -> Self {
        Self::Stdout
    }

    pub(crate) const fn stderr() -> Self {
        Self::Stderr
    }

    pub(crate) fn write_record(&mut self, bytes: &[u8]) -> io::Result<()> {
        fn whole(mut writer: impl Write, bytes: &[u8]) -> io::Result<()> {
            writer.write_all(bytes)?;
            writer.flush()
        }

        match self {
            Self::Stdout => whole(io::stdout().lock(), bytes),
            Self::Stderr => whole(io::stderr().lock(), bytes),
            #[cfg(feature = "render")]
            Self::Transient(notices) => whole(notices, bytes),
            Self::Custom(inner) => whole(&mut **lock(inner), bytes),
        }
    }
}

impl Write for SharedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout => io::stdout().write(buffer),
            Self::Stderr => io::stderr().write(buffer),
            #[cfg(feature = "render")]
            Self::Transient(notices) => notices.write(buffer),
            Self::Custom(inner) => lock(inner).write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout => io::stdout().flush(),
            Self::Stderr => io::stderr().flush(),
            #[cfg(feature = "render")]
            Self::Transient(notices) => notices.flush(),
            Self::Custom(inner) => lock(inner).flush(),
        }
    }
}

fn escape_json(value: &str) -> String {
    value
        .chars()
        .flat_map(|value| {
            match value {
                '"' => "\\\"".chars().collect::<Vec<_>>(),
                '\\' => "\\\\".chars().collect(),
                '\n' => "\\n".chars().collect(),
                '\r' => "\\r".chars().collect(),
                '\t' => "\\t".chars().collect(),
                value if value.is_control() => format!("\\u{:04x}", value as u32).chars().collect(),
                value => vec![value],
            }
        })
        .collect()
}

fn format_text(message: impl fmt::Display) -> String {
    format!("{message}\n")
}

fn write_text(mut writer: SharedWriter, message: impl fmt::Display) -> Result<()> {
    writer
        .write_record(format_text(message).as_bytes())
        .map_err(|error| Error::with_source(ErrorKind::Output, error))
}

#[cfg(feature = "structured")]
fn output_policy(message: &'static str) -> Error {
    Error::with_source(ErrorKind::Output, io::Error::other(message))
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "structured")]
    use std::sync::atomic::{
        AtomicUsize,
        Ordering,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Capture {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl Write for Capture {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[cfg(feature = "structured")]
    #[derive(Clone, Default)]
    struct FlushCapture {
        bytes:   Arc<Mutex<Vec<u8>>>,
        flushes: Arc<AtomicUsize>,
    }

    #[cfg(feature = "structured")]
    impl Write for FlushCapture {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.bytes.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn text_messages_end_in_a_newline() {
        assert_eq!(Output::new(Format::Text).format_message("hello"), "hello\n");
    }

    #[test]
    fn json_messages_are_escaped_strings() {
        assert_eq!(
            Output::new(Format::Json).format_message("a\n\"b"),
            "\"a\\n\\\"b\"\n"
        );
    }

    #[cfg(feature = "structured")]
    #[test]
    fn records_from_clones_never_interleave_on_a_short_writing_destination() {
        struct OneByte(Capture);

        impl Write for OneByte {
            fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
                std::thread::yield_now();
                self.0.write(&buffer[..buffer.len().min(1)])
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let capture = Capture::default();
        let output = Output::new(Format::Text).with_writer(OneByte(capture.clone()));
        let threads: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|letter| {
                let output = output.clone();
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        output
                            .stream(&letter.repeat(20))
                            .text(|value| value)
                            .emit()
                            .unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let text = capture.text();
        assert_eq!(text.lines().count(), 100);
        for line in text.lines() {
            assert!(line == "a".repeat(20) || line == "b".repeat(20), "{line}");
        }
    }

    #[cfg(feature = "structured")]
    #[test]
    fn configured_writer_receives_durable_output() {
        let capture = Capture::default();
        Output::new(Format::Text)
            .with_writer(capture.clone())
            .write_bytes(b"hello\n")
            .unwrap();
        assert_eq!(capture.text(), "hello\n");
    }

    #[cfg(feature = "structured")]
    #[test]
    fn finite_results_are_deferred_until_commit() {
        let capture = Capture::default();
        let output = Output::new(Format::Text).with_writer(capture.clone());
        let value = 42;
        output
            .result(&value)
            .text(|value| format!("answer: {value}"))
            .emit()
            .unwrap();
        assert_eq!(capture.text(), "");
        output.commit().unwrap();
        assert_eq!(capture.text(), "answer: 42\n");
    }

    #[cfg(feature = "structured")]
    #[test]
    fn committing_a_finite_result_flushes_its_destination() {
        let capture = FlushCapture::default();
        let output = Output::new(Format::Text).with_writer(capture.clone());
        output.result(&42).text(|value| value).emit().unwrap();

        output.commit().unwrap();

        assert_eq!(capture.flushes.load(Ordering::SeqCst), 1);
    }

    #[cfg(feature = "structured")]
    #[test]
    fn finite_results_retain_the_registering_clones_destination() {
        let capture = Capture::default();
        let lifecycle = Output::new(Format::Text);
        lifecycle
            .clone()
            .with_writer(capture.clone())
            .result(&42)
            .text(|value| value)
            .emit()
            .unwrap();
        lifecycle.commit().unwrap();
        assert_eq!(capture.text(), "42\n");
    }

    #[cfg(feature = "structured")]
    #[test]
    fn structured_results_use_json_in_json_mode() {
        let capture = Capture::default();
        let output = Output::new(Format::Json).with_writer(capture.clone());
        let answer = std::collections::BTreeMap::from([("value", 42)]);
        output
            .result(&answer)
            .text(|answer| answer["value"])
            .emit()
            .unwrap();
        output.commit().unwrap();
        assert_eq!(capture.text(), "{\"value\":42}\n");
    }

    #[cfg(feature = "structured")]
    #[test]
    fn cloned_outputs_share_the_finite_result_slot() {
        let output = Output::new(Format::Text);
        output.result(&1).text(|value| value).emit().unwrap();
        let second = output.clone();
        let error = second.result(&2).text(|value| value).emit().unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Output);
        assert_eq!(error.to_string(), "a finite result is already registered");
        output.discard();
    }

    #[cfg(feature = "structured")]
    #[test]
    fn streams_write_json_lines_and_exclude_finite_results() {
        let capture = Capture::default();
        let output = Output::new(Format::Json).with_writer(capture.clone());
        output.stream(&1).text(|value| value).emit().unwrap();
        output.stream(&2).text(|value| value).emit().unwrap();
        let error = output.result(&3).text(|value| value).emit().unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot emit a finite result after streaming output"
        );
        assert_eq!(capture.text(), "1\n2\n");
    }

    #[cfg(feature = "structured")]
    #[test]
    fn completed_output_rejects_late_results_from_clones() {
        let output = Output::new(Format::Text);
        let late = output.clone();
        output.commit().unwrap();
        let error = late.result(&1).text(|value| value).emit().unwrap_err();
        assert_eq!(
            error.to_string(),
            "the output lifecycle is already complete"
        );
    }

    #[test]
    fn notices_are_sideband_and_suppressed_for_json() {
        let text = Capture::default();
        let mut output = Output::new(Format::Text);
        output.notices = SharedWriter::new(text.clone());
        output.notice("heads up").unwrap();
        assert_eq!(text.text(), "heads up\n");

        let json = Capture::default();
        let mut output = Output::new(Format::Json);
        output.notices = SharedWriter::new(json.clone());
        output.notice("heads up").unwrap();
        assert_eq!(json.text(), "");
    }

    #[cfg(feature = "structured")]
    #[test]
    fn commit_waits_for_a_stream_write_already_in_flight() {
        use std::sync::mpsc;

        struct Gated {
            bytes:   Capture,
            entered: mpsc::Sender<()>,
            release: Arc<Mutex<mpsc::Receiver<()>>>,
        }

        impl Write for Gated {
            fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
                self.entered.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
                self.bytes.write(buffer)
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let capture = Capture::default();
        let (entered, entered_rx) = mpsc::channel();
        let (release_tx, release) = mpsc::channel();
        let output = Output::new(Format::Text).with_writer(Gated {
            bytes: capture.clone(),
            entered,
            release: Arc::new(Mutex::new(release)),
        });

        let streaming = output.clone();
        let stream = std::thread::spawn(move || streaming.stream(&1).text(|value| value).emit());
        entered_rx.recv().unwrap();

        let commit = std::thread::spawn(move || {
            output.commit().unwrap();
            capture.text()
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            !commit.is_finished(),
            "commit must wait for the write in flight"
        );

        release_tx.send(()).unwrap();
        stream.join().unwrap().unwrap();
        assert_eq!(commit.join().unwrap(), "1\n");
    }

    #[cfg(all(feature = "render", feature = "structured"))]
    #[test]
    fn queued_route_has_written_when_the_call_returns() {
        let capture = Capture::default();
        let coordinator = crate::status::StatusCoordinator::new(
            SharedWriter::new(capture.clone()),
            crate::terminal::StatusMode::Silent,
        );
        let output = Output::new(Format::Text).with_route(PresentationRoute::Queued(coordinator));
        for index in 0..20 {
            output
                .write_bytes(format!("line {index}\n").as_bytes())
                .unwrap();
            assert!(capture.text().ends_with(&format!("line {index}\n")));
        }
    }

    #[cfg(all(feature = "render", feature = "structured"))]
    #[test]
    fn a_stream_route_never_replaces_a_writer_the_caller_supplied() {
        let coordinator = crate::status::StatusCoordinator::new(
            SharedWriter::new(Capture::default()),
            crate::terminal::StatusMode::Silent,
        );
        let around = || PresentationRoute::Around(coordinator.clone());

        let supplied = Output::new(Format::Text)
            .with_writer(Capture::default())
            .with_stream_route(around());
        assert!(matches!(supplied.route, PresentationRoute::Direct));

        let process = Output::new(Format::Text).with_stream_route(around());
        assert!(matches!(process.route, PresentationRoute::Around(_)));
    }

    #[cfg(all(feature = "interactive", feature = "render", feature = "structured"))]
    #[test]
    fn queued_route_defers_writes_while_the_coordinator_is_leased() {
        let capture = Capture::default();
        let coordinator = crate::status::StatusCoordinator::new(
            SharedWriter::new(capture.clone()),
            crate::terminal::StatusMode::Silent,
        );
        let output =
            Output::new(Format::Text).with_route(PresentationRoute::Queued(coordinator.clone()));

        let guard = coordinator.application_guard().unwrap();
        output.write_bytes(b"heads up\n").unwrap();
        assert!(capture.text().is_empty());

        drop(guard);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while capture.text().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(capture.text(), "heads up\n");
    }
}
