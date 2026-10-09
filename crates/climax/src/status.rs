// SPDX-License-Identifier: EUPL-1.2

use std::{
    any::Any,
    collections::{BTreeMap, VecDeque},
    fmt,
    io::Write as _,
    os::fd::OwnedFd,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle, ThreadId},
    time::{Duration, Instant},
};

use screw::{Looping, RenderCtx, Renderer, Surface, Text, TickInterest, Widget, WidgetRef, layout, widget};

use crate::{
    Error, Result,
    error::ErrorKind,
    output::SharedWriter,
    terminal::{StatusMode, TerminalCapabilities},
};

/// Standalone status for use without a [`crate::Context`].
///
/// Uses a process-wide coordinator on stderr, separate from any
/// `Context::status` coordinator. Do not mix the two on the same stream;
/// each coordinator owns its own renderer.
#[must_use]
pub fn message(message: impl Into<String>) -> Status {
    static COORDINATOR: OnceLock<StatusCoordinator> = OnceLock::new();
    let coordinator = COORDINATOR.get_or_init(|| {
        let capabilities = TerminalCapabilities::detect();
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
    message:         String,
    widget:          Option<WidgetRef>,
    spinner:         bool,
    fps:             u16,
    final_message:   Option<String>,
    failure_message: Option<String>,
    coordinator: StatusCoordinator,
}

impl Status {
    #[must_use]
    pub(crate) fn new(message: impl Into<String>, coordinator: StatusCoordinator) -> Self {
        Self {
            message:         message.into(),
            widget:          None,
            spinner:         false,
            fps:             15,
            final_message:   None,
            failure_message: None,
            coordinator,
        }
    }

    /// Render `widget` in the live region in place of the message, after the
    /// spinner when there is one.
    ///
    /// The message is still the line plain status mode prints. The widget is
    /// drawn on the coordinator's thread, so state it reads from elsewhere has
    /// to be shared, and [`StatusRuntime::mark_dirty`] asks for a redraw after
    /// that state changes.
    ///
    /// Every prompt, notice and status removal waits on that same thread, so
    /// `render` must be quick and must not block, and it must not call back
    /// into anything that waits on the coordinator, which fails at once
    /// instead. A widget that panics is removed, and the failure is reported
    /// by that status's `finish`, or by the closing flush of the application
    /// lifecycle when its handle is still live then. No other call reports it.
    #[must_use]
    pub fn widget(mut self, widget: WidgetRef) -> Self {
        self.widget = Some(widget);
        self
    }

    #[must_use]
    pub const fn spinner(mut self) -> Self {
        self.spinner = true;
        self
    }

    /// Frame rate hint applied when this status is inserted.
    ///
    /// The status stack's draw rate only ratchets up to the highest fps any
    /// inserted status has requested, and stays there even after that status
    /// finishes.
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

    /// Message printed instead of [`Self::final_message`] when the operation
    /// [`Self::during`] runs fails, or when the handle is dropped without
    /// finishing. Nothing prints when this is unset.
    #[must_use]
    pub fn failure_message(mut self, message: impl Into<String>) -> Self {
        self.failure_message = Some(message.into());
        self
    }

    /// Start rendering and return a handle that removes the entry on `finish`.
    ///
    /// The handle must be bound: dropping it immediately removes the status.
    /// A failure to draw the first frame is retained and reported by the next
    /// [`StatusRuntime::finish`] or by the closing flush of the application
    /// lifecycle rather than discarded.
    #[must_use]
    pub fn start(self) -> StatusRuntime {
        let entry = StatusEntry {
            widget: self.root_widget(),
            plain: self.message,
            final_message: self.final_message,
            failure_message: self.failure_message,
        };
        let coordinator = self.coordinator.current().clone();
        let id = coordinator.insert(entry, self.fps);
        StatusRuntime {
            coordinator,
            id: Some(id),
        }
    }

    pub fn finish(self) -> Result<()> {
        self.start().finish()
    }

    pub fn during<T>(self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let mut status = self.start();
        match operation() {
            Ok(value) => {
                status.remove(true)?;
                Ok(value)
            },
            Err(error) => match status.remove(false) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(error.with_related(cleanup)),
            },
        }
    }

    fn root_widget(&self) -> WidgetRef {
        let body = self.widget.clone();
        match (self.spinner, body) {
            (false, Some(body)) => body,
            (false, None) => widget(Text::new(self.message.clone())),
            (true, body) => {
                layout()
                    .line(vec![
                        widget(Looping::new(["/", "-", "\\", "|"])),
                        widget(Text::new(" ")),
                        body.unwrap_or_else(|| widget(Text::new(self.message.clone()))),
                    ])
                    .into_widget()
            },
        }
    }
}

pub struct StatusRuntime {
    coordinator: StatusCoordinator,
    id: Option<u64>,
}

impl StatusRuntime {
    pub fn mark_dirty(&self) -> Result<()> {
        self.coordinator.mark_dirty()
    }

    pub fn finish(mut self) -> Result<()> {
        self.remove(true)
    }

    fn remove(&mut self, success: bool) -> Result<()> {
        self.id.take().map_or(Ok(()), |id| self.coordinator.remove(id, success))
    }
}

impl Drop for StatusRuntime {
    fn drop(&mut self) {
        let _result = self.remove(false);
    }
}

struct StatusEntry {
    widget: WidgetRef,
    plain: String,
    final_message: Option<String>,
    failure_message: Option<String>,
}

/// One thread owns the transient terminal, with the live renderer, the status
/// entries, the queued-line retry buffer and the prompt lease, so nothing
/// about the terminal is shared behind a lock.
///
/// Handles are cheap clones. The thread starts on the first command that
/// needs it and is joined, within a bound, when the last handle drops.
/// Inserting a status, marking it dirty and releasing a lease are
/// fire-and-forget, and every other call waits for a reply. The channel is
/// FIFO, so a command sent before a reply-bearing one from the same thread is
/// always applied first.
///
/// A notice waits until the line reaches the transient writer and returns the
/// write result, so it stays ordered against the caller's own writes to the
/// same stream and survives a process exit that follows. Writes to a stream
/// that shares the terminal, such as `diagnostic().stream` on stderr, take the
/// same path. While a prompt or terminal application holds the lease those
/// lines are queued instead and the call returns at once, so they are the only
/// writes that can still be pending after a return. Failed lines stay queued
/// and resume after the bytes the writer accepted. Broken pipe and EIO mean
/// the terminal is gone, so the channel is marked dead, the queue is dropped
/// and later lines are discarded. The queue holds at most 1024 lines and drops
/// the oldest past that. The first failure, plus counts of repeats, drops and
/// discards, is reported when a caller takes the report through a status
/// finish, a flush or the lifecycle commit.
///
/// Stream writes to a stdout that shares the terminal wait for a reply,
/// because stdout carries durable data. A blocked stdout therefore stalls
/// status drawing until it drains. A stream write requested while a prompt
/// holds the lease waits for the lease to end, unless the requesting thread
/// holds it, in which case it fails because it would wait on itself. The wait
/// is bounded like every request, so a lease holder that joins a worker
/// which is streaming sees the worker's write fail after `REQUEST_TIMEOUT`
/// and the join complete, and the write is dropped rather than made late.
///
/// Every request gives up with an error after `REQUEST_TIMEOUT` when a widget
/// blocks the thread, and the final join stops waiting after the same bound.
/// A request made from the actor thread itself, which only a widget can do,
/// fails at once because it would wait on itself.
#[derive(Clone)]
pub(crate) struct StatusCoordinator {
    inner: Arc<Inner>,
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const RESIZE_POLL: Duration = Duration::from_millis(250);

/// Where the live region reads its width from.
pub(crate) enum WidthSource {
    Fixed(usize),
    Stderr,
    Terminal(OwnedFd),
}

impl WidthSource {
    pub(crate) const FALLBACK: Self = Self::Fixed(screw::Viewport::FALLBACK.columns);

    fn measure(&self) -> Option<screw::Viewport> {
        match self {
            Self::Fixed(_) => None,
            Self::Stderr => screw::Viewport::of(&std::io::stderr()).ok(),
            Self::Terminal(handle) => screw::Viewport::of(handle).ok(),
        }
    }

    fn columns(&self) -> usize {
        match self {
            Self::Fixed(columns) => *columns,
            Self::Stderr | Self::Terminal(_) => self.measure().unwrap_or(screw::Viewport::FALLBACK).columns,
        }
    }

    /// A source that cannot be measured has nothing to follow, so it is
    /// pinned to the fallback width instead of being polled for resizes.
    fn resolved(self) -> Self {
        if matches!(self, Self::Fixed(_)) || self.measure().is_some() {
            self
        } else {
            Self::FALLBACK
        }
    }

    const fn follows_terminal(&self) -> bool {
        !matches!(self, Self::Fixed(_))
    }

    fn duplicate(&self) -> Self {
        match self {
            Self::Fixed(columns) => Self::Fixed(*columns),
            Self::Stderr => Self::Stderr,
            Self::Terminal(handle) => handle
                .try_clone()
                .map_or(Self::FALLBACK, Self::Terminal),
        }
    }
}

struct Inner {
    next_id: AtomicU64,
    writer: SharedWriter,
    mode: AtomicU64,
    width: WidthSource,
    request_timeout: Duration,
    channel: OnceLock<Channel>,
    successor: OnceLock<StatusCoordinator>,
}

struct Channel {
    sender: Sender<Command>,
    thread: JoinHandle<()>,
}

impl Inner {
    fn channel(&self) -> &Channel {
        let mut spawned_with = None;
        let channel = self.channel.get_or_init(|| {
            let stamp = self.mode.load(Ordering::Acquire);
            spawned_with = Some(stamp);
            let (sender, receiver) = mpsc::channel();
            let config = ActorConfig {
                writer: self.writer.clone(),
                stamp,
                width: self.width.duplicate(),
            };
            let thread = thread::spawn(move || run_actor(config, &receiver));
            Channel { sender, thread }
        });
        if let Some(stamp) = spawned_with {
            self.reconcile_mode(channel, stamp);
        }
        channel
    }

    /// A `set_mode` that ran between the actor reading its mode and the
    /// channel being published saw no channel and sent nothing, so the latest
    /// stored mode is replayed once the channel exists.
    fn reconcile_mode(&self, channel: &Channel, spawned_with: u64) {
        let latest = self.mode.load(Ordering::Acquire);
        if latest > spawned_with {
            let (reply, _discarded) = mpsc::channel();
            let _ = channel.sender.send(Command::SetMode { stamp: latest, reply });
        }
    }

    fn publish_mode(&self, mode: StatusMode) -> u64 {
        let next = |stamp: u64| ((stamp >> MODE_BITS) + 1) << MODE_BITS | u64::from(mode_to_u8(mode));
        let previous = self
            .mode
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |stamp| Some(next(stamp)))
            .unwrap_or_else(|stamp| stamp);
        next(previous)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(channel) = self.channel.take() {
            if channel.thread.thread().id() == thread::current().id() {
                return;
            }
            let (done, stopped) = mpsc::channel();
            let _ = channel.sender.send(Command::Stop { done });
            drop(channel.sender);
            match stopped.recv_timeout(self.request_timeout) {
                Err(RecvTimeoutError::Timeout) => {},
                Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                    let _ = channel.thread.join();
                },
            }
        }
    }
}

