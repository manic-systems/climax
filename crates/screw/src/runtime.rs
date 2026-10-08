// SPDX-License-Identifier: EUPL-1.2

use std::{
    any::Any,
    fmt,
    io::{self, IsTerminal as _, Write},
    panic::{self, AssertUnwindSafe},
    sync::mpsc::{
        self,
        Receiver,
        RecvTimeoutError,
        Sender,
    },
    thread::{
        self,
        JoinHandle,
    },
    time::{
        Duration,
        Instant,
    },
};

use crate::{
    CursorVisibility, LayoutMode, RenderCtx, RenderStats, Renderer, Surface, Theme, TickInterest,
    WidgetRef,
    renderer::{layout_surface, usable_columns},
    terminal::stderr_size,
};

const DEFAULT_FPS: u16 = 15;

pub struct Runtime<W, H = WidgetRef, F = WidgetRef> {
    root: H,
    final_widget: Option<F>,
    renderer:       Renderer<W>,
    frame_interval: Duration,
    last_draw:      Option<Instant>,
    dirty:          bool,
}

impl<W: fmt::Debug, H, F> fmt::Debug for Runtime<W, H, F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Runtime")
            .field("renderer", &self.renderer)
            .field("frame_interval", &self.frame_interval)
            .field("last_draw", &self.last_draw)
            .field("dirty", &self.dirty)
            .field("final_widget", &self.final_widget.is_some())
            .finish_non_exhaustive()
    }
}

impl<W, H> Runtime<W, H, WidgetRef>
where
    W: Write,
    H: crate::Widget,
{
    pub fn new(writer: W, root: H) -> Self {
        Self {
            root,
            final_widget: None,
            renderer: Renderer::new(writer),
            frame_interval: fps_interval(DEFAULT_FPS),
            last_draw: None,
            dirty: true,
        }
    }
}

impl<W, H, F> Runtime<W, H, F>
where
    W: Write,
    H: crate::Widget,
{
    #[must_use]
    pub fn fps(mut self, fps: u16) -> Self {
        self.frame_interval = fps_interval(fps);
        self
    }

    #[must_use]
    pub fn width(mut self, width: usize) -> Self {
        self.renderer = self.renderer.width(width);
        self
    }

    #[must_use]
    pub fn height(mut self, height: usize) -> Self {
        self.renderer = self.renderer.height(height);
        self
    }

    #[must_use]
    pub fn viewport(self, width: usize, height: usize) -> Self {
        self.width(width).height(height)
    }

    #[must_use]
    pub fn layout_mode(mut self, mode: LayoutMode) -> Self {
        self.renderer = self.renderer.layout_mode(mode);
        self
    }

    #[must_use]
    pub fn cursor_visibility(mut self, visibility: CursorVisibility) -> Self {
        self.renderer = self.renderer.cursor_visibility(visibility);
        self
    }

    #[must_use]
    pub fn theme(mut self, theme: Theme) -> Self {
        self.renderer = self.renderer.theme(theme);
        self
    }

    #[must_use]
    pub fn final_widget<G>(self, final_widget: G) -> Runtime<W, H, G>
    where
        G: crate::Widget,
    {
        self.with_final_widget_type(Some(final_widget))
    }

    fn with_final_widget_type<G>(self, final_widget: Option<G>) -> Runtime<W, H, G> {
        Runtime {
            root: self.root,
            final_widget,
            renderer: self.renderer,
            frame_interval: self.frame_interval,
            last_draw: self.last_draw,
            dirty: self.dirty,
        }
    }

    pub const fn resize(&mut self, width: usize) {
        self.renderer.resize(width);
        self.dirty = true;
    }

    pub fn resize_viewport(&mut self, width: usize, height: usize) {
        self.renderer.resize_viewport(width, height);
        self.dirty = true;
    }

    pub const fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn draw_now(&mut self, now: Instant) -> io::Result<RenderStats> {
        let stats = self.renderer.draw(&self.root)?;
        self.dirty = false;
        self.last_draw = Some(now);
        Ok(stats)
    }

    pub fn tick(&mut self, now: Instant) -> io::Result<Option<RenderStats>> {
        if !self.should_draw(now) {
            return Ok(None);
        }
        self.draw_now(now).map(Some)
    }

    pub fn into_inner(self) -> W {
        self.renderer.into_inner()
    }

    /// Move this runtime onto its rendering thread.
    ///
    /// Local references deliberately cannot cross this boundary:
    ///
    /// ```compile_fail
    /// let root = screw::local_widget("local");
    /// let _runtime = screw::Runtime::new(Vec::new(), root).start();
    /// ```
    pub fn start(self) -> LiveRuntime<W>
    where
        W: Send + 'static,
        H: Send + 'static,
        F: crate::Widget + Send + 'static,
    {
        LiveRuntime::start(self)
    }

    fn should_draw(&self, now: Instant) -> bool {
        if self.last_draw.is_none() {
            return true;
        }

        let elapsed = self.last_draw.map_or(Duration::ZERO, |last_draw| {
            now.saturating_duration_since(last_draw)
        });
        let due = elapsed >= self.frame_interval;

        if !due {
            return false;
        }

        self.dirty || wants_frame_tick(self.root.tick_interest(), elapsed, self.frame_interval)
    }
}

