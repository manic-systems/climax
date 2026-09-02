use std::{
    collections::BTreeMap,
    fmt,
    io::Write as _,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::Duration,
};

use screw::{
    AutoRuntime, Looping, RenderCtx, Runtime, Surface, Text, TickInterest, Widget, WidgetRef,
    layout, widget,
};

use crate::{
    Error, Result,
    error::ErrorKind,
    output::SharedWriter,
    terminal::{StatusMode, TerminalCapabilities},
};

#[must_use]
pub fn message(message: impl Into<String>) -> Status {
    static COORDINATOR: OnceLock<StatusCoordinator> = OnceLock::new();
    let coordinator = COORDINATOR.get_or_init(|| {
        let capabilities = TerminalCapabilities::detect_process();
        StatusCoordinator::new(
            SharedWriter::stderr(),
            if capabilities.live_status_available() {
                StatusMode::Live
            } else {
                StatusMode::Silent
            },
        )
    });
    Status::new(message, coordinator.clone())
}

pub struct Status {
    message: String,
    spinner: bool,
    fps: u16,
    final_message: Option<String>,
    coordinator: StatusCoordinator,
}

impl Status {
    #[must_use]
    pub(crate) fn new(message: impl Into<String>, coordinator: StatusCoordinator) -> Self {
        Self {
            message: message.into(),
            spinner: false,
            fps: 15,
            final_message: None,
            coordinator,
        }
    }

    #[must_use]
    pub const fn spinner(mut self) -> Self {
        self.spinner = true;
        self
    }

    #[must_use]
    pub const fn fps(mut self, fps: u16) -> Self {
        self.fps = fps;
        self
    }

    #[must_use]
    pub fn final_message(mut self, message: impl Into<String>) -> Self {
        self.final_message = Some(message.into());
        self
    }

    pub fn start(self) -> StatusRuntime {
        let entry = StatusEntry {
            widget: self.root_widget(),
            plain: self.message,
            final_message: self.final_message,
        };
        let id = self.coordinator.insert(entry, self.fps);
        StatusRuntime {
            coordinator: self.coordinator,
            id: Some(id),
        }
    }

    pub fn finish(self) -> Result<()> {
        self.start().finish()
    }

    pub fn during<T>(self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let status = self.start();
        match operation() {
            Ok(value) => {
                status.finish()?;
                Ok(value)
            },
            Err(error) => match status.finish() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(error.with_cleanup(cleanup)),
            },
        }
    }

    fn root_widget(&self) -> WidgetRef {
        if self.spinner {
            layout()
                .line(vec![
                    widget(Looping::new(["/", "-", "\\", "|"])),
                    widget(Text::new(format!(" {}", self.message))),
                ])
                .into_widget()
        } else {
            widget(Text::new(self.message.clone()))
        }
    }
}

pub struct StatusRuntime {
    coordinator: StatusCoordinator,
    id: Option<u64>,
}

impl StatusRuntime {
    pub fn mark_dirty(&self) -> Result<()> {
        self.coordinator.mark_dirty().map_err(output_error)?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        if let Some(id) = self.id.take() {
            self.coordinator.remove(id)?;
        }
        Ok(())
    }
}

impl Drop for StatusRuntime {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let _result = self.coordinator.remove(id);
        }
    }
}

struct StatusEntry {
    widget: WidgetRef,
    plain: String,
    final_message: Option<String>,
}

#[derive(Clone)]
pub(crate) struct StatusCoordinator {
    inner: Arc<CoordinatorInner>,
}

struct CoordinatorInner {
    entries: Arc<Mutex<BTreeMap<u64, StatusEntry>>>,
    state: Mutex<CoordinatorState>,
    writer: SharedWriter,
}

struct CoordinatorState {
    next_id: u64,
    mode: StatusMode,
    fps: u16,
    runtime: Option<AutoRuntime<SharedWriter>>,
    prompt_active: bool,
}

impl StatusCoordinator {
    pub(crate) fn new(writer: SharedWriter, mode: StatusMode) -> Self {
        Self {
            inner: Arc::new(CoordinatorInner {
                entries: Arc::new(Mutex::new(BTreeMap::new())),
                state: Mutex::new(CoordinatorState {
                    next_id: 0,
                    mode,
                    fps: 15,
                    runtime: None,
                    prompt_active: false,
                }),
                writer,
            }),
        }
    }

    pub(crate) fn set_mode(&self, mode: StatusMode) -> Result<()> {
        let runtime = {
            let mut state = lock(&self.inner.state);
            state.mode = mode;
            state.runtime.take()
        };
        if let Some(runtime) = runtime {
            let _writer = runtime.finish_cleared().map_err(output_error)?;
        }
        self.ensure_runtime();
        Ok(())
    }