const fn mode_to_u8(mode: StatusMode) -> u8 {
    match mode {
        StatusMode::Auto => 0,
        StatusMode::Live => 1,
        StatusMode::Plain => 2,
        StatusMode::Silent => 3,
    }
}

/// The mode in the low bits and a generation above it, so the actor can tell
/// a replayed or reordered update from the latest one.
const MODE_BITS: u32 = 8;

const fn mode_from_stamp(stamp: u64) -> StatusMode {
    mode_from_u8((stamp & ((1 << MODE_BITS) - 1)) as u8)
}

const fn mode_from_u8(value: u8) -> StatusMode {
    match value {
        1 => StatusMode::Live,
        2 => StatusMode::Plain,
        3 => StatusMode::Silent,
        _ => StatusMode::Auto,
    }
}

enum Command {
    Insert {
        id: u64,
        entry: StatusEntry,
        fps: u16,
    },
    Remove {
        id: u64,
        success: bool,
        reply: Sender<Result<()>>,
    },
    MarkDirty,
    Stop {
        done: Sender<()>,
    },
    Notice {
        bytes: Vec<u8>,
        reply: Sender<Result<()>>,
    },
    #[cfg(feature = "structured")]
    Around {
        writer: SharedWriter,
        bytes: Vec<u8>,
        requester: ThreadId,
        deadline: Instant,
        reply: Sender<Result<()>>,
    },
    #[cfg(feature = "interactive")]
    AcquireLease {
        holder: ThreadId,
        reply: Sender<std::result::Result<(), LeaseError>>,
    },
    #[cfg(feature = "interactive")]
    ReleaseLease,
    SetMode {
        stamp: u64,
        reply: Sender<Result<()>>,
    },
    Flush {
        reply: Sender<Result<()>>,
    },
    IsIdle {
        reply: Sender<bool>,
    },
}

#[cfg(feature = "interactive")]
enum LeaseError {
    Busy,
    Clear(Error),
}

enum Failure {
    Stopped,
    TimedOut,
    OnActor,
}

impl From<Failure> for Error {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Stopped => actor_stopped(),
            Failure::TimedOut => output_error(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the transient actor did not answer in time, a status widget may be blocking it",
            )),
            Failure::OnActor => output_error(std::io::Error::other(
                "a status widget called into the transient actor it runs on, which would wait on itself",
            )),
        }
    }
}

impl StatusCoordinator {
    pub(crate) fn new(writer: SharedWriter, mode: StatusMode) -> Self {
        Self::with_width(writer, mode, WidthSource::Stderr)
    }

    pub(crate) fn with_width(writer: SharedWriter, mode: StatusMode, width: WidthSource) -> Self {
        Self::build(writer, mode, width, REQUEST_TIMEOUT)
    }

    fn build(writer: SharedWriter, mode: StatusMode, width: WidthSource, request_timeout: Duration) -> Self {
        Self {
            inner: Arc::new(Inner {
                next_id: AtomicU64::new(0),
                writer,
                mode: AtomicU64::new(mode_to_u8(mode).into()),
                width,
                request_timeout,
                channel: OnceLock::new(),
                successor: OnceLock::new(),
            }),
        }
    }

    fn send(&self, command: Command) {
        let _ = self.inner.channel().sender.send(command);
    }

    fn request<T>(&self, command: impl FnOnce(Sender<T>) -> Command, failed: impl FnOnce(Failure) -> T) -> T {
        let channel = self.inner.channel();
        if channel.thread.thread().id() == thread::current().id() {
            return failed(Failure::OnActor);
        }
        let (reply, receiver) = mpsc::channel();
        let _ = channel.sender.send(command(reply));
        match receiver.recv_timeout(self.inner.request_timeout) {
            Ok(value) => value,
            Err(RecvTimeoutError::Timeout) => failed(Failure::TimedOut),
            Err(RecvTimeoutError::Disconnected) => failed(Failure::Stopped),
        }
    }

    pub(crate) fn set_mode(&self, mode: StatusMode) -> Result<()> {
        let this = self.current();
        let stamp = this.inner.publish_mode(mode);
        if this.inner.channel.get().is_none() {
            return Ok(());
        }
        this.request(|reply| Command::SetMode { stamp, reply }, |failure| Err(failure.into()))
    }

    fn insert(&self, entry: StatusEntry, fps: u16) -> u64 {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.send(Command::Insert { id, entry, fps });
        id
    }

    fn remove(&self, id: u64, success: bool) -> Result<()> {
        self.request(
            |reply| Command::Remove { id, success, reply },
            |failure| Err(failure.into()),
        )
    }

    pub(crate) fn mark_dirty(&self) -> Result<()> {
        if self.inner.channel().sender.send(Command::MarkDirty).is_err() {
            return Err(actor_stopped());
        }
        Ok(())
    }