impl Runtime<io::Stderr, WidgetRef> {
    pub fn stderr(root: WidgetRef) -> Self {
        let mut runtime = Self::new(io::stderr(), root);
        runtime.renderer = Renderer::stderr();
        runtime
    }

    pub fn stderr_auto(root: WidgetRef) -> AutoRuntimeBuilder<io::Stderr> {
        let (width, height) = stderr_size();
        let mut builder = Self::auto(io::stderr(), root, io::stderr().is_terminal()).width(width);
        builder.height = height;
        builder
    }
}

impl<W, H> Runtime<W, H, WidgetRef>
where
    W: Write + Send + 'static,
    H: crate::Widget + Send + 'static,
{
    pub fn auto(writer: W, root: H, interactive: bool) -> AutoRuntimeBuilder<W, H> {
        AutoRuntimeBuilder::new(writer, root, interactive)
    }
}

enum RuntimeCommand {
    Dirty,
    Resize(usize),
    ResizeViewport(usize, usize),
    Finish(ThreadFinishMode),
}

enum ThreadFinishMode {
    Current,
    With(Box<dyn crate::Widget + Send>),
    Clear,
}

pub struct LiveRuntime<W> {
    handle: RuntimeHandle,
    thread: Option<JoinHandle<Result<W, (W, io::Error)>>>,
}

impl<W> fmt::Debug for LiveRuntime<W> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LiveRuntime")
            .field("handle", &self.handle)
            .field("running", &self.thread.is_some())
            .finish()
    }
}

#[derive(Debug)]
pub struct RuntimeHandle {
    tx: Sender<RuntimeCommand>,
}

pub struct AutoRuntimeBuilder<W, H = WidgetRef, F = WidgetRef> {
    writer:       W,
    root: H,
    interactive:  bool,
    fps:          u16,
    width:        Option<usize>,
    height: Option<usize>,
    layout_mode: LayoutMode,
    cursor_visibility: CursorVisibility,
    theme:        Theme,
    final_widget: Option<F>,
}

impl<W, H, F> fmt::Debug for AutoRuntimeBuilder<W, H, F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AutoRuntimeBuilder")
            .field("interactive", &self.interactive)
            .field("fps", &self.fps)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("layout_mode", &self.layout_mode)
            .field("cursor_visibility", &self.cursor_visibility)
            .field("theme", &self.theme)
            .finish_non_exhaustive()
    }
}

impl<W, H> AutoRuntimeBuilder<W, H, WidgetRef>
where
    W: Write + Send + 'static,
    H: crate::Widget + Send + 'static,
{
    fn new(writer: W, root: H, interactive: bool) -> Self {
        Self {
            writer,
            root,
            interactive,
            fps: DEFAULT_FPS,
            width: None,
            height: None,
            layout_mode: LayoutMode::Clip,
            cursor_visibility: CursorVisibility::Preserve,
            theme: Theme::default(),
            final_widget: None,
        }
    }
}

