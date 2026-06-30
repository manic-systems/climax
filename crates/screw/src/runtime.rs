use std::{
    io::{
        self,
        Write,
    },
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
    LayoutMode,
    RenderCtx,
    RenderStats,
    Renderer,
    Surface,
    Theme,
    TickInterest,
    WidgetRef,
    renderer::layout_surface,
    stderr_is_terminal,
    terminal_width_or_default,
};

const DEFAULT_FPS: u16 = 15;

pub struct Runtime<W> {
    root:           WidgetRef,
    final_widget:   Option<WidgetRef>,
    renderer:       Renderer<W>,
    frame_interval: Duration,
    last_draw:      Option<Instant>,
    dirty:          bool,
}

impl<W> Runtime<W>
where
    W: Write,
{
    pub fn new(writer: W, root: WidgetRef) -> Self {
        Self {
            root,
            final_widget: None,
            renderer: Renderer::new(writer),
            frame_interval: fps_interval(DEFAULT_FPS),
            last_draw: None,
            dirty: true,
        }
    }

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
    pub fn theme(mut self, theme: Theme) -> Self {
        self.renderer = self.renderer.theme(theme);
        self
    }

    #[must_use]
    pub fn final_widget(mut self, final_widget: WidgetRef) -> Self {
        self.final_widget = Some(final_widget);
        self
    }

    pub const fn resize(&mut self, width: usize) {
        self.renderer.resize(width);
        self.dirty = true;
    }

    pub const fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn draw_now(&mut self, now: Instant) -> io::Result<RenderStats> {
        self.dirty = false;
        self.last_draw = Some(now);
        self.renderer.draw(&self.root)
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

    pub fn start(self) -> LiveRuntime<W>
    where
        W: Send + 'static,
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

impl Runtime<io::Stderr> {
    pub fn stderr(root: WidgetRef) -> Self {
        Self::new(io::stderr(), root).width(terminal_width_or_default())
    }

    pub fn stderr_auto(root: WidgetRef) -> AutoRuntimeBuilder<io::Stderr> {
        Self::auto(io::stderr(), root, stderr_is_terminal()).width(terminal_width_or_default())
    }
}

impl<W> Runtime<W>
where
    W: Write + Send + 'static,
{
    pub fn auto(writer: W, root: WidgetRef, interactive: bool) -> AutoRuntimeBuilder<W> {
        AutoRuntimeBuilder::new(writer, root, interactive)
    }
}

enum RuntimeCommand {
    Dirty,
    Resize(usize),
    Finish(FinishMode),
}

enum FinishMode {
    Current,
    With(WidgetRef),
    Clear,
}

pub struct LiveRuntime<W> {
    handle: RuntimeHandle,
    thread: Option<JoinHandle<io::Result<W>>>,
}

#[derive(Clone)]
pub struct RuntimeHandle {
    tx: Sender<RuntimeCommand>,
}

pub struct AutoRuntimeBuilder<W> {
    writer:       W,
    root:         WidgetRef,
    interactive:  bool,
    fps:          u16,
    width:        Option<usize>,
    layout_mode:  LayoutMode,
    theme:        Theme,
    final_widget: Option<WidgetRef>,
}

impl<W> AutoRuntimeBuilder<W>
where
    W: Write + Send + 'static,
{
    fn new(writer: W, root: WidgetRef, interactive: bool) -> Self {
        Self {
            writer,
            root,
            interactive,
            fps: DEFAULT_FPS,
            width: None,
            layout_mode: LayoutMode::Clip,
            theme: Theme::default(),
            final_widget: None,
        }
    }

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
    pub const fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    #[must_use]
    pub fn final_widget(mut self, final_widget: WidgetRef) -> Self {
        self.final_widget = Some(final_widget);
        self
    }

    pub fn start(self) -> AutoRuntime<W> {
        if self.interactive {
            let mut runtime = Runtime::new(self.writer, self.root).fps(self.fps);
            if let Some(width) = self.width {
                runtime = runtime.width(width);
            }
            runtime = runtime.layout_mode(self.layout_mode);
            runtime = runtime.theme(self.theme);
            if let Some(final_widget) = self.final_widget {
                runtime = runtime.final_widget(final_widget);
            }
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

pub enum AutoRuntime<W> {
    Live(LiveRuntime<W>),
    Plain(PlainRuntime<W>),
}

impl<W> AutoRuntime<W>
where
    W: Write + Send + 'static,
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

    pub fn finish_with(self, final_widget: WidgetRef) -> io::Result<W> {
        match self {
            Self::Live(runtime) => runtime.finish_with(final_widget),
            Self::Plain(runtime) => runtime.finish_with(final_widget),
        }
    }

    pub fn finish_cleared(self) -> io::Result<W> {
        match self {
            Self::Live(runtime) => runtime.finish_cleared(),
            Self::Plain(runtime) => runtime.finish_cleared(),
        }
    }
}

pub struct PlainRuntime<W> {
    writer:       W,
    root:         WidgetRef,
    width:        Option<usize>,
    layout_mode:  LayoutMode,
    theme:        Theme,
    final_widget: Option<WidgetRef>,
}

impl<W> PlainRuntime<W>
where
    W: Write,
{
    pub const fn resize(&mut self, width: usize) {
        self.width = Some(width);
    }

    pub fn finish(self) -> io::Result<W> {
        self.finish_mode(FinishMode::Current)
    }

    pub fn finish_with(self, final_widget: WidgetRef) -> io::Result<W> {
        self.finish_mode(FinishMode::With(final_widget))
    }

    pub fn finish_cleared(self) -> io::Result<W> {
        self.finish_mode(FinishMode::Clear)
    }

    fn finish_mode(mut self, finish_mode: FinishMode) -> io::Result<W> {
        if matches!(finish_mode, FinishMode::Clear) {
            self.writer.flush()?;
            return Ok(self.writer);
        }

        let mut surface = Surface::new();
        let root = match finish_mode {
            FinishMode::Current => self.final_widget.unwrap_or(self.root),
            FinishMode::With(final_widget) => final_widget,
            FinishMode::Clear => unreachable!("clear finish returned before rendering"),
        };
        root.render(
            &RenderCtx {
                frame: 0,
                width: self.width,
                theme: self.theme,
            },
            &mut surface,
        );
        surface = layout_surface(surface, self.width, self.layout_mode);
        self.writer.write_all(surface.plain_text().as_bytes())?;
        self.writer.flush()?;
        Ok(self.writer)
    }
}

impl<W> LiveRuntime<W>
where
    W: Write + Send + 'static,
{
    fn start(mut runtime: Runtime<W>) -> Self {
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
                        return finish_runtime(runtime, FinishMode::Current);
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
            .send(RuntimeCommand::Finish(FinishMode::Current))?;
        self.join()
    }

    pub fn finish_with(mut self, final_widget: WidgetRef) -> io::Result<W> {
        self.handle
            .send(RuntimeCommand::Finish(FinishMode::With(final_widget)))?;
        self.join()
    }

    pub fn finish_cleared(mut self) -> io::Result<W> {
        self.handle
            .send(RuntimeCommand::Finish(FinishMode::Clear))?;
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
                .send(RuntimeCommand::Finish(FinishMode::Current));
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}

const fn apply_command<W>(runtime: &mut Runtime<W>, command: &RuntimeCommand)
where
    W: Write,
{
    match command {
        RuntimeCommand::Dirty => runtime.mark_dirty(),
        RuntimeCommand::Resize(width) => runtime.resize(*width),
        RuntimeCommand::Finish(_) => {},
    }
}

fn finish_runtime<W>(mut runtime: Runtime<W>, finish_mode: FinishMode) -> io::Result<W>
where
    W: Write,
{
    match finish_mode {
        FinishMode::Current => {
            if let Some(final_widget) = runtime.final_widget.clone() {
                runtime.dirty = false;
                runtime.last_draw = Some(Instant::now());
                runtime.renderer.draw(&final_widget)?;
            } else {
                runtime.draw_now(Instant::now())?;
            }
        },
        FinishMode::With(final_widget) => {
            runtime.dirty = false;
            runtime.last_draw = Some(Instant::now());
            runtime.renderer.draw(&final_widget)?;
        },
        FinishMode::Clear => {
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