    /// Write `buffer` to the transient stream and wait for the result, or
    /// queue it and return at once while a prompt or terminal application holds
    /// the lease.
    pub(crate) fn notice(&self, buffer: &[u8]) -> Result<()> {
        self.current().request(
            |reply| Command::Notice { bytes: buffer.to_owned(), reply },
            |failure| Err(failure.into()),
        )
    }

    /// Clear the live region, write `bytes` directly to `writer` and flush,
    /// letting the next frame redraw below them.
    ///
    /// Waits for the lease when another thread holds it, giving up after the
    /// request bound, and fails when the calling thread holds it.
    #[cfg(feature = "structured")]
    pub(crate) fn write_around(&self, writer: SharedWriter, bytes: Vec<u8>) -> Result<()> {
        let this = self.current();
        let deadline = Instant::now() + this.inner.request_timeout;
        this.request(
            |reply| Command::Around { writer, bytes, requester: thread::current().id(), deadline, reply },
            |failure| Err(failure.into()),
        )
    }

    /// No live status, prompt lease or unwritten transient line depends on
    /// this coordinator's writer.
    pub(crate) fn is_idle(&self) -> bool {
        let this = self.current();
        if this.inner.channel.get().is_none() {
            return true;
        }
        this.request(|reply| Command::IsIdle { reply }, |failure| matches!(failure, Failure::Stopped))
    }

    /// Make `next` the coordinator that replaces this one, so a flush of this
    /// handle also reaches whatever the caller moved to.
    pub(crate) fn supersede(&self, next: &Self) {
        let _ = self.inner.successor.set(next.clone());
    }

    /// The end of the successor chain, where a handle kept from before a
    /// replacement sends its operations.
    fn current(&self) -> &Self {
        let mut current = self;
        while let Some(next) = current.inner.successor.get() {
            current = next;
        }
        current
    }

    /// Retry lines an earlier failed write left queued, on this coordinator
    /// and every one that replaced it.
    pub(crate) fn flush_transient(&self) -> Result<()> {
        let own = if self.inner.channel.get().is_none() {
            Ok(())
        } else {
            self.request(|reply| Command::Flush { reply }, |failure| Err(failure.into()))
        };
        let Some(next) = self.inner.successor.get() else {
            return own;
        };
        match (own, next.flush_transient()) {
            (Err(first), Err(later)) => Err(first.with_related(later)),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

fn actor_stopped() -> Error {
    output_error(std::io::Error::other("the transient actor thread stopped"))
}

#[derive(Clone)]
pub(crate) struct TransientNotice {
    coordinator: StatusCoordinator,
}

impl TransientNotice {
    pub(crate) const fn coordinator(coordinator: StatusCoordinator) -> Self {
        Self { coordinator }
    }
}

impl std::io::Write for TransientNotice {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.coordinator
            .notice(buffer)
            .map(|()| buffer.len())
            .map_err(std::io::Error::other)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl StatusCoordinator {
    #[cfg(feature = "interactive")]
    pub(crate) fn prompt_guard(&self) -> bang::Result<PromptGuard> {
        let this = self.current();
        match this.acquire_lease() {
            Ok(()) => Ok(PromptGuard { coordinator: this.clone() }),
            Err(LeaseError::Busy) => Err(bang::Error::interaction_busy()),
            Err(LeaseError::Clear(error)) => Err(bang::Error::terminal(error)),
        }
    }

    #[cfg(feature = "interactive")]
    pub(crate) fn application_guard(&self) -> Result<PromptGuard> {
        let this = self.current();
        match this.acquire_lease() {
            Ok(()) => Ok(PromptGuard { coordinator: this.clone() }),
            Err(LeaseError::Busy) => Err(Error::from(bang::Error::interaction_busy())),
            Err(LeaseError::Clear(error)) => Err(error),
        }
    }

    #[cfg(feature = "interactive")]
    fn acquire_lease(&self) -> std::result::Result<(), LeaseError> {
        self.request(
            |reply| Command::AcquireLease { holder: thread::current().id(), reply },
            |failure| Err(LeaseError::Clear(failure.into())),
        )
    }

    #[cfg(feature = "interactive")]
    pub(crate) fn writer(&self) -> SharedWriter {
        self.current().inner.writer.clone()
    }
}

impl fmt::Debug for StatusCoordinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusCoordinator").finish_non_exhaustive()
    }
}

#[cfg(feature = "interactive")]
pub(crate) struct PromptGuard {
    coordinator: StatusCoordinator,
}

#[cfg(feature = "interactive")]
impl Drop for PromptGuard {
    fn drop(&mut self) {
        self.coordinator.send(Command::ReleaseLease);
    }
}

const MAX_QUEUED_LINES: usize = 1024;

#[derive(Default)]
struct ErrorLog {
    error: Option<Error>,
    repeated: u64,
    capped: u64,
    discarded: u64,
}

impl ErrorLog {
    fn record(&mut self, error: Error) {
        if self.error.is_none() {
            self.error = Some(error);
        } else {
            self.repeated += 1;
        }
    }

    const fn note_capped(&mut self, count: u64) {
        self.capped += count;
    }

    const fn note_discarded(&mut self, count: u64) {
        self.discarded += count;
    }

    fn take(&mut self) -> Option<Error> {
        let mut error = self.error.take()?;
        if self.repeated > 0 {
            error =
                error.with_related(Error::message(format!("{} more transient failures of the same kind", self.repeated)));
        }
        if self.capped > 0 {
            error = error.with_related(Error::message(format!("{} transient lines were dropped", self.capped)));
        }
        if self.discarded > 0 {
            error = error.with_related(Error::message(format!("{} transient lines were discarded", self.discarded)));
        }
        self.repeated = 0;
        self.capped = 0;
        self.discarded = 0;
        Some(error)
    }
}

fn is_permanent(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::BrokenPipe || error.raw_os_error() == Some(5)
}

struct ActorConfig {
    writer: SharedWriter,
    stamp: u64,
    width: WidthSource,
}

#[cfg(feature = "structured")]
struct DeferredAround {
    writer: SharedWriter,
    bytes: Vec<u8>,
    deadline: Instant,
    reply: Sender<Result<()>>,
}

/// State owned by the transient-terminal thread.
struct Actor {
    renderer: Renderer<SharedWriter>,
    writer: SharedWriter,
    mode: StatusMode,
    mode_stamp: u64,
    width: WidthSource,
    columns: usize,
    last_width_check: Instant,
    fps: u16,
    entries: BTreeMap<u64, StatusEntry>,
    dirty: bool,
    last_draw: Option<Instant>,
    pending: VecDeque<Vec<u8>>,
    accepted: usize,
    flush_owed: bool,
    lease_holder: Option<ThreadId>,
    #[cfg(feature = "structured")]
    deferred: VecDeque<DeferredAround>,
    dead: bool,
    errors: ErrorLog,
    panicked: BTreeMap<u64, Error>,
}

impl Actor {
    fn new(config: ActorConfig) -> Self {
        let width = config.width.resolved();
        let columns = width.columns();
        Self {
            renderer: Renderer::new(config.writer.clone()).width(columns),
            writer: config.writer,
            mode: mode_from_stamp(config.stamp),
            mode_stamp: config.stamp,
            width,
            columns,
            last_width_check: Instant::now(),
            fps: 15,
            entries: BTreeMap::new(),
            dirty: false,
            last_draw: None,
            pending: VecDeque::new(),
            accepted: 0,
            flush_owed: false,
            lease_holder: None,
            #[cfg(feature = "structured")]
            deferred: VecDeque::new(),
            dead: false,
            errors: ErrorLog::default(),
            panicked: BTreeMap::new(),
        }
    }

    const fn leased(&self) -> bool {
        self.lease_holder.is_some()
    }

    #[cfg(feature = "structured")]
    fn around(&mut self, write: DeferredAround, requester: ThreadId) {
        if Instant::now() >= write.deadline {
            return;
        }
        match self.lease_holder {
            Some(holder) if holder == requester => {
                let _ = write.reply.send(Err(output_error(std::io::Error::other(
                    "the calling thread holds the terminal, so a stream write would wait on itself",
                ))));
            },
            Some(_) => self.deferred.push_back(write),
            None => {
                let _ = write.reply.send(self.write_around(write.writer, &write.bytes));
            },
        }
    }

    #[cfg(feature = "interactive")]
    fn acquire_lease(
        &mut self,
        holder: ThreadId,
        reply: &Sender<std::result::Result<(), LeaseError>>,
    ) {
        if self.leased() {
            let _ = reply.send(Err(LeaseError::Busy));
            return;
        }
        match self.clear_immediate() {
            Ok(()) => {
                if reply.send(Ok(())).is_ok() {
                    self.lease_holder = Some(holder);
                }
            },
            Err(error) => {
                let _ = reply.send(Err(LeaseError::Clear(error)));
            },
        }
    }

    fn remove(&mut self, id: u64, success: bool) {
        if let Some(entry) = self.entries.remove(&id) {
            if self.entries.is_empty() {
                self.clear_if_live();
            }
            self.dirty = true;
            let StatusEntry { widget, plain, final_message, failure_message } = entry;
            self.discard_widget(id, widget);
            let line = match (self.mode, success) {
                (StatusMode::Plain, true) => Some(final_message.unwrap_or(plain)),
                (StatusMode::Live, true) => final_message,
                (StatusMode::Plain | StatusMode::Live, false) => failure_message,
                // `Auto` is resolved to `Live`/`Silent` before
                // coordinators are built, so a raw `Auto` here means
                // silent.
                (StatusMode::Silent | StatusMode::Auto, _) => None,
            };
            if let Some(line) = line {
                self.enqueue(format!("{line}\n").into_bytes());
            }
            self.drain_pending();
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Insert { id, entry, fps } => {
                self.fps = self.fps.max(fps);
                self.entries.insert(id, entry);
                self.dirty = true;
            },
            Command::Remove { id, success, reply } => {
                self.remove(id, success);
                let panic = self.panicked.remove(&id);
                let _ = reply.send(self.report(panic));
            },
            Command::MarkDirty => self.dirty = true,
            Command::Stop { .. } => {},
            Command::Notice { bytes, reply } => {
                self.enqueue(bytes);
                let result = if self.leased() {
                    Ok(())
                } else {
                    self.drain_pending();
                    self.notice_result()
                };
                let _ = reply.send(result);
            },
            #[cfg(feature = "structured")]
            Command::Around { writer, bytes, requester, deadline, reply } => {
                self.around(DeferredAround { writer, bytes, deadline, reply }, requester);
            },
            #[cfg(feature = "interactive")]
            Command::AcquireLease { holder, reply } => self.acquire_lease(holder, &reply),
            #[cfg(feature = "interactive")]
            Command::ReleaseLease => {
                self.lease_holder = None;
                self.drain_pending();
                #[cfg(feature = "structured")]
                while let Some(DeferredAround { writer, bytes, deadline, reply }) = self.deferred.pop_front() {
                    if Instant::now() < deadline {
                        let _ = reply.send(self.write_around(writer, &bytes));
                    }
                }
                self.dirty = true;
            },
            Command::SetMode { stamp, reply } => {
                if stamp <= self.mode_stamp {
                    let _ = reply.send(Ok(()));
                    return;
                }
                let mode = mode_from_stamp(stamp);
                self.mode_stamp = stamp;
                let clearing = self.mode == StatusMode::Live && mode != StatusMode::Live;
                let result = if clearing { self.clear_immediate() } else { Ok(()) };
                self.mode = mode;
                self.dirty = true;
                let _ = reply.send(result);
            },
            Command::Flush { reply } => {
                self.drain_pending();
                let _ = reply.send(self.report_all());
            },
            Command::IsIdle { reply } => {
                let idle = self.entries.is_empty() && self.pending.is_empty() && !self.flush_owed && !self.leased();
                let _ = reply.send(idle);
            },
        }
    }

    #[cfg(feature = "structured")]
    fn write_around(&mut self, mut writer: SharedWriter, bytes: &[u8]) -> Result<()> {
        self.clear_if_live();
        self.dirty = true;
        writer.write_record(bytes).map_err(output_error)
    }

    fn notice_result(&mut self) -> Result<()> {
        if self.dead || !self.pending.is_empty() || self.flush_owed {
            return Err(self
                .errors
                .take()
                .unwrap_or_else(|| output_error(std::io::ErrorKind::BrokenPipe.into())));
        }
        Ok(())
    }

    fn report(&mut self, panic: Option<Error>) -> Result<()> {
        collect_reports(self.errors.take().into_iter().chain(panic))
    }

    fn report_all(&mut self) -> Result<()> {
        let panics = std::mem::take(&mut self.panicked);
        collect_reports(self.errors.take().into_iter().chain(panics.into_values()))
    }

    fn record_widget_panic(&mut self, ids: &[u64], what: &str, payload: &(dyn Any + Send)) {
        let message = format!("a status widget panicked while {what} and was removed, {}", panic_message(payload));
        for id in ids {
            let error = Error::message(message.clone());
            let error = match self.panicked.remove(id) {
                Some(first) => first.with_related(error),
                None => error,
            };
            self.panicked.insert(*id, error);
        }
        self.dirty = true;
    }

    fn discard_widget(&mut self, id: u64, widget: WidgetRef) {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| drop(widget))) {
            self.record_widget_panic(&[id], "dropping", payload.as_ref());
        }
    }

