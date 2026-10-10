use std::{
    io::{self, Write},
    sync::mpsc::{
        self,
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
    renderer::layout_surface,
    stderr_is_terminal,
    terminal_width_or_default,
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
        Self::new(io::stderr(), root).width(terminal_width_or_default())
    }

    pub fn stderr_auto(root: WidgetRef) -> AutoRuntimeBuilder<io::Stderr> {
        Self::auto(io::stderr(), root, stderr_is_terminal()).width(terminal_width_or_default())
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
    Finish(ThreadFinishMode),
}

enum ThreadFinishMode {
    Current,
    With(Box<dyn crate::Widget + Send>),
    Clear,
}

pub struct LiveRuntime<W> {
    handle: RuntimeHandle,
    thread: Option<JoinHandle<io::Result<W>>>,
}

pub struct RuntimeHandle {
    tx: Sender<RuntimeCommand>,
}

pub struct AutoRuntimeBuilder<W, H = WidgetRef, F = WidgetRef> {
    writer:       W,
    root: H,
    interactive:  bool,
    fps:          u16,
    width:        Option<usize>,
    layout_mode: LayoutMode,
    cursor_visibility: CursorVisibility,
    theme:        Theme,
    final_widget: Option<F>,
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
            Self::Plain(runtime) => {
                runtime.resize(width);
                Ok(())
            },
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
    layout_mode:  LayoutMode,
    theme:        Theme,
    final_widget: Option<F>,
}

impl<W, H, F> PlainRuntime<W, H, F>
where
    W: Write,
    H: crate::Widget,
    F: crate::Widget,
{
    pub const fn resize(&mut self, width: usize) {
        self.width = Some(width);
    }

    pub fn finish(self) -> io::Result<W> {
        let Self {
            writer,
            root,
            width,
            layout_mode,
            theme,
            final_widget,
        } = self;
        if let Some(final_widget) = final_widget {
            write_plain_frame(writer, &final_widget, width, layout_mode, theme)
        } else {
            write_plain_frame(writer, &root, width, layout_mode, theme)
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
    layout_mode: LayoutMode,
    theme: Theme,
) -> io::Result<W>
where
    W: Write,
    G: crate::Widget,
{
    let mut surface = Surface::new();
    root.render(
        &RenderCtx::new().with_columns(width).with_theme(theme),
        &mut surface,
    );
    surface = layout_surface(surface, width, layout_mode);
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
        let frame_interval = runtime.frame_interval;
        let thread = thread::spawn(move || {
            runtime.draw_now(Instant::now())?;
            loop {
                match rx.recv_timeout(frame_interval) {
                    Ok(command @ (RuntimeCommand::Dirty | RuntimeCommand::Resize(_))) => {
                        apply_command(&mut runtime, &command);
                    },
                    Ok(RuntimeCommand::Finish(finish_mode)) => {
                        return finish_runtime(runtime, finish_mode);
                    },
                    Err(RecvTimeoutError::Disconnected) => {
                        return finish_runtime(runtime, ThreadFinishMode::Current);
                    },
                    Err(RecvTimeoutError::Timeout) => {
                        let _ = runtime.tick(Instant::now())?;
                    },
                }

                while let Ok(command) = rx.try_recv() {
                    match command {
                        RuntimeCommand::Dirty | RuntimeCommand::Resize(_) => {
                            apply_command(&mut runtime, &command);
                        },
                        RuntimeCommand::Finish(finish_mode) => {
                            return finish_runtime(runtime, finish_mode);
                        },
                    }
                }
                let _ = runtime.tick(Instant::now())?;
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

    pub fn finish(mut self) -> io::Result<W> {
        self.handle
            .send(RuntimeCommand::Finish(ThreadFinishMode::Current))?;
        self.join()
    }

    pub fn finish_with<G>(mut self, final_widget: G) -> io::Result<W>
    where
        G: crate::Widget + Send + 'static,
    {
        self.handle
            .send(RuntimeCommand::Finish(ThreadFinishMode::With(Box::new(
                final_widget,
            ))))?;
        self.join()
    }

    pub fn finish_cleared(mut self) -> io::Result<W> {
        self.handle
            .send(RuntimeCommand::Finish(ThreadFinishMode::Clear))?;
        self.join()
    }

    fn join(&mut self) -> io::Result<W> {
        let thread = self
            .thread
            .take()
            .expect("live runtime thread is joined at most once");
        thread
            .join()
            .map_err(|_| io::Error::other("runtime thread panicked"))?
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
            let _ = self
                .handle
                .send(RuntimeCommand::Finish(ThreadFinishMode::Current));
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

const fn apply_command<W, H, F>(runtime: &mut Runtime<W, H, F>, command: &RuntimeCommand)
where
    W: Write,
    H: crate::Widget,
{
    match command {
        RuntimeCommand::Dirty => runtime.mark_dirty(),
        RuntimeCommand::Resize(width) => runtime.resize(*width),
        RuntimeCommand::Finish(_) => {},
    }
}

fn finish_runtime<W, H, F>(
    mut runtime: Runtime<W, H, F>,
    finish_mode: ThreadFinishMode,
) -> io::Result<W>
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
    Ok(runtime.into_inner())
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
