// SPDX-License-Identifier: EUPL-1.2

use std::os::fd::{AsFd, OwnedFd};
use std::{
    collections::VecDeque,
    io::{self, Read},
    time::{Duration, Instant},
};

use bang_core::Event;
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    io::{Errno, read},
};
use screw::Viewport;

use crate::{Decoder, Signal, SignalPoller, decoder::EscapeState};

const DEFAULT_ESCAPE_TIMEOUT: Duration = Duration::from_millis(35);
const SOURCE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// One result of [`TerminalEvents::next_event`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalPoll {
    /// A decoded input event, including `Event::Resize` from the resize terminal.
    Event(Event),
    /// A signal caught by the signal guard.
    Signal(Signal),
    /// Input is exhausted and every buffered event has been delivered.
    End,
}

enum WaitMode {
    Sync,
    Poll(OwnedFd),
}

enum InputReadiness {
    Bytes(Vec<u8>),
    End,
    Timeout,
}

/// Pull source of terminal input events.
///
/// It reads bytes from `R`, decodes them and merges in signals from an
/// optional [`SignalPoller`] and resizes from an optional terminal descriptor.
/// A lone Escape is held back for a deadline so it can be told apart from the
/// start of a key sequence.
///
/// Build one with [`TerminalEvents::pollable`] for a descriptor that can be
/// polled or [`TerminalEvents::blocking`] for a plain reader, then call
/// [`TerminalEvents::next_event`] in a loop.
pub struct TerminalEvents<R> {
    input: R,
    signals: Option<SignalPoller>,
    resize: Option<OwnedFd>,
    clock: fn() -> Instant,
    wait: WaitMode,
    decoder: Decoder,
    pending: VecDeque<TerminalPoll>,
    last_size: Option<Viewport>,
    size_initialized: bool,
    escape_timeout: Duration,
    escape_since: Option<Instant>,
    ended: bool,
}

impl<R> TerminalEvents<R> {
    /// Own a blocking reader without a worker thread.
    ///
    /// A lone Escape waits for the next byte or end of input, since a plain
    /// reader cannot time out. `escape_timeout` applies only to
    /// [`Self::pollable`].
    #[must_use]
    pub const fn blocking(input: R) -> Self
    where
        R: Read,
    {
        Self::new(input, WaitMode::Sync)
    }

    const fn new(input: R, wait: WaitMode) -> Self {
        Self {
            input,
            signals: None,
            resize: None,
            clock: Instant::now,
            wait,
            decoder: Decoder::new(),
            pending: VecDeque::new(),
            last_size: None,
            size_initialized: false,
            escape_timeout: DEFAULT_ESCAPE_TIMEOUT,
            escape_since: None,
            ended: false,
        }
    }

    /// Report signals queued for `signals`, which interrupt a wait.
    ///
    /// The source only reads the queue. The [`SignalGuard`](crate::SignalGuard)
    /// that installed the handlers stays with the caller, so it can restore
    /// them after the terminal modes and see any failure.
    #[must_use]
    pub const fn signals(mut self, signals: SignalPoller) -> Self {
        self.signals = Some(signals);
        self
    }

    /// Report `Event::Resize` whenever the terminal behind `terminal` changes size.
    #[must_use]
    pub fn resize_from(mut self, terminal: OwnedFd) -> Self {
        self.resize = Some(terminal);
        self.size_initialized = false;
        self
    }

    /// How long a lone Escape waits for more bytes before it is a key press.
    /// The default is 35 milliseconds.
    #[must_use]
    pub const fn escape_timeout(mut self, timeout: Duration) -> Self {
        self.escape_timeout = timeout;
        self
    }

    #[cfg(test)]
    fn with_clock(mut self, clock: fn() -> Instant) -> Self {
        self.clock = clock;
        self
    }
}

impl<R> TerminalEvents<R>
where
    R: AsFd,
{
    /// Poll a descriptor for input and Escape deadlines without a worker.
    ///
    /// Fails when the descriptor is not open.
    pub fn pollable(input: R) -> io::Result<Self> {
        let fd = input.as_fd().try_clone_to_owned()?;
        Ok(Self::new(input, WaitMode::Poll(fd)))
    }
}