    /// Remove every entry whose widget panics when probed, or all of them when
    /// the panic cannot be pinned on one, so the next draw cannot panic again.
    fn remove_panicking_entries(&mut self, what: &str, payload: &(dyn Any + Send)) {
        let culprits: Vec<u64> = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                catch_unwind(AssertUnwindSafe(|| {
                    let _ = screw::render_plain(&*entry.widget);
                    let _ = entry.widget.tick_interest();
                }))
                .is_err()
            })
            .map(|(id, _)| *id)
            .collect();
        let doomed = if culprits.is_empty() { self.entries.keys().copied().collect() } else { culprits };
        self.record_widget_panic(&doomed, what, payload);
        for id in doomed {
            if let Some(entry) = self.entries.remove(&id) {
                self.discard_widget(id, entry.widget);
            }
        }
        if self.entries.is_empty() {
            self.clear_if_live();
        }
    }

    fn kill(&mut self) {
        if self.dead {
            return;
        }
        self.dead = true;
        let discarded = self.pending.len();
        self.pending.clear();
        self.accepted = 0;
        self.flush_owed = false;
        if discarded > 0 {
            self.errors.note_discarded(discarded as u64);
        }
    }

    fn record_transient_failure(&mut self, error: std::io::Error) {
        let permanent = is_permanent(&error);
        self.errors.record(output_error(error));
        if permanent {
            self.kill();
        }
    }

    fn immediate_failure(&mut self, error: std::io::Error) -> Error {
        if is_permanent(&error) {
            self.kill();
        }
        output_error(error)
    }

    fn clear_if_live(&mut self) {
        if self.dead {
            return;
        }
        if let Err(error) = self.renderer.clear() {
            self.record_transient_failure(error);
        }
    }

    fn clear_immediate(&mut self) -> Result<()> {
        if self.dead {
            return Ok(());
        }
        self.renderer.clear().map(|_stats| ()).map_err(|error| self.immediate_failure(error))
    }

    fn enqueue(&mut self, line: Vec<u8>) {
        if self.dead {
            self.errors.note_discarded(1);
            return;
        }
        self.pending.push_back(line);
        while self.pending.len() > MAX_QUEUED_LINES {
            self.pending.pop_front();
            self.accepted = 0;
            self.errors.note_capped(1);
        }
    }

    fn drain_pending(&mut self) {
        if self.leased() || self.dead {
            return;
        }
        if !self.pending.is_empty() {
            self.clear_if_live();
            self.dirty = true;
        }
        let mut writer = self.writer.clone();
        while let Some(line) = self.pending.front() {
            let result = writer.write(&line[self.accepted..]);
            let line_len = line.len();
            match result {
                Ok(0) => {
                    self.record_transient_failure(std::io::ErrorKind::WriteZero.into());
                    return;
                },
                Ok(accepted) => {
                    self.accepted += accepted;
                    self.flush_owed = true;
                    if self.accepted == line_len {
                        self.pending.pop_front();
                        self.accepted = 0;
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {},
                Err(error) => {
                    self.record_transient_failure(error);
                    return;
                },
            }
        }
        if self.flush_owed {
            match writer.flush() {
                Ok(()) => self.flush_owed = false,
                Err(error) => self.record_transient_failure(error),
            }
        }
    }

    fn refresh_width(&mut self) {
        self.last_width_check = Instant::now();
        let columns = self.width.columns();
        if columns != self.columns {
            self.columns = columns;
            self.renderer.resize(columns);
            self.dirty = true;
        }
    }

    fn poll_width(&mut self) {
        if self.width.follows_terminal()
            && self.wants_periodic_ticks()
            && self.last_width_check.elapsed() >= RESIZE_POLL
        {
            self.refresh_width();
        }
    }

    fn draw(&mut self) {
        if self.width.follows_terminal() {
            self.refresh_width();
        }
        let widgets: Vec<WidgetRef> = self.entries.values().map(|entry| entry.widget.clone()).collect();
        match catch_unwind(AssertUnwindSafe(|| self.renderer.draw(&StatusStack(widgets)))) {
            Ok(Ok(_)) => self.dirty = false,
            Ok(Err(error)) => {
                self.record_transient_failure(error);
                self.dirty = false;
            },
            Err(payload) => self.remove_panicking_entries("rendering", payload.as_ref()),
        }
        self.last_draw = Some(Instant::now());
    }

    fn wants_periodic_ticks(&self) -> bool {
        self.mode == StatusMode::Live && !self.leased() && !self.dead && !self.entries.is_empty()
    }

    fn frame_interval(&self) -> Duration {
        Duration::from_nanos(1_000_000_000 / u64::from(self.fps.max(1)))
    }

    fn tick_interest(&mut self) -> TickInterest {
        match catch_unwind(AssertUnwindSafe(|| combined_tick_interest(&self.entries))) {
            Ok(interest) => interest,
            Err(payload) => {
                self.remove_panicking_entries("reporting its tick interest", payload.as_ref());
                TickInterest::Never
            },
        }
    }

    fn should_draw(&mut self) -> bool {
        if !self.wants_periodic_ticks() {
            return false;
        }
        let Some(last_draw) = self.last_draw else { return true };
        let elapsed = Instant::now().saturating_duration_since(last_draw);
        if elapsed < self.frame_interval() {
            return false;
        }
        self.dirty || wants_frame_tick(self.tick_interest(), elapsed)
    }

    /// How long the thread may sleep before it next has to act, or `None` to
    /// sleep until a command arrives.
    fn next_wake(&mut self) -> Option<Duration> {
        if !self.wants_periodic_ticks() {
            return None;
        }
        let Some(last_draw) = self.last_draw else { return Some(Duration::ZERO) };
        let elapsed = Instant::now().saturating_duration_since(last_draw);
        let until_frame = self.frame_interval().saturating_sub(elapsed);
        let until_draw = if self.dirty {
            Some(until_frame)
        } else {
            match self.tick_interest() {
                TickInterest::Never => None,
                TickInterest::EveryFrame => Some(until_frame),
                TickInterest::Every(interval) => Some(until_frame.max(interval.saturating_sub(elapsed))),
            }
        };
        // Terminals give no resize notice without a signal handler, so a
        // width read from one is polled.
        let until_resize = self
            .width
            .follows_terminal()
            .then(|| RESIZE_POLL.saturating_sub(self.last_width_check.elapsed()));
        match (until_draw, until_resize) {
            (Some(draw), Some(resize)) => Some(draw.min(resize)),
            (wake, None) | (None, wake) => wake,
        }
    }
}

fn run_actor(config: ActorConfig, receiver: &Receiver<Command>) {
    let mut actor = Actor::new(config);
    loop {
        let event = match actor.next_wake() {
            None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(wait) => receiver.recv_timeout(wait),
        };
        match event {
            Ok(Command::Stop { done }) => {
                let _ = done.send(());
                break;
            },
            Ok(command) => actor.apply(command),
            Err(RecvTimeoutError::Timeout) => {},
            Err(RecvTimeoutError::Disconnected) => break,
        }
        actor.poll_width();
        if actor.should_draw() {
            actor.draw();
        }
    }
}

fn collect_reports(errors: impl IntoIterator<Item = Error>) -> Result<()> {
    let mut errors = errors.into_iter();
    errors.next().map_or(Ok(()), |first| Err(errors.fold(first, Error::with_related)))
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|message| (*message).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "with a non-string payload".to_owned())
}