impl<W, H, F> AutoRuntimeBuilder<W, H, F>
where
    W: Write + Send + 'static,
    H: crate::Widget + Send + 'static,
    F: crate::Widget + Send + 'static,
{
    #[must_use]
    pub const fn fps(mut self, fps: u16) -> Self {
        self.fps = fps;
        self
    }

    #[must_use]
    pub const fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    #[must_use]
    pub const fn height(mut self, height: usize) -> Self {
        self.height = Some(height);
        self
    }

    #[must_use]
    pub const fn viewport(mut self, width: usize, height: usize) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    #[must_use]
    pub const fn layout_mode(mut self, mode: LayoutMode) -> Self {
        self.layout_mode = mode;
        self
    }

    #[must_use]
    pub const fn cursor_visibility(mut self, visibility: CursorVisibility) -> Self {
        self.cursor_visibility = visibility;
        self
    }

    #[must_use]
    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    #[must_use]
    pub fn final_widget<G>(self, final_widget: G) -> AutoRuntimeBuilder<W, H, G>
    where
        G: crate::Widget + Send + 'static,
    {
        AutoRuntimeBuilder {
            writer: self.writer,
            root: self.root,
            interactive: self.interactive,
            fps: self.fps,
            width: self.width,
            height: self.height,
            layout_mode: self.layout_mode,
            cursor_visibility: self.cursor_visibility,
            theme: self.theme,
            final_widget: Some(final_widget),
        }
    }

    pub fn start(self) -> AutoRuntime<W, H, F> {
        if self.interactive {
            let mut runtime = Runtime::new(self.writer, self.root).fps(self.fps);
            if let Some(width) = self.width {
                runtime = runtime.width(width);
            }
            if let Some(height) = self.height {
                runtime = runtime.height(height);
            }
            runtime = runtime.layout_mode(self.layout_mode);
            runtime = runtime.cursor_visibility(self.cursor_visibility);
            runtime = runtime.theme(self.theme);
            let runtime = runtime.with_final_widget_type(self.final_widget);
            AutoRuntime::Live(runtime.start())
        } else {
            AutoRuntime::Plain(PlainRuntime {
                writer:       self.writer,
                root:         self.root,
                width:        self.width,
                height: self.height,
                layout_mode:  self.layout_mode,
                theme:        self.theme,
                final_widget: self.final_widget,
            })
        }
    }
}

pub enum AutoRuntime<W, H = WidgetRef, F = WidgetRef> {
    Live(LiveRuntime<W>),
    Plain(PlainRuntime<W, H, F>),
}

impl<W, H, F> fmt::Debug for AutoRuntime<W, H, F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Live(runtime) => formatter.debug_tuple("Live").field(runtime).finish(),
            Self::Plain(runtime) => formatter.debug_tuple("Plain").field(runtime).finish(),
        }
    }
}

impl<W, H, F> AutoRuntime<W, H, F>
where
    W: Write + Send + 'static,
    H: crate::Widget + Send + 'static,
    F: crate::Widget + Send + 'static,
{
    pub fn mark_dirty(&self) -> io::Result<()> {
        match self {
            Self::Live(runtime) => runtime.mark_dirty(),
            Self::Plain(_) => Ok(()),
        }
    }

    pub fn resize(&mut self, width: usize) -> io::Result<()> {
        match self {
            Self::Live(runtime) => runtime.resize(width),
            Self::Plain(runtime) => runtime.resize(width),
        }
    }

    pub fn resize_viewport(&mut self, width: usize, height: usize) -> io::Result<()> {
        match self {
            Self::Live(runtime) => runtime.resize_viewport(width, height),
            Self::Plain(runtime) => runtime.resize_viewport(width, height),
        }
    }

    pub fn finish(self) -> io::Result<W> {
        match self {
            Self::Live(runtime) => runtime.finish(),
            Self::Plain(runtime) => runtime.finish(),
        }
    }

    pub fn finish_with<G>(self, final_widget: G) -> io::Result<W>
    where
        G: crate::Widget + Send + 'static,
    {
        match self {
            Self::Live(runtime) => runtime.finish_with(final_widget),
            Self::Plain(runtime) => runtime.finish_with(&final_widget),
        }
    }

    pub fn finish_cleared(self) -> io::Result<W> {
        match self {
            Self::Live(runtime) => runtime.finish_cleared(),
            Self::Plain(runtime) => runtime.finish_cleared(),
        }
    }
}

pub struct PlainRuntime<W, H = WidgetRef, F = WidgetRef> {
    writer:       W,
    root: H,
    width: Option<usize>,
    height: Option<usize>,
    layout_mode:  LayoutMode,
    theme:        Theme,
    final_widget: Option<F>,
}