impl<R> TerminalEvents<R>
where
    R: Read,
{
    /// Read the terminal size once and treat it as already reported, so the first poll
    /// does not repeat it as a resize. `None` without a [`Self::resize_from`] terminal.
    pub fn initial_terminal_size(&mut self) -> Option<Viewport> {
        let size = self.current_size();
        self.size_initialized = true;
        self.last_size = size;
        size
    }

    fn current_size(&self) -> Option<Viewport> {
        self.resize
            .as_ref()
            .and_then(|terminal| Viewport::of(terminal).ok())
    }

    /// Block until the next event, signal or end of input.
    ///
    /// Read and poll failures are returned as errors.
    pub fn next_event(&mut self) -> io::Result<TerminalPoll> {
        loop {
            if let Some(item) = self.next_event_within(None)? {
                return Ok(item);
            }
        }
    }

    /// Like [`Self::next_event`], but give up after `limit` and return `None`.
    ///
    /// Only a [`Self::pollable`] source can time out. A [`Self::blocking`]
    /// source waits in `read` and ignores `limit`.
    pub fn next_event_within(&mut self, limit: Option<Duration>) -> io::Result<Option<TerminalPoll>> {
        if let Some(item) = self.pending.pop_front() {
            return Ok(Some(item));
        }
        if self.ended {
            return Ok(Some(TerminalPoll::End));
        }
        if let Some(item) = self.poll_sideband()? {
            return Ok(Some(item));
        }
        let deadline = limit.map(|limit| (self.clock)() + limit);

        loop {
            let remaining = match deadline {
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since((self.clock)());
                    if remaining.is_zero() {
                        return Ok(None);
                    }
                    Some(remaining)
                },
                None => None,
            };
            let bytes = match self.wait_for_input(remaining)? {
                InputReadiness::Timeout => {
                    if let Some(event) = self.flush_due_escape() {
                        return Ok(Some(TerminalPoll::Event(event)));
                    }
                    if let Some(item) = self.poll_sideband()? {
                        return Ok(Some(item));
                    }
                    continue;
                },
                InputReadiness::Bytes(bytes) => Some(bytes),
                InputReadiness::End => None,
            };
            if let Some(bytes) = bytes {
                let decoded = self.decoder.feed(&bytes);
                self.queue_events(decoded);
                // Only arm the deadline for a prefix `flush_escape` can resolve;
                // an incomplete CSI has to wait for its final byte regardless.
                if self.decoder.escape_state() == EscapeState::Ambiguous
                    && !matches!(self.wait, WaitMode::Sync)
                {
                    let now = (self.clock)();
                    self.escape_since.get_or_insert(now);
                } else {
                    self.escape_since = None;
                }
            } else {
                let decoded = self.decoder.flush();
                self.queue_events(decoded);
                self.ended = true;
                self.pending.push_back(TerminalPoll::End);
            }
            if let Some(item) = self.pending.pop_front() {
                return Ok(Some(item));
            }
            if let Some(item) = self.poll_sideband()? {
                return Ok(Some(item));
            }
        }
    }

    fn wait_for_input(&mut self, cap: Option<Duration>) -> io::Result<InputReadiness> {
        let timeout = cap.map_or_else(|| self.wait_timeout(), |cap| self.wait_timeout().min(cap));
        match &mut self.wait {
            WaitMode::Sync => {
                let mut buffer = [0_u8; 256];
                match self.input.read(&mut buffer) {
                    Ok(0) => Ok(InputReadiness::End),
                    Ok(read) => Ok(InputReadiness::Bytes(buffer[..read].to_vec())),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        Ok(InputReadiness::Timeout)
                    },
                    Err(error) => Err(error),
                }
            },
            WaitMode::Poll(fd) => read_when_ready(fd, timeout),
        }
    }

    fn poll_sideband(&mut self) -> io::Result<Option<TerminalPoll>> {
        if let Some(signals) = &self.signals
            && let Some(signal) = signals.poll_signal()?
        {
            return Ok(Some(TerminalPoll::Signal(signal)));
        }
        let size = self.current_size();
        if !self.size_initialized || size != self.last_size {
            self.size_initialized = true;
            self.last_size = size;
            if let Some(size) = size {
                return Ok(Some(TerminalPoll::Event(resize_event(size))));
            }
        }
        Ok(None)
    }

    fn queue_events(&mut self, events: Vec<Event>) {
        self.pending
            .extend(events.into_iter().map(TerminalPoll::Event));
    }

    fn flush_due_escape(&mut self) -> Option<Event> {
        let since = self.escape_since?;
        if (self.clock)().saturating_duration_since(since) < self.escape_timeout {
            return None;
        }
        self.escape_since = None;
        self.decoder.flush_escape()
    }

    fn wait_timeout(&self) -> Duration {
        self.escape_since.map_or(SOURCE_POLL_INTERVAL, |since| {
            self.escape_timeout
                .saturating_sub((self.clock)().saturating_duration_since(since))
                .min(SOURCE_POLL_INTERVAL)
        })
    }
}