    fn insert(&self, entry: StatusEntry, fps: u16) -> u64 {
        let id = {
            let mut state = lock(&self.inner.state);
            let id = state.next_id;
            state.next_id = state.next_id.wrapping_add(1);
            state.fps = state.fps.max(fps);
            id
        };
        lock(&self.inner.entries).insert(id, entry);
        self.ensure_runtime();
        let _result = self.mark_dirty();
        id
    }

    fn remove(&self, id: u64) -> Result<()> {
        let Some(entry) = lock(&self.inner.entries).remove(&id) else {
            return Ok(());
        };
        let mode = lock(&self.inner.state).mode;
        let mut failure = self.mark_dirty().map_err(output_error).err();
        if lock(&self.inner.entries).is_empty() {
            collect_failure(&mut failure, self.stop_runtime());
        }
        match mode {
            StatusMode::Plain => {
                collect_failure(
                    &mut failure,
                    self.write_line(entry.final_message.as_deref().unwrap_or(&entry.plain)),
                );
            },
            StatusMode::Live | StatusMode::Auto => {
                if let Some(message) = entry.final_message {
                    collect_failure(&mut failure, self.write_line(&message));
                }
            },
            StatusMode::Silent => {},
        }
        failure.map_or(Ok(()), Err)
    }

    fn mark_dirty(&self) -> std::io::Result<()> {
        if let Some(runtime) = &lock(&self.inner.state).runtime {
            runtime.mark_dirty()?;
        }
        Ok(())
    }

    fn ensure_runtime(&self) {
        let entries = lock(&self.inner.entries);
        if entries.is_empty() {
            return;
        }
        let mut state = lock(&self.inner.state);
        if state.mode != StatusMode::Live || state.prompt_active || state.runtime.is_some() {
            return;
        }
        let root = widget(StatusStack {
            entries: self.inner.entries.clone(),
        });
        state.runtime = Some(
            Runtime::auto(self.inner.writer.clone(), root, true)
                .fps(state.fps)
                .width(screw::terminal_width_or_default())
                .start(),
        );
        drop(state);
        drop(entries);
    }

    fn stop_runtime(&self) -> Result<()> {
        let runtime = lock(&self.inner.state).runtime.take();
        if let Some(runtime) = runtime {
            let _writer = runtime.finish_cleared().map_err(output_error)?;
        }
        Ok(())
    }

    fn write_line(&self, message: &str) -> Result<()> {
        self.stop_runtime()?;
        let mut writer = self.inner.writer.clone();
        writer.write_all(message.as_bytes()).map_err(output_error)?;
        writer.write_all(b"\n").map_err(output_error)?;
        writer.flush().map_err(output_error)?;
        self.ensure_runtime();
        Ok(())
    }

    #[cfg(feature = "interactive")]
    pub(crate) fn prompt_guard(&self) -> bang::Result<PromptGuard> {
        let runtime = {
            let mut state = lock(&self.inner.state);
            if state.prompt_active {
                return Err(bang::Error::interaction_busy());
            }
            state.prompt_active = true;
            state.runtime.take()
        };
        if let Some(runtime) = runtime
            && let Err(error) = runtime.finish_cleared()
        {
            lock(&self.inner.state).prompt_active = false;
            return Err(bang::Error::terminal(error));
        }
        Ok(PromptGuard {
            coordinator: self.clone(),
        })
    }

    #[cfg(feature = "interactive")]
    pub(crate) fn application_guard(&self) -> Result<PromptGuard> {
        let runtime = {
            let mut state = lock(&self.inner.state);
            if state.prompt_active {
                return Err(Error::from(bang::Error::interaction_busy()));
            }
            state.prompt_active = true;
            state.runtime.take()
        };
        if let Some(runtime) = runtime
            && let Err(error) = runtime.finish_cleared()
        {
            lock(&self.inner.state).prompt_active = false;
            return Err(output_error(error));
        }
        Ok(PromptGuard {
            coordinator: self.clone(),
        })
    }

    #[cfg(feature = "interactive")]
    pub(crate) fn writer(&self) -> SharedWriter {
        self.inner.writer.clone()
    }
}

impl fmt::Debug for StatusCoordinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusCoordinator")
            .field("entries", &lock(&self.inner.entries).len())
            .field("mode", &lock(&self.inner.state).mode)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "interactive")]
pub(crate) struct PromptGuard {
    coordinator: StatusCoordinator,
}

#[cfg(feature = "interactive")]
impl Drop for PromptGuard {
    fn drop(&mut self) {
        lock(&self.coordinator.inner.state).prompt_active = false;
        self.coordinator.ensure_runtime();
    }
}

struct StatusStack {
    entries: Arc<Mutex<BTreeMap<u64, StatusEntry>>>,
}

impl Widget for StatusStack {
    fn render(&self, context: &RenderCtx, output: &mut Surface) {
        for (index, entry) in lock(&self.entries).values().enumerate() {
            if index > 0 {
                output.newline();
            }
            entry.widget.render(context, output);
        }
    }

    fn tick_interest(&self) -> TickInterest {
        combine_tick_interest(
            lock(&self.entries)
                .values()
                .map(|entry| entry.widget.tick_interest()),
        )
    }
}

