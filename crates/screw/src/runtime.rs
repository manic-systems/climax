use std::{
    io::{self, Write},
    marker::PhantomData,
    sync::mpsc::{self, RecvTimeoutError, Sender},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    CursorVisibility, LayoutMode, RenderCtx, RenderStats, Renderer, Surface, Theme, TickInterest,
    WidgetRef,
    renderer::{layout_surface, usable_columns},
    stderr_is_terminal, terminal_width_or_default,
};

const DEFAULT_FPS: u16 = 15;

pub struct Runtime<W, H = WidgetRef, F = WidgetRef> {
    root: H,
    final_widget: Option<F>,
    renderer: Renderer<W>,
    frame_interval: Duration,
    last_draw: Option<Instant>,
    dirty: bool,
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

    pub const fn resize_viewport(&mut self, width: usize, height: usize) {
        self.renderer.resize_viewport(width, height);
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

    /// Move this runtime onto its rendering thread.
    ///
    /// Local references deliberately cannot cross this boundary:
    ///
    /// ```compile_fail
    /// let root = screw::local_widget("local");
    /// let _runtime = screw::Runtime::new(Vec::new(), root).start();
    /// ```
    pub fn start(self) -> LiveRuntime<W, H, F>
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
    ResizeViewport(usize, usize),
    Finish(ThreadFinishMode),
}

enum ThreadFinishMode {
    Current,
    With(Box<dyn crate::Widget + Send>),
    Clear,
}

pub struct LiveRuntime<W, H = WidgetRef, F = WidgetRef> {
    handle: RuntimeHandle,
    thread: Option<JoinHandle<io::Result<W>>>,
    widget_types: PhantomData<fn() -> (H, F)>,
}

pub struct RuntimeHandle {
    tx: Sender<RuntimeCommand>,
}

pub struct AutoRuntimeBuilder<W, H = WidgetRef, F = WidgetRef> {
    writer: W,
    root: H,
    interactive: bool,
    fps: u16,
    width: Option<usize>,
    height: Option<usize>,
    layout_mode: LayoutMode,
    cursor_visibility: CursorVisibility,
    theme: Theme,
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
                writer: self.writer,
                root: self.root,
                width: self.width,
                height: self.height,
                layout_mode: self.layout_mode,
                theme: self.theme,
                final_widget: self.final_widget,
            })
        }
    }
}

pub enum AutoRuntime<W, H = WidgetRef, F = WidgetRef> {
    Live(LiveRuntime<W, H, F>),
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

    pub fn resize_viewport(&mut self, width: usize, height: usize) -> io::Result<()> {
        match self {
            Self::Live(runtime) => runtime.resize_viewport(width, height),
            Self::Plain(runtime) => {
                runtime.resize_viewport(width, height);
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
    writer: W,
    root: H,
    width: Option<usize>,
    height: Option<usize>,
    layout_mode: LayoutMode,
    theme: Theme,
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

    pub const fn resize_viewport(&mut self, width: usize, height: usize) {
        self.width = Some(width);
        self.height = Some(height);
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
            render_plain(writer, &final_widget, width, height, layout_mode, theme)
        } else {
            render_plain(writer, &root, width, height, layout_mode, theme)
        }
    }

    pub fn finish_with<G>(self, final_widget: &G) -> io::Result<W>
    where
        G: crate::Widget,
    {
        render_plain(
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

fn render_plain<W, G>(
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

impl<W, H, F> LiveRuntime<W, H, F>
where
    W: Write + Send + 'static,
    H: crate::Widget + Send + 'static,
    F: crate::Widget + Send + 'static,
{
    fn start(mut runtime: Runtime<W, H, F>) -> Self {
        let (tx, rx) = mpsc::channel();
        let frame_interval = runtime.frame_interval;
        let thread = thread::spawn(move || {
            runtime.draw_now(Instant::now())?;
            loop {
                match rx.recv_timeout(frame_interval) {
                    Ok(
                        command @ (RuntimeCommand::Dirty
                        | RuntimeCommand::Resize(_)
                        | RuntimeCommand::ResizeViewport(_, _)),
                    ) => {
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
                        RuntimeCommand::Dirty
                        | RuntimeCommand::Resize(_)
                        | RuntimeCommand::ResizeViewport(_, _) => {
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
            widget_types: PhantomData,
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

impl<W, H, F> Drop for LiveRuntime<W, H, F> {
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
        RuntimeCommand::ResizeViewport(width, height) => runtime.resize_viewport(*width, *height),
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
                runtime.dirty = false;
                runtime.last_draw = Some(Instant::now());
                runtime.renderer.draw(&final_widget)?;
            } else {
                runtime.draw_now(Instant::now())?;
            }
        },
        ThreadFinishMode::With(final_widget) => {
            runtime.dirty = false;
            runtime.last_draw = Some(Instant::now());
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
}