/// The resize event reporting `size`, as [`TerminalEvents`] emits it.
pub fn resize_event(size: Viewport) -> Event {
    Event::Resize {
        cols: u16::try_from(size.columns).unwrap_or(u16::MAX),
        rows: u16::try_from(size.rows).unwrap_or(u16::MAX),
    }
}

// Reads the fd `poll` just reported ready, bypassing any userspace buffer a
// generic `R: Read` might hold bytes in that `poll` cannot see. It takes one
// byte at a time, including inside a paste, so typeahead behind a key or a paste
// end that finishes the session stays in the kernel queue for whoever reads the
// terminal next.
fn read_when_ready(fd: &OwnedFd, timeout: Duration) -> io::Result<InputReadiness> {
    if !wait_for_fd(fd, timeout)? {
        return Ok(InputReadiness::Timeout);
    }
    let mut buffer = [0_u8; 1];
    match read(fd, &mut buffer) {
        Ok(0) | Err(Errno::IO) => Ok(InputReadiness::End),
        Ok(read) => Ok(InputReadiness::Bytes(buffer[..read].to_vec())),
        Err(Errno::INTR) => Ok(InputReadiness::Timeout),
        Err(errno) => Err(errno.into()),
    }
}

fn wait_for_fd(fd: &OwnedFd, timeout: Duration) -> io::Result<bool> {
    let mut descriptors = [PollFd::new(fd, PollFlags::IN)];
    let timeout = Timespec {
        tv_sec: i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: timeout.subsec_nanos().into(),
    };
    match poll(&mut descriptors, Some(&timeout)) {
        Ok(0) | Err(Errno::INTR) => return Ok(false),
        Ok(_) => {},
        Err(errno) => return Err(errno.into()),
    }
    let ready = descriptors[0].revents();
    if ready.contains(PollFlags::NVAL) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal input descriptor is invalid",
        ));
    }
    Ok(ready.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::{io::Write as _, os::unix::net::UnixStream, thread};

    use bang_core::Key;

    use super::*;

    #[test]
    fn blocking_source_preserves_decoder_queue_before_end() {
        let mut events = TerminalEvents::blocking(Cursor::new(b"ab"));
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('a'))
        );
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('b'))
        );
        assert_eq!(events.next_event().unwrap(), TerminalPoll::End);
    }

    #[test]
    fn initial_and_changed_sizes_are_emitted_without_input_policy() {
        let (master, slave) = crate::testing::pty(80, 24).unwrap();
        let mut events = TerminalEvents::blocking(Cursor::new(b"x")).resize_from(slave);
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::Resize { cols: 80, rows: 24 }),
        );
        crate::testing::resize_pty(&master, 100, 30).unwrap();
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::Resize {
                cols: 100,
                rows: 30
            }),
        );
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('x'))
        );
    }

    #[test]
    fn signals_are_sideband_outcomes_not_input_events() {
        let _serial = crate::testing::SIGNAL_LOCK.lock().unwrap();
        let guard = crate::SignalGuard::install_terminal_handlers().unwrap();
        let mut events = TerminalEvents::blocking(Cursor::new(b"x")).signals(guard.poller());
        // SAFETY: raising a signal the guard has a handler for.
        assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Signal(Signal::TERM),
        );
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('x')),
        );
        guard.restore().unwrap();
    }

    #[test]
    fn pollable_poll_distinguishes_escape_deadline_from_hangup() {
        fn fixed_now() -> Instant {
            static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            *BASE.get_or_init(Instant::now)
        }

        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"\x1b").unwrap();
        let mut events = TerminalEvents::pollable(reader)
            .unwrap()
            .with_clock(fixed_now)
            .escape_timeout(Duration::ZERO);
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::key(Key::Esc)),
        );
        drop(writer);
        assert_eq!(events.next_event().unwrap(), TerminalPoll::End);
    }

    #[test]
    fn pollable_source_leaves_typeahead_behind_a_key_unread() {
        use std::io::Read as _;

        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut leftover = reader.try_clone().unwrap();
        writer.write_all(b"a\rnext").unwrap();
        let mut events = TerminalEvents::pollable(reader).unwrap();
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('a'))
        );
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::key(Key::Enter))
        );
        drop(events);
        drop(writer);

        let mut rest = String::new();
        leftover.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "next");
    }

    #[test]
    fn pollable_source_leaves_typeahead_behind_a_paste_end_unread() {
        use std::io::Read as _;

        let (reader, mut writer) = UnixStream::pair().unwrap();
        let mut leftover = reader.try_clone().unwrap();
        writer.write_all(b"\x1b[200~ab\x1b[201~next").unwrap();
        let mut events = TerminalEvents::pollable(reader).unwrap();
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::Paste("ab".into())),
        );
        drop(events);
        drop(writer);

        let mut rest = String::new();
        leftover.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "next");
    }

    #[test]
    fn a_hung_up_terminal_is_the_end_of_input() {
        let (master, slave) = crate::testing::pty(80, 24).unwrap();
        let mut events = TerminalEvents::pollable(std::fs::File::from(slave)).unwrap();
        drop(master);
        assert_eq!(events.next_event().unwrap(), TerminalPoll::End);
        assert_eq!(events.next_event().unwrap(), TerminalPoll::End);
    }

    #[test]
    fn pollable_source_times_escape_while_the_peer_stays_open() {
        use std::io::Write as _;

        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"\x1b").unwrap();
        let mut events = TerminalEvents::pollable(reader)
            .unwrap()
            .escape_timeout(Duration::ZERO);
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::key(Key::Esc)),
        );

        // The peer is intentionally still open: resolving Escape did not
        // depend on EOF or a termios zero-byte timeout.
        drop(writer);
    }

    #[test]
    fn pollable_source_does_not_flush_escape_during_a_paste() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"\x1b[200~abc\x1b").unwrap();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(80));
            writer.write_all(b"[201~x\r").unwrap();
        });

        let mut events = TerminalEvents::pollable(reader)
            .unwrap()
            .escape_timeout(Duration::from_millis(20));

        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::Paste("abc".into())),
        );
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('x')),
        );
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::key(Key::Enter)),
        );
        handle.join().unwrap();
    }

    /// A reader that owns its descriptor through an 8 KiB userspace buffer,
    /// the shape `stdin.lock()` has in production.
    struct Buffered(std::io::BufReader<UnixStream>);

    impl Read for Buffered {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.0.read(buffer)
        }
    }

    impl AsFd for Buffered {
        fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
            self.0.get_ref().as_fd()
        }
    }

    #[test]
    fn pollable_source_delivers_a_burst_larger_than_one_read_without_the_next_byte() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let alphabet = b"abcdefghijklmnopqrstuvwxyz";
        let burst: Vec<u8> = (0..300).map(|index| alphabet[index % alphabet.len()]).collect();
        let burst_for_writer = burst.clone();
        let trailing_sent = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer_sent = std::sync::Arc::clone(&trailing_sent);
        let (burst_read, wait_for_burst) = std::sync::mpsc::channel::<()>();
        let handle = thread::spawn(move || {
            writer.write_all(&burst_for_writer).unwrap();
            let _ = wait_for_burst.recv_timeout(Duration::from_secs(5));
            writer_sent.store(true, std::sync::atomic::Ordering::SeqCst);
            writer.write_all(b"z").unwrap();
        });

        let mut events =
            TerminalEvents::pollable(Buffered(std::io::BufReader::new(reader))).unwrap();
        for byte in burst {
            assert_eq!(
                events.next_event().unwrap(),
                TerminalPoll::Event(Event::char(byte.into())),
            );
        }
        assert!(
            !trailing_sent.load(std::sync::atomic::Ordering::SeqCst),
            "burst delivery waited for the trailing byte",
        );
        burst_read.send(()).unwrap();

        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('z')),
        );
        handle.join().unwrap();
    }

    struct Trickle(VecDeque<u8>);

    impl Read for Trickle {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match (self.0.pop_front(), buffer.first_mut()) {
                (Some(byte), Some(slot)) => {
                    *slot = byte;
                    Ok(1)
                },
                _ => Ok(0),
            }
        }
    }

    #[test]
    fn blocking_source_holds_escape_across_read_boundaries() {
        let mut events = TerminalEvents::blocking(Trickle(VecDeque::from(b"\x1b[A".to_vec())));
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::key(Key::Up)),
        );
        assert_eq!(events.next_event().unwrap(), TerminalPoll::End);
    }

    #[test]
    fn blocking_source_resolves_a_held_escape_at_end_of_input() {
        let mut events = TerminalEvents::blocking(Trickle(VecDeque::from(b"\x1b".to_vec())));
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::key(Key::Esc)),
        );
        assert_eq!(events.next_event().unwrap(), TerminalPoll::End);
    }
}