fn combine_tick_interest(interests: impl IntoIterator<Item = TickInterest>) -> TickInterest {
    let mut every: Option<Duration> = None;
    for interest in interests {
        match interest {
            TickInterest::EveryFrame => return TickInterest::EveryFrame,
            TickInterest::Every(duration) => {
                every = Some(every.map_or(duration, |current| current.min(duration)));
            },
            TickInterest::Never => {},
        }
    }
    every.map_or(TickInterest::Never, TickInterest::Every)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn output_error(error: std::io::Error) -> Error {
    Error::with_source(ErrorKind::Output, error)
}

fn collect_failure(failure: &mut Option<Error>, result: Result<()>) {
    if let Err(error) = result {
        *failure = Some(match failure.take() {
            Some(primary) => primary.with_cleanup(error),
            None => error,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Capture {
        fn text(&self) -> String {
            String::from_utf8(lock(&self.0).clone()).expect("captured status is UTF-8")
        }
    }

    impl io::Write for Capture {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            lock(&self.0).extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn status_stack_composes_multiple_animating_widgets() {
        let entries = Arc::new(Mutex::new(BTreeMap::from([
            (
                1,
                StatusEntry {
                    widget: widget(Text::new("first")),
                    plain: "first".to_owned(),
                    final_message: None,
                },
            ),
            (
                2,
                StatusEntry {
                    widget: widget(Text::new("second")),
                    plain: "second".to_owned(),
                    final_message: None,
                },
            ),
        ])));
        let stack = StatusStack { entries };
        assert_eq!(screw::render_plain(&stack), "first\nsecond");
    }

    #[test]
    fn plain_statuses_emit_once_when_their_handles_finish() {
        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        let first = Status::new("first", coordinator.clone()).spinner().start();
        let second = Status::new("second", coordinator).spinner().start();
        assert!(capture.text().is_empty());
        first.finish().unwrap();
        second.finish().unwrap();
        assert_eq!(capture.text(), "first\nsecond\n");
    }

    #[test]
    fn silent_statuses_never_emit_final_messages() {
        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Silent);

        Status::new("working", coordinator)
            .final_message("finished")
            .finish()
            .unwrap();

        assert!(capture.text().is_empty());
    }

    #[test]
    fn during_preserves_the_operation_error() {
        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        let error = Status::new("working", coordinator)
            .during::<()>(|| Err(crate::Error::message("work failed")))
            .unwrap_err();
        assert_eq!(error.to_string(), "work failed");
        assert_eq!(capture.text(), "working\n");
    }

    #[test]
    fn during_retains_operation_and_cleanup_errors() {
        #[derive(Clone, Copy)]
        struct FailingWriter;

        impl io::Write for FailingWriter {
            fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("status output failed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("status flush failed"))
            }
        }

        let coordinator =
            StatusCoordinator::new(SharedWriter::new(FailingWriter), StatusMode::Plain);
        let error = Status::new("working", coordinator)
            .during::<()>(|| Err(crate::Error::message("work failed")))
            .unwrap_err();

        assert_eq!(error.kind(), ErrorKind::Message);
        assert!(error.to_string().contains("work failed"));
        assert!(error.to_string().contains("status output failed"));
        assert_eq!(error.related_errors().len(), 1);
        assert_eq!(error.related_errors()[0].kind(), ErrorKind::Output);
    }

    #[test]
    fn removal_continues_cleanup_after_a_dirty_notification_fails() {
        #[derive(Clone, Default)]
        struct FailingWriter(Arc<AtomicUsize>);

        impl io::Write for FailingWriter {
            fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(io::Error::other("scripted write failure"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let writer = FailingWriter::default();
        let attempts = writer.0.clone();
        let coordinator = StatusCoordinator::new(SharedWriter::new(writer), StatusMode::Live);
        let status = Status::new("working", coordinator.clone())
            .final_message("finished")
            .start();

        for _ in 0..10_000 {
            if attempts.load(Ordering::SeqCst) > 0 && coordinator.mark_dirty().is_err() {
                break;
            }
            std::thread::yield_now();
        }
        assert!(coordinator.mark_dirty().is_err());

        let error = status.finish().unwrap_err();

        assert!(attempts.load(Ordering::SeqCst) >= 2);
        assert!(
            !error.related_errors().is_empty(),
            "later cleanup failures should be retained"
        );
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn prompt_exclusivity_suspends_and_restores_status_presentation() {
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Live);
        let status = Status::new("working", coordinator.clone())
            .spinner()
            .start();
        assert!(lock(&coordinator.inner.state).runtime.is_some());

        let guard = coordinator.prompt_guard().unwrap();
        assert!(lock(&coordinator.inner.state).runtime.is_none());
        assert!(coordinator.prompt_guard().is_err());

        drop(guard);
        assert!(lock(&coordinator.inner.state).runtime.is_some());
        status.finish().unwrap();
    }
}