impl<W, H, F> fmt::Debug for PlainRuntime<W, H, F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlainRuntime")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("layout_mode", &self.layout_mode)
            .field("theme", &self.theme)
            .field("final_widget", &self.final_widget.is_some())
            .finish_non_exhaustive()
    }
}

impl<W, H, F> PlainRuntime<W, H, F>
where
    W: Write,
    H: crate::Widget,
    F: crate::Widget,
{
    pub const fn resize(&mut self, width: usize) -> io::Result<()> {
        self.width = Some(width);
        Ok(())
    }

    pub const fn resize_viewport(&mut self, width: usize, height: usize) -> io::Result<()> {
        self.width = Some(width);
        self.height = Some(height);
        Ok(())
    }

    pub fn finish(self) -> io::Result<W> {
        let Self {
            writer,
            root,
            width,
            height,
            layout_mode,
            theme,
            final_widget,
        } = self;
        if let Some(final_widget) = final_widget {
            write_plain_frame(writer, &final_widget, width, height, layout_mode, theme)
        } else {
            write_plain_frame(writer, &root, width, height, layout_mode, theme)
        }
    }

    pub fn finish_with<G>(self, final_widget: &G) -> io::Result<W>
    where
        G: crate::Widget,
    {
        write_plain_frame(
            self.writer,
            final_widget,
            self.width,
            self.height,
            self.layout_mode,
            self.theme,
        )
    }

    pub fn finish_cleared(mut self) -> io::Result<W> {
        self.writer.flush()?;
        Ok(self.writer)
    }
}

fn write_plain_frame<W, G>(
    mut writer: W,
    root: &G,
    width: Option<usize>,
    height: Option<usize>,
    layout_mode: LayoutMode,
    theme: Theme,
) -> io::Result<W>
where
    W: Write,
    G: crate::Widget,
{
    let mut surface = Surface::new();
    let columns = width.map(usable_columns);
    root.render(
        &RenderCtx::new()
            .with_constraints(columns, height)
            .with_layout_mode(layout_mode)
            .with_theme(theme),
        &mut surface,
    );
    surface = layout_surface(surface, columns, layout_mode);
    if let Some(height) = height {
        surface.fit_height(height);
    }
    writer.write_all(surface.plain_text().as_bytes())?;
    writer.flush()?;
    Ok(writer)
}

impl<W> LiveRuntime<W>
where
    W: Write + Send + 'static,
{
    fn start<H, F>(mut runtime: Runtime<W, H, F>) -> Self
    where
        H: crate::Widget + Send + 'static,
        F: crate::Widget + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                let finish_mode = drive(&mut runtime, &rx)?;
                finish_frame(&mut runtime, finish_mode)
            }));
            let restored = runtime.renderer.restore_cursor();
            let failure = match result {
                Ok(Ok(())) => restored.err(),
                Ok(Err(error)) => Some(error),
                Err(payload) => Some(panic_error(&*payload)),
            };
            let writer = runtime.into_inner();
            match failure {
                None => Ok(writer),
                Some(error) => Err((writer, error)),
            }
        });

        Self {
            handle: RuntimeHandle { tx },
            thread: Some(thread),
        }
    }

    pub fn handle(&self) -> RuntimeHandle {
        self.handle.clone()
    }

    pub fn mark_dirty(&self) -> io::Result<()> {
        self.handle.mark_dirty()
    }

    pub fn resize(&self, width: usize) -> io::Result<()> {
        self.handle.resize(width)
    }

    pub fn resize_viewport(&self, width: usize, height: usize) -> io::Result<()> {
        self.handle.resize_viewport(width, height)
    }

    pub fn finish(self) -> io::Result<W> {
        self.finish_via(ThreadFinishMode::Current)
    }

    pub fn finish_recovering(self) -> Result<W, (Option<W>, io::Error)> {
        self.finish_via_recovering(ThreadFinishMode::Current)
    }

    pub fn finish_with<G>(self, final_widget: G) -> io::Result<W>
    where
        G: crate::Widget + Send + 'static,
    {
        self.finish_via(ThreadFinishMode::With(Box::new(final_widget)))
    }

    pub fn finish_cleared(self) -> io::Result<W> {
        self.finish_via(ThreadFinishMode::Clear)
    }

    fn finish_via(self, mode: ThreadFinishMode) -> io::Result<W> {
        self.finish_via_recovering(mode).map_err(|(_, error)| error)
    }

    fn finish_via_recovering(
        mut self,
        mode: ThreadFinishMode,
    ) -> Result<W, (Option<W>, io::Error)> {
        let sent = self.handle.send(RuntimeCommand::Finish(mode));
        match (sent, self.join()) {
            (Ok(()), joined) => joined,
            (Err(_), Err(joined)) => Err(joined),
            (Err(sent), Ok(writer)) => Err((Some(writer), sent)),
        }
    }

    fn join(&mut self) -> Result<W, (Option<W>, io::Error)> {
        let thread = self
            .thread
            .take()
            .expect("live runtime thread is joined at most once");
        match thread.join() {
            Ok(Ok(writer)) => Ok(writer),
            Ok(Err((writer, error))) => Err((Some(writer), error)),
            Err(payload) => Err((None, panic_error(&*payload))),
        }
    }
}