struct StatusStack(Vec<WidgetRef>);

impl Widget for StatusStack {
    fn render(&self, context: &RenderCtx, output: &mut Surface) {
        for (index, widget) in self.0.iter().enumerate() {
            if index > 0 {
                output.newline();
            }
            widget.render(context, output);
        }
    }
}

fn combined_tick_interest(entries: &BTreeMap<u64, StatusEntry>) -> TickInterest {
    screw::combine_tick_interest(entries.values().map(|entry| entry.widget.as_ref().tick_interest()))
}

fn wants_frame_tick(interest: TickInterest, elapsed: Duration) -> bool {
    match interest {
        TickInterest::Never => false,
        TickInterest::EveryFrame => true,
        TickInterest::Every(interval) => elapsed >= interval,
    }
}

fn output_error(error: std::io::Error) -> Error {
    Error::with_source(ErrorKind::Output, error)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        io,
        sync::{
            Barrier, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use crate::sync::lock;

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

    fn entry(message: &str) -> StatusEntry {
        StatusEntry {
            widget: widget(Text::new(message.to_owned())),
            plain: message.to_owned(),
            final_message: None,
            failure_message: None,
        }
    }

    /// Poll `condition` until it holds or a generous deadline passes, then
    /// assert it. Fire-and-forget effects need this, and no test asserts a
    /// timing bound with it.
    fn wait_for(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(condition(), "condition was not met within the deadline");
    }

    #[test]
    fn status_stack_composes_multiple_animating_widgets() {
        let stack = StatusStack(vec![entry("first").widget, entry("second").widget]);
        assert_eq!(screw::render_plain(&stack), "first\nsecond");
    }

    #[test]
    fn a_custom_widget_replaces_the_message_after_the_spinner() {
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Silent);
        let plain = Status::new("working", coordinator.clone())
            .widget(widget(Text::new("3 of 5")))
            .root_widget();
        assert_eq!(screw::render_plain(&plain), "3 of 5");

        let spinning = Status::new("working", coordinator)
            .spinner()
            .widget(widget(Text::new("3 of 5")))
            .root_widget();
        assert_eq!(screw::render_plain(&spinning), "/ 3 of 5");
    }

    #[test]
    fn mark_dirty_redraws_a_custom_widget_from_shared_state() {
        struct Progress(Arc<AtomicU64>);

        impl Widget for Progress {
            fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
                let done = self.0.load(Ordering::Relaxed);
                out.write(format!("{done} of 5"), screw::Style::PLAIN);
            }
        }

        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Live);
        let done = Arc::new(AtomicU64::new(1));
        let status = Status::new("copying", coordinator)
            .widget(widget(Progress(Arc::clone(&done))))
            .start();

        wait_for(|| capture.text().contains("1 of 5"));
        let first = capture.text().len();
        done.store(4, Ordering::Relaxed);
        status.mark_dirty().unwrap();
        wait_for(|| capture.text()[first..].contains('4'));
        status.finish().unwrap();
    }

    #[test]
    fn plain_mode_prints_the_message_of_a_custom_widget_status() {
        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        Status::new("copying", coordinator)
            .widget(widget(Text::new("3 of 5")))
            .finish()
            .unwrap();
        assert_eq!(capture.text(), "copying\n");
    }

    #[test]
    fn a_builder_saved_before_a_replacement_starts_on_the_successor() {
        let old = Capture::default();
        let new = Capture::default();
        let previous = StatusCoordinator::new(SharedWriter::new(old.clone()), StatusMode::Plain);
        let saved = Status::new("saved", previous.clone());
        let next = StatusCoordinator::new(SharedWriter::new(new.clone()), StatusMode::Plain);
        previous.supersede(&next);

        saved.finish().unwrap();
        previous.notice(b"late\n").unwrap();

        assert_eq!(old.text(), "");
        assert_eq!(new.text(), "saved\nlate\n");
    }

    #[test]
    fn the_status_stack_does_not_keep_its_coordinator_alive() {
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Live);
        let weak = Arc::downgrade(&coordinator.inner);
        let status = Status::new("working", coordinator.clone())
            .spinner()
            .start();

        status.finish().unwrap();
        drop(coordinator);

        assert!(
            weak.upgrade().is_none(),
            "the actor thread's root widget must not own the coordinator"
        );
    }

    #[test]
    fn concurrent_statuses_share_one_render_thread() {
        struct Probe(Arc<Mutex<HashSet<std::thread::ThreadId>>>);

        impl Widget for Probe {
            fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
                lock(&self.0).insert(std::thread::current().id());
                out.write("working", screw::Style::PLAIN);
            }
        }

        let seen = Arc::new(Mutex::new(HashSet::new()));
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Live);
        let barrier = Arc::new(Barrier::new(17));
        let ids = std::thread::scope(|scope| {
            let mut threads = Vec::new();
            for _ in 0..16 {
                let seen = Arc::clone(&seen);
                let barrier = Arc::clone(&barrier);
                let coordinator = coordinator.clone();
                threads.push(scope.spawn(move || {
                    barrier.wait();
                    coordinator.insert(
                        StatusEntry {
                            widget: widget(Probe(seen)),
                            plain: "working".to_owned(),
                            final_message: None,
                            failure_message: None,
                        },
                        15,
                    )
                }));
            }
            barrier.wait();
            threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect::<Vec<_>>()
        });

        wait_for(|| !lock(&seen).is_empty());
        assert_eq!(lock(&seen).len(), 1);
        for id in ids {
            coordinator.remove(id, true).unwrap();
        }
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
        assert!(capture.text().is_empty());
    }

    #[test]
    fn during_prints_nothing_on_failure_without_a_failure_message() {
        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        let error = Status::new("working", coordinator)
            .final_message("done")
            .during::<()>(|| Err(crate::Error::message("work failed")))
            .unwrap_err();
        assert_eq!(error.to_string(), "work failed");
        assert!(capture.text().is_empty());
    }

    #[test]
    fn during_prints_the_failure_message_on_error() {
        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        let error = Status::new("working", coordinator)
            .final_message("done")
            .failure_message("failed")
            .during::<()>(|| Err(crate::Error::message("work failed")))
            .unwrap_err();
        assert_eq!(error.to_string(), "work failed");
        assert_eq!(capture.text(), "failed\n");
    }

    #[test]
    fn dropping_an_unfinished_status_prints_its_failure_message() {
        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        let status = Status::new("working", coordinator)
            .final_message("done")
            .failure_message("aborted")
            .start();
        drop(status);
        wait_for(|| capture.text() == "aborted\n");
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
            .failure_message("working failed")
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

        // A draw failure never marks the channel dead, so `mark_dirty` keeps
        // succeeding while failures accumulate.
        wait_for(|| attempts.load(Ordering::SeqCst) >= 1);
        coordinator.mark_dirty().unwrap();
        wait_for(|| attempts.load(Ordering::SeqCst) >= 2);
        coordinator.mark_dirty().unwrap();

        let error = status.finish().unwrap_err();

        assert!(attempts.load(Ordering::SeqCst) >= 2);
        assert!(
            !error.related_errors().is_empty(),
            "later draw failures should be retained"
        );
    }

    #[test]
    fn a_failed_first_frame_is_reported_by_finish() {
        #[derive(Clone, Copy)]
        struct FailingWriter;

        impl io::Write for FailingWriter {
            fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("first frame failed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let coordinator =
            StatusCoordinator::new(SharedWriter::new(FailingWriter), StatusMode::Live);
        let status = Status::new("working", coordinator).start();

        let error = status.finish().unwrap_err();

        assert_eq!(error.kind(), ErrorKind::Output);
        assert!(error.to_string().contains("first frame failed"));
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn prompt_exclusivity_suspends_and_restores_status_presentation() {
        let capture = Capture::default();
        let coordinator = StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Live);
        let status = Status::new("working", coordinator.clone())
            .spinner()
            .start();
        wait_for(|| !capture.text().is_empty());

        let guard = coordinator.prompt_guard().unwrap();
        assert!(coordinator.prompt_guard().is_err());
        let before = capture.text();
        coordinator.mark_dirty().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(capture.text(), before, "no draw happens while leased");

        drop(guard);
        wait_for(|| capture.text() != before);
        status.finish().unwrap();
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn finishing_statuses_defer_final_lines_while_a_prompt_holds_the_lease() {
        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        let status = Status::new("working", coordinator.clone())
            .final_message("finished")
            .start();
        let guard = coordinator.prompt_guard().unwrap();
        status.finish().unwrap();
        assert!(capture.text().is_empty());
        drop(guard);
        wait_for(|| capture.text() == "finished\n");
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn a_failed_write_keeps_lines_queued_until_the_next_emit() {
        struct FailOnce {
            fail:   bool,
            output: Capture,
        }

        impl io::Write for FailOnce {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.fail {
                    self.fail = false;
                    return Err(io::Error::other("first write failed"));
                }
                self.output.write(bytes)
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let output = Capture::default();
        let coordinator = StatusCoordinator::new(
            SharedWriter::new(FailOnce {
                fail:   true,
                output: output.clone(),
            }),
            StatusMode::Plain,
        );
        let guard = coordinator.prompt_guard().unwrap();
        coordinator.notice(b"first\n").unwrap();
        coordinator.notice(b"second\n").unwrap();
        drop(guard);

        // The very first write attempt anywhere is scripted to fail before
        // writing bytes, so nothing can have reached `output` yet.
        assert!(output.text().is_empty());
        coordinator.notice(b"third\n").unwrap();
        assert_eq!(output.text(), "first\nsecond\nthird\n");
    }

    #[test]
    fn unwritable_lines_stay_queued_and_fail_the_commit_flush() {
        struct Closed;

        impl io::Write for Closed {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("closed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let coordinator = StatusCoordinator::new(SharedWriter::new(Closed), StatusMode::Plain);
        assert!(coordinator.notice(b"lost\n").is_err());
        assert_eq!(
            coordinator.flush_transient().unwrap_err().kind(),
            ErrorKind::Output
        );
    }

    #[test]
    fn a_retry_resumes_after_the_bytes_a_partial_write_accepted() {
        struct Partial {
            limit:  Option<usize>,
            output: Capture,
        }

        impl io::Write for Partial {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                match self.limit.take() {
                    Some(0) => Err(io::ErrorKind::WouldBlock.into()),
                    Some(limit) => {
                        self.limit = Some(0);
                        self.output.write(&bytes[..limit])
                    },
                    None => self.output.write(bytes),
                }
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let output = Capture::default();
        let coordinator = StatusCoordinator::new(
            SharedWriter::new(Partial {
                limit:  Some(3),
                output: output.clone(),
            }),
            StatusMode::Plain,
        );
        assert!(coordinator.notice(b"hello\n").is_err());
        coordinator.notice(b"next\n").unwrap();
        assert_eq!(output.text(), "hello\nnext\n");
    }

    #[test]
    fn a_failed_flush_is_retried_without_rewriting_accepted_lines() {
        struct Buffered {
            buffer:     Vec<u8>,
            fail_flush: bool,
            output:     Capture,
        }

        impl io::Write for Buffered {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.buffer.extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                if std::mem::take(&mut self.fail_flush) {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                self.output.write_all(&std::mem::take(&mut self.buffer))
            }
        }

        let output = Capture::default();
        let coordinator = StatusCoordinator::new(
            SharedWriter::new(Buffered {
                buffer:     Vec::new(),
                fail_flush: true,
                output:     output.clone(),
            }),
            StatusMode::Plain,
        );
        let error = coordinator.notice(b"first\n").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Output);
        assert!(output.text().is_empty());
        coordinator.flush_transient().unwrap();
        assert_eq!(output.text(), "first\n");
        coordinator.flush_transient().unwrap();
    }

    #[test]
    fn concurrent_final_lines_do_not_interleave() {
        let capture = Capture::default();
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        let barrier = Arc::new(Barrier::new(9));
        std::thread::scope(|scope| {
            for index in 0..8 {
                let coordinator = coordinator.clone();
                let barrier = Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    Status::new(format!("work-{index}"), coordinator)
                        .final_message(format!("done-{index}"))
                        .finish()
                        .unwrap();
                });
            }
            barrier.wait();
        });
        let output = capture.text();
        let mut lines: Vec<&str> = output.lines().collect();
        lines.sort_unstable();
        assert_eq!(lines, [
            "done-0", "done-1", "done-2", "done-3", "done-4", "done-5", "done-6", "done-7",
        ]);
    }

    #[test]
    fn formatting_the_coordinator_does_not_block_on_concurrent_status_churn() {
        let coordinator =
            StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Live);
        let stop = Arc::new(AtomicBool::new(false));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let coordinator = coordinator.clone();
                let stop = Arc::clone(&stop);
                scope.spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        Status::new("working", coordinator.clone())
                            .start()
                            .finish()
                            .unwrap();
                    }
                });
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                let _ = format!("{coordinator:?}");
            }
            stop.store(true, Ordering::Relaxed);
        });
    }

    #[test]
    fn a_broken_pipe_marks_the_channel_dead_and_reports_a_discard_count() {
        struct BrokenPipe;

        impl io::Write for BrokenPipe {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let coordinator = StatusCoordinator::new(SharedWriter::new(BrokenPipe), StatusMode::Plain);
        let error = coordinator.notice(b"first\n").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Output);
        assert!(error.related_errors().iter().any(|related| related.to_string().contains("discarded")));

        for _ in 0..10 {
            assert!(coordinator.notice(b"after-death\n").is_err());
        }
        coordinator.flush_transient().unwrap();
    }

    #[test]
    fn more_than_a_thousand_queued_lines_drop_the_oldest_and_report_the_count() {
        struct AlwaysFails;

        impl io::Write for AlwaysFails {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("still failing"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let coordinator = StatusCoordinator::new(SharedWriter::new(AlwaysFails), StatusMode::Plain);
        let reported_drop = (0..1100)
            .filter_map(|_| coordinator.notice(b"line\n").err())
            .inspect(|error| assert_eq!(error.kind(), ErrorKind::Output))
            .any(|error| error.related_errors().iter().any(|related| related.to_string().contains("dropped")));
        assert!(reported_drop);
    }

    #[cfg(feature = "structured")]
    #[test]
    fn a_stream_write_through_around_appears_after_the_live_region_clears_and_is_not_overwritten() {
        let capture = Capture::default();
        let coordinator = StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Live);
        let status = Status::new("working", coordinator.clone())
            .spinner()
            .start();
        wait_for(|| !capture.text().is_empty());
        let before = capture.text();

        coordinator
            .write_around(SharedWriter::new(capture.clone()), b"stream line\n".to_vec())
            .unwrap();
        let after_around = capture.text();
        let appended = &after_around[before.len()..];
        assert!(appended.ends_with("stream line\n"));
        assert!(
            appended.len() > b"stream line\n".len(),
            "a clear sequence must precede the line: {appended:?}"
        );

        wait_for(|| capture.text().len() > after_around.len());
        status.finish().unwrap();
    }

    #[test]
    fn a_notice_under_a_live_status_clears_the_region_before_it_is_written() {
        let capture = Capture::default();
        let coordinator = StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Live);
        let status = Status::new("working", coordinator.clone()).start();
        wait_for(|| !capture.text().is_empty());
        let before = capture.text();

        coordinator.notice(b"notice line\n").unwrap();
        let appended = capture.text()[before.len()..].to_owned();
        let at = appended.find("notice line\n").expect("the notice was written");
        assert!(at > 0, "a clear sequence must precede the notice: {appended:?}");

        status.finish().unwrap();
    }

    #[test]
    fn a_notice_waits_for_the_write_and_returns_its_failure() {
        struct AlwaysFails;

        impl io::Write for AlwaysFails {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("notice sink failed"))
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let coordinator = StatusCoordinator::new(SharedWriter::new(AlwaysFails), StatusMode::Plain);
        let mut notice = TransientNotice::coordinator(coordinator);
        let error = io::Write::write(&mut notice, b"trouble\n").unwrap_err();
        assert!(error.to_string().contains("notice sink failed"));
    }

    #[test]
    fn a_notice_has_reached_the_writer_when_it_returns() {
        let capture = Capture::default();
        let coordinator = StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        let mut notice = TransientNotice::coordinator(coordinator);
        for index in 0..50 {
            io::Write::write_all(&mut notice, format!("notice {index}\n").as_bytes()).unwrap();
            assert!(capture.text().ends_with(&format!("notice {index}\n")));
        }
    }

    #[test]
    fn a_notice_after_the_actor_died_is_an_error() {
        struct Panics;

        impl io::Write for Panics {
            fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
                panic!("writer exploded");
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let coordinator = StatusCoordinator::new(SharedWriter::new(Panics), StatusMode::Plain);
        assert!(coordinator.notice(b"first\n").is_err());
        let error = coordinator.notice(b"second\n").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Output);
        assert!(error.to_string().contains("stopped"));
    }

    #[cfg(feature = "interactive")]
    #[test]
    fn a_notice_under_a_lease_returns_at_once_and_is_written_on_release() {
        let capture = Capture::default();
        let coordinator = StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Plain);
        let guard = coordinator.prompt_guard().unwrap();
        coordinator.notice(b"queued\n").unwrap();
        assert!(capture.text().is_empty());
        drop(guard);
        wait_for(|| capture.text() == "queued\n");
    }

    #[cfg(all(feature = "interactive", feature = "structured"))]
    #[test]
    fn a_stream_write_waits_for_another_threads_lease_and_fails_on_the_holders_own() {
        let capture = Capture::default();
        let coordinator = StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Plain);
        let guard = coordinator.prompt_guard().unwrap();

        let own = coordinator
            .write_around(SharedWriter::new(capture.clone()), b"own\n".to_vec())
            .unwrap_err();
        assert!(own.to_string().contains("would wait on itself"));

        let other = std::thread::scope(|scope| {
            let streaming = scope.spawn(|| {
                coordinator.write_around(SharedWriter::new(capture.clone()), b"other\n".to_vec())
            });
            std::thread::sleep(Duration::from_millis(100));
            assert!(capture.text().is_empty(), "the write must wait for the lease");
            drop(guard);
            streaming.join().unwrap()
        });
        other.unwrap();
        assert_eq!(capture.text(), "other\n");
    }

    fn actor_with(width: WidthSource) -> Actor {
        Actor::new(ActorConfig {
            writer: SharedWriter::new(Capture::default()),
            stamp: mode_to_u8(StatusMode::Live).into(),
            width,
        })
    }

    fn drawn_static_status(actor: &mut Actor) {
        actor.apply(Command::Insert { id: 0, entry: entry("static"), fps: 15 });
        actor.draw();
    }

    #[test]
    fn an_idle_static_live_status_does_not_wake_the_actor() {
        let mut fixed = actor_with(WidthSource::Fixed(40));
        drawn_static_status(&mut fixed);
        assert_eq!(fixed.next_wake(), None);

        let null = std::fs::File::open("/dev/null").unwrap();
        let mut not_a_terminal = actor_with(WidthSource::Terminal(null.into()));
        drawn_static_status(&mut not_a_terminal);
        assert_eq!(not_a_terminal.next_wake(), None);
    }

    #[test]
    fn stderr_is_polled_for_resizes_only_when_it_is_a_terminal() {
        let mut actor = actor_with(WidthSource::Stderr);
        drawn_static_status(&mut actor);
        let is_terminal = screw::Viewport::of(&std::io::stderr()).is_ok();
        assert_eq!(actor.next_wake().is_some(), is_terminal);
    }

    struct PanicsOnRender;

    impl Widget for PanicsOnRender {
        fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {
            panic!("widget exploded");
        }
    }

    fn start_broken(coordinator: &StatusCoordinator) -> StatusRuntime {
        let broken = Status::new("broken", coordinator.clone()).widget(widget(PanicsOnRender)).start();
        // The first insert draws at once, and a reply means the draw has run.
        coordinator.is_idle();
        broken
    }

    #[test]
    fn a_panicking_widget_is_reported_by_its_own_finish_and_the_actor_survives() {
        let capture = Capture::default();
        let coordinator = StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Live);
        let broken = start_broken(&coordinator);
        let healthy = Status::new("healthy", coordinator.clone()).start();

        coordinator.mark_dirty().unwrap();
        coordinator.notice(b"still alive\n").unwrap();
        assert!(capture.text().contains("still alive"));
        let error = broken.finish().unwrap_err();
        assert!(error.to_string().contains("widget exploded"), "{error}");

        healthy.finish().unwrap();
        coordinator.flush_transient().unwrap();
        Status::new("later", coordinator).final_message("later done").finish().unwrap();
    }

    #[test]
    fn a_stale_widget_panic_report_reaches_no_other_call() {
        let coordinator = StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Live);
        let broken = start_broken(&coordinator);
        drop(broken);

        coordinator.notice(b"unrelated\n").unwrap();
        Status::new("other", coordinator.clone()).finish().unwrap();
        coordinator.flush_transient().unwrap();
    }

    #[test]
    fn the_closing_flush_reports_a_panicked_status_that_is_still_live() {
        let coordinator = StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Live);
        let broken = start_broken(&coordinator);

        coordinator.notice(b"unrelated\n").unwrap();
        let error = coordinator.flush_transient().unwrap_err();
        assert!(error.to_string().contains("widget exploded"), "{error}");
        coordinator.flush_transient().unwrap();
        drop(broken);
    }

    #[test]
    fn a_widget_that_waits_on_the_coordinator_it_runs_on_fails_at_once() {
        struct Notifies(StatusCoordinator, Arc<Mutex<Option<Error>>>);

        impl Widget for Notifies {
            fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
                *lock(&self.1) = self.0.notice(b"from the widget\n").err();
                out.write("x", screw::Style::PLAIN);
            }
        }

        let coordinator = StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Live);
        let seen = Arc::new(Mutex::new(None));
        let started = Instant::now();
        let status = Status::new("widget", coordinator.clone())
            .widget(widget(Notifies(coordinator, Arc::clone(&seen))))
            .start();
        wait_for(|| lock(&seen).is_some());
        assert!(started.elapsed() < Duration::from_secs(5));
        let error = lock(&seen).take().unwrap();
        assert!(error.to_string().contains("would wait on itself"), "{error}");
        status.finish().unwrap();
    }

    #[cfg(feature = "structured")]
    #[test]
    fn a_widget_streaming_beside_a_waiting_stream_fails_at_once() {
        use crate::output::{Format, Output, PresentationRoute};

        struct Streams(Output, Arc<Barrier>, AtomicBool, Arc<Mutex<Option<(Error, Duration)>>>);

        impl Widget for Streams {
            fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
                if !self.2.swap(true, Ordering::SeqCst) {
                    self.1.wait();
                    thread::sleep(Duration::from_millis(200));
                    let started = Instant::now();
                    if let Err(error) = self.0.stream(&1).text(|value| value).emit() {
                        *lock(&self.3) = Some((error, started.elapsed()));
                    }
                }
                out.write("x", screw::Style::PLAIN);
            }
        }

        let coordinator = StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Live);
        let output = Output::new(Format::Text)
            .with_writer(Capture::default())
            .with_route(PresentationRoute::Around(coordinator.clone()));
        let barrier = Arc::new(Barrier::new(2));
        let seen = Arc::new(Mutex::new(None));
        let status = Status::new("widget", coordinator)
            .widget(widget(Streams(
                output.clone(),
                Arc::clone(&barrier),
                AtomicBool::new(false),
                Arc::clone(&seen),
            )))
            .start();
        barrier.wait();
        let _ = output.stream(&2).text(|value| value).emit();
        wait_for(|| lock(&seen).is_some());
        let (error, elapsed) = lock(&seen).take().unwrap();
        assert!(error.to_string().contains("would wait on itself"), "{error}");
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
        status.finish().unwrap();
    }

    #[test]
    fn a_blocking_widget_makes_requests_time_out_with_an_error() {
        struct Blocks(Arc<Barrier>);

        impl Widget for Blocks {
            fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {
                self.0.wait();
                self.0.wait();
            }
        }

        let release = Arc::new(Barrier::new(2));
        let coordinator = StatusCoordinator::build(
            SharedWriter::new(Capture::default()),
            StatusMode::Live,
            WidthSource::Fixed(40),
            Duration::from_millis(100),
        );
        let status = Status::new("blocked", coordinator.clone())
            .widget(widget(Blocks(Arc::clone(&release))))
            .start();
        release.wait();

        let error = coordinator.notice(b"stuck\n").unwrap_err();
        assert!(error.to_string().contains("did not answer in time"), "{error}");
        release.wait();
        drop(status);
    }

    #[test]
    fn a_stalled_actor_bounds_set_mode_and_is_idle() {
        struct Blocks(Arc<Barrier>);

        impl Widget for Blocks {
            fn render(&self, _ctx: &RenderCtx, _out: &mut Surface) {
                self.0.wait();
                self.0.wait();
            }
        }

        let release = Arc::new(Barrier::new(2));
        let coordinator = StatusCoordinator::build(
            SharedWriter::new(Capture::default()),
            StatusMode::Live,
            WidthSource::Fixed(40),
            Duration::from_millis(100),
        );
        let status = Status::new("blocked", coordinator.clone())
            .widget(widget(Blocks(Arc::clone(&release))))
            .start();
        release.wait();

        let error = coordinator.set_mode(StatusMode::Plain).unwrap_err();
        assert!(error.to_string().contains("did not answer in time"), "{error}");
        assert!(!coordinator.is_idle());
        release.wait();
        drop(status);
    }

    #[cfg(all(feature = "interactive", feature = "structured"))]
    #[test]
    fn a_stream_write_waiting_for_a_lease_gives_up_and_is_not_made_late() {
        let capture = Capture::default();
        let coordinator = StatusCoordinator::build(
            SharedWriter::new(Capture::default()),
            StatusMode::Plain,
            WidthSource::Fixed(40),
            Duration::from_millis(100),
        );
        let guard = coordinator.prompt_guard().unwrap();

        let error = std::thread::scope(|scope| {
            scope
                .spawn(|| coordinator.write_around(SharedWriter::new(capture.clone()), b"late\n".to_vec()))
                .join()
                .unwrap()
        })
        .unwrap_err();
        assert!(error.to_string().contains("did not answer in time"), "{error}");

        drop(guard);
        coordinator.flush_transient().unwrap();
        assert!(capture.text().is_empty());
    }

    #[test]
    fn a_mode_set_while_the_actor_starts_is_replayed() {
        let capture = Capture::default();
        let coordinator = StatusCoordinator::new(SharedWriter::new(capture.clone()), StatusMode::Live);
        let status = Status::new("working", coordinator.clone()).final_message("done").start();

        coordinator.inner.publish_mode(StatusMode::Silent);
        coordinator.inner.reconcile_mode(coordinator.inner.channel(), mode_to_u8(StatusMode::Live).into());

        status.finish().unwrap();
        assert!(!capture.text().contains("done"));
    }

    #[test]
    fn a_stale_mode_update_does_not_overwrite_a_newer_one() {
        let mut actor = actor_with(WidthSource::Fixed(40));
        let silent = (1 << MODE_BITS) | u64::from(mode_to_u8(StatusMode::Silent));
        let live = (2 << MODE_BITS) | u64::from(mode_to_u8(StatusMode::Live));
        let (reply, _replies) = mpsc::channel();
        actor.apply(Command::SetMode { stamp: live, reply: reply.clone() });
        actor.apply(Command::SetMode { stamp: silent, reply });
        assert_eq!(actor.mode, StatusMode::Live);
    }

    #[test]
    fn set_mode_does_not_start_the_actor() {
        let coordinator = StatusCoordinator::new(SharedWriter::new(Capture::default()), StatusMode::Auto);
        coordinator.set_mode(StatusMode::Silent).unwrap();
        assert!(coordinator.inner.channel.get().is_none());
    }
}