impl Clone for RuntimeHandle {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl RuntimeHandle {
    pub fn mark_dirty(&self) -> io::Result<()> {
        self.send(RuntimeCommand::Dirty)
    }

    pub fn resize(&self, width: usize) -> io::Result<()> {
        self.send(RuntimeCommand::Resize(width))
    }

    pub fn resize_viewport(&self, width: usize, height: usize) -> io::Result<()> {
        self.send(RuntimeCommand::ResizeViewport(width, height))
    }

    fn send(&self, command: RuntimeCommand) -> io::Result<()> {
        self.tx.send(command).map_err(|err| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("runtime thread stopped before command was delivered: {err}"),
            )
        })
    }
}

impl<W> Drop for LiveRuntime<W> {
    fn drop(&mut self) {
        if self.thread.is_some() {
            let finish_mode = if thread::panicking() {
                ThreadFinishMode::Clear
            } else {
                ThreadFinishMode::Current
            };
            let _ = self.handle.send(RuntimeCommand::Finish(finish_mode));
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

fn drive<W, H, F>(
    runtime: &mut Runtime<W, H, F>,
    commands: &Receiver<RuntimeCommand>,
) -> io::Result<ThreadFinishMode>
where
    W: Write,
    H: crate::Widget,
{
    runtime.draw_now(Instant::now())?;
    loop {
        match commands.recv_timeout(runtime.frame_interval) {
            Ok(command) => {
                if let Some(finish_mode) = apply_command(runtime, command) {
                    return Ok(finish_mode);
                }
            },
            Err(RecvTimeoutError::Disconnected) => return Ok(ThreadFinishMode::Current),
            Err(RecvTimeoutError::Timeout) => {
                runtime.tick(Instant::now())?;
            },
        }

        while let Ok(command) = commands.try_recv() {
            if let Some(finish_mode) = apply_command(runtime, command) {
                return Ok(finish_mode);
            }
        }
        runtime.tick(Instant::now())?;
    }
}

fn apply_command<W, H, F>(
    runtime: &mut Runtime<W, H, F>,
    command: RuntimeCommand,
) -> Option<ThreadFinishMode>
where
    W: Write,
    H: crate::Widget,
{
    match command {
        RuntimeCommand::Dirty => runtime.mark_dirty(),
        RuntimeCommand::Resize(width) => runtime.resize(width),
        RuntimeCommand::ResizeViewport(width, height) => runtime.resize_viewport(width, height),
        RuntimeCommand::Finish(finish_mode) => return Some(finish_mode),
    }
    None
}

fn finish_frame<W, H, F>(
    runtime: &mut Runtime<W, H, F>,
    finish_mode: ThreadFinishMode,
) -> io::Result<()>
where
    W: Write,
    H: crate::Widget,
    F: crate::Widget,
{
    match finish_mode {
        ThreadFinishMode::Current => {
            if let Some(final_widget) = runtime.final_widget.take() {
                runtime.renderer.draw(&final_widget)?;
            } else {
                runtime.draw_now(Instant::now())?;
            }
        },
        ThreadFinishMode::With(final_widget) => {
            runtime.renderer.draw(&final_widget)?;
        },
        ThreadFinishMode::Clear => {
            runtime.renderer.clear()?;
        },
    }
    Ok(())
}

fn panic_error(payload: &(dyn Any + Send)) -> io::Error {
    io::Error::other(format!("runtime thread panicked: {}", panic_message(payload)))
}

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string payload")
}

fn wants_frame_tick(interest: TickInterest, elapsed: Duration, frame_interval: Duration) -> bool {
    match interest {
        TickInterest::Never => false,
        TickInterest::EveryFrame => elapsed >= frame_interval,
        TickInterest::Every(interval) => elapsed >= frame_interval && elapsed >= interval,
    }
}

fn fps_interval(fps: u16) -> Duration {
    let fps = u64::from(fps.max(1));
    Duration::from_nanos(1_000_000_000 / fps)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::{Arc, Mutex},
    };

    use crate::{Position, Stack, Style, Widget, local_widget};

    use super::*;

    type RecordedFrame = (u64, Option<usize>, Option<usize>);
    type RecordedFrames = Arc<Mutex<Vec<RecordedFrame>>>;

    struct CursorWidget;

    impl Widget for CursorWidget {
        fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
            out.write("cursor", Style::PLAIN);
            out.set_cursor(Position { row: 0, col: 2 });
        }
    }

    #[test]
    fn synchronous_runtime_threads_viewport_and_cursor_policy_to_renderer() {
        let root: WidgetRef = Arc::new(CursorWidget);
        let mut runtime = Runtime::new(Vec::new(), root)
            .viewport(10, 3)
            .cursor_visibility(CursorVisibility::FromSurface);
        runtime.draw_now(Instant::now()).unwrap();
        runtime.resize_viewport(6, 2);
        runtime.draw_now(Instant::now()).unwrap();
        let output = runtime.into_inner();
        assert!(output.windows(6).any(|part| part == b"\x1b[?25h"));
    }

    #[derive(Clone, Default)]
    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct PanicsOnSecondRender(std::sync::atomic::AtomicUsize);

    impl Widget for PanicsOnSecondRender {
        fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
            assert!(
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0,
                "widget exploded"
            );
            out.write("x", Style::PLAIN);
        }

        fn tick_interest(&self) -> TickInterest {
            TickInterest::Never
        }
    }

    #[test]
    fn a_widget_panic_on_the_thread_restores_the_cursor_and_is_reported() {
        let buffer = SharedBuffer::default();
        let widget = PanicsOnSecondRender(std::sync::atomic::AtomicUsize::default());
        let live = Runtime::new(buffer.clone(), widget)
            .cursor_visibility(CursorVisibility::FromSurface)
            .start();
        let Err(error) = live.finish() else {
            panic!("the widget panic was not reported");
        };
        assert!(error.to_string().contains("widget exploded"), "{error}");
        assert!(buffer.0.lock().unwrap().ends_with(b"\x1b[?25h"));
    }

    #[derive(Default)]
    struct LineBuffered {
        pending: Vec<u8>,
        flushed: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for LineBuffered {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.pending.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushed.lock().unwrap().append(&mut self.pending);
            Ok(())
        }
    }

    #[test]
    fn the_cursor_restore_is_flushed_when_the_thread_ends() {
        let flushed = Arc::new(Mutex::new(Vec::new()));
        let writer = LineBuffered {
            pending: Vec::new(),
            flushed: Arc::clone(&flushed),
        };
        let live = Runtime::new(writer, "content")
            .cursor_visibility(CursorVisibility::FromSurface)
            .start();
        drop(live.finish().unwrap());
        assert!(flushed.lock().unwrap().ends_with(b"\x1b[?25h"));
    }

    #[test]
    fn finish_recovering_returns_the_writer_after_a_widget_panic() {
        let buffer = SharedBuffer::default();
        let widget = PanicsOnSecondRender(std::sync::atomic::AtomicUsize::default());
        let live = Runtime::new(buffer, widget).start();
        let Err((writer, error)) = live.finish_recovering() else {
            panic!("the widget panic was not reported");
        };
        assert!(error.to_string().contains("widget exploded"), "{error}");
        assert!(writer.is_some());
    }

    #[test]
    fn finish_restores_a_hidden_cursor_left_by_a_final_frame_without_an_anchor() {
        let live = Runtime::new(Vec::new(), "content")
            .cursor_visibility(CursorVisibility::FromSurface)
            .start();
        let output = live.finish().unwrap();
        assert!(output.ends_with(b"\x1b[?25h"));
    }

    #[test]
    fn finish_with_restores_a_hidden_cursor_left_by_a_final_widget_without_an_anchor() {
        let root: WidgetRef = Arc::new(CursorWidget);
        let live = Runtime::new(Vec::new(), root)
            .cursor_visibility(CursorVisibility::FromSurface)
            .start();
        let output = live.finish_with("final").unwrap();
        assert!(output.ends_with(b"\x1b[?25h"));
    }

    #[test]
    fn dropping_a_live_runtime_restores_a_hidden_cursor() {
        let buffer = SharedBuffer::default();
        let live = Runtime::new(buffer.clone(), "content")
            .cursor_visibility(CursorVisibility::FromSurface)
            .start();
        drop(live);
        assert!(buffer.0.lock().unwrap().ends_with(b"\x1b[?25h"));
    }

    #[test]
    fn finish_does_not_emit_an_extra_show_when_the_final_frame_keeps_a_visible_cursor() {
        let root: WidgetRef = Arc::new(CursorWidget);
        let live = Runtime::new(Vec::new(), root)
            .cursor_visibility(CursorVisibility::FromSurface)
            .start();
        let output = live.finish().unwrap();
        let shows = output
            .windows(b"\x1b[?25h".len())
            .filter(|part| *part == b"\x1b[?25h")
            .count();
        assert_eq!(shows, 1);
    }

    struct LocalValue(Rc<RefCell<String>>);

    impl Widget for LocalValue {
        fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
            out.write(&*self.0.borrow(), Style::PLAIN);
        }
    }

    #[test]
    fn synchronous_runtime_accepts_a_local_root() {
        let value = Rc::new(RefCell::new("before".to_owned()));
        let root = local_widget(LocalValue(value.clone()));
        let mut runtime = Runtime::new(Vec::new(), root).width(12);
        runtime.draw_now(Instant::now()).unwrap();
        *value.borrow_mut() = "after".to_owned();
        runtime.mark_dirty();
        runtime.draw_now(Instant::now()).unwrap();
        assert!(!runtime.into_inner().is_empty());
    }

    struct SendOnlyWidget(Cell<usize>);

    impl Widget for SendOnlyWidget {
        fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
            self.0.set(self.0.get() + 1);
            out.write("owned", Style::PLAIN);
        }
    }

    #[test]
    fn live_runtime_requires_send_but_not_sync_for_an_owned_root() {
        let child: Box<dyn Widget + Send> = Box::new(SendOnlyWidget(Cell::new(0)));
        let output = Runtime::new(Vec::new(), Stack::new(vec![child]))
            .start()
            .finish()
            .unwrap();
        assert!(!output.is_empty());
    }

    #[derive(Debug)]
    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("distinctive failing writer message"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("distinctive failing writer message"))
        }
    }

    fn dead_render_thread() -> LiveRuntime<FailingWriter> {
        let live = Runtime::new(FailingWriter, "content").start();
        let deadline = Instant::now() + Duration::from_secs(5);
        while live.mark_dirty().is_ok() {
            assert!(
                Instant::now() < deadline,
                "render thread never reported the write failure"
            );
            thread::sleep(Duration::from_millis(10));
        }
        live
    }

    #[test]
    fn finish_cleared_reports_the_render_threads_error_after_it_has_already_exited() {
        let err = dead_render_thread().finish_cleared().unwrap_err();
        assert!(err.to_string().contains("distinctive failing writer message"));
    }

    #[test]
    fn finish_reports_the_render_threads_error_after_it_has_already_exited() {
        let err = dead_render_thread().finish().unwrap_err();
        assert!(err.to_string().contains("distinctive failing writer message"));
    }

    #[test]
    fn configured_final_widget_has_an_independent_type() {
        let plain = Runtime::auto(Vec::new(), "plain root", false)
            .final_widget("plain final".to_owned())
            .start()
            .finish()
            .unwrap();
        assert_eq!(plain, b"plain final");

        let live = Runtime::new(Vec::new(), "live root")
            .final_widget("live final".to_owned())
            .start()
            .finish()
            .unwrap();
        assert!(!live.is_empty());
    }

    #[test]
    fn finish_with_accepts_a_third_widget_type() {
        let output = Runtime::new(Vec::new(), "root")
            .final_widget("configured".to_owned())
            .start()
            .finish_with(Box::new("override") as Box<dyn Widget + Send>)
            .unwrap();
        assert!(!output.is_empty());
    }

    #[test]
    fn plain_auto_runtime_honours_resized_width_and_height() {
        let root: WidgetRef = Arc::new("one\ntwo\nthree".to_owned());
        let mut runtime = Runtime::auto(Vec::new(), root, false)
            .viewport(8, 3)
            .cursor_visibility(CursorVisibility::FromSurface)
            .start();
        runtime.resize_viewport(4, 1).unwrap();
        assert_eq!(runtime.finish().unwrap(), b"one");
    }

    #[derive(Clone)]
    struct ConstraintRecorder {
        seen: RecordedFrames,
    }

    impl Widget for ConstraintRecorder {
        fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
            self.seen.lock().unwrap().push((
                ctx.frame(),
                ctx.available_columns(),
                ctx.available_rows(),
            ));
            out.write("frame", Style::PLAIN);
        }
    }

    fn recording_widget() -> (WidgetRef, RecordedFrames) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        (Arc::new(ConstraintRecorder { seen: seen.clone() }), seen)
    }

    #[test]
    fn synchronous_live_and_plain_resize_paths_share_viewport_semantics() {
        let (synchronous_root, synchronous_seen) = recording_widget();
        let mut synchronous = Runtime::new(Vec::new(), synchronous_root).viewport(8, 3);
        synchronous.draw_now(Instant::now()).unwrap();
        synchronous.resize_viewport(5, 2);
        synchronous.draw_now(Instant::now()).unwrap();
        assert_eq!(
            synchronous_seen.lock().unwrap().as_slice(),
            [(0, Some(7), Some(3)), (1, Some(4), Some(2)),],
        );

        let (live_root, live_seen) = recording_widget();
        let live = Runtime::new(Vec::new(), live_root).viewport(8, 3).start();
        live.resize_viewport(5, 2).unwrap();
        live.finish().unwrap();
        let live_frames = live_seen.lock().unwrap();
        assert_eq!(
            live_frames
                .last()
                .map(|(_, columns, rows)| (*columns, *rows)),
            Some((Some(4), Some(2))),
        );
        assert!(
            live_frames.len() >= 2,
            "initial and resized frames are rendered"
        );
        drop(live_frames);

        let (plain_root, plain_seen) = recording_widget();
        let mut plain = Runtime::auto(Vec::new(), plain_root, false)
            .viewport(8, 3)
            .start();
        plain.resize_viewport(5, 2).unwrap();
        plain.finish().unwrap();
        assert_eq!(
            plain_seen.lock().unwrap().as_slice(),
            [(0, Some(4), Some(2))],
        );
    }

    #[test]
    fn stderr_constructor_takes_its_height_from_the_terminal() {
        let (root, seen) = recording_widget();
        let mut runtime = Runtime::stderr(root);
        runtime.draw_now(Instant::now()).unwrap();
        assert_eq!(
            seen.lock().unwrap().last().unwrap().2,
            crate::Viewport::of(&io::stderr()).ok().map(|size| size.rows),
        );
    }

    #[test]
    fn dropping_a_live_runtime_while_panicking_clears_instead_of_drawing_the_final_frame() {
        let buffer = SharedBuffer::default();
        let live = Runtime::new(buffer.clone(), "content")
            .final_widget("success frame")
            .start();

        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _live = live;
            panic!("simulated panic while a live runtime is in scope");
        }));
        assert!(unwound.is_err());

        let output = buffer.0.lock().unwrap().clone();
        let drew_final_frame = output
            .windows(b"success frame".len())
            .any(|part| part == b"success frame");
        assert!(
            !drew_final_frame,
            "a panicking drop must not draw the configured final frame: {:?}",
            String::from_utf8_lossy(&output),
        );
    }

    #[test]
    fn dropping_a_live_runtime_without_a_panic_still_draws_the_final_frame() {
        let buffer = SharedBuffer::default();
        let live = Runtime::new(buffer.clone(), "content")
            .final_widget("success frame")
            .start();
        drop(live);

        let drew_final_frame = {
            let output = buffer.0.lock().unwrap();
            output
                .windows(b"success frame".len())
                .any(|part| part == b"success frame")
        };
        assert!(drew_final_frame);
    }
}
