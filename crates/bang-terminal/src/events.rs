// SPDX-License-Identifier: EUPL-1.2

use alloc::collections::VecDeque;
use core::time::Duration;
use std::{
    io::{
        self,
        Read,
    },
    os::fd::{
        AsFd,
        OwnedFd,
    },
    time::Instant,
};

use bang_core::Event;
use rustix::{
    event::{
        PollFd,
        PollFlags,
        Timespec,
        poll,
    },
    io::{
        Errno,
        read,
    },
};
use screw::Viewport;

use crate::{
    Decoder,
    Signal,
    SignalPoller,
    decoder::EscapeState,
};

const DEFAULT_ESCAPE_TIMEOUT: Duration = Duration::from_millis(50);
const SOURCE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// one result of [`TerminalEvents::next_event`]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalPoll {
    /// a decoded input event, including `Event::Resize` from the resize
    /// terminal
    Event(Event),
    /// a signal caught by the signal guard
    Signal(Signal),
    /// input is exhausted and every buffered event has been delivered
    End,
}

enum WaitMode<R> {
    Sync(fn(&mut R, &mut [u8]) -> io::Result<usize>),
    Poll(OwnedFd),
}

enum InputReadiness {
    Bytes(Vec<u8>),
    End,
    Timeout,
}

/// pull source of terminal input events, a lone Escape is held back for a
/// deadline
pub struct TerminalEvents<R> {
    input:            R,
    signals:          Option<SignalPoller>,
    resize:           Option<OwnedFd>,
    clock:            fn() -> Instant,
    wait:             WaitMode<R>,
    decoder:          Decoder,
    pending:          VecDeque<TerminalPoll>,
    last_size:        Option<Viewport>,
    size_initialized: bool,
    escape_timeout:   Duration,
    escape_since:     Option<Instant>,
    ended:            bool,
}

impl<R> TerminalEvents<R> {
    /// owns a blocking reader without a worker thread, a lone Escape waits for
    /// the next byte or end, and a signal is reported once a read returns
    #[must_use]
    pub const fn blocking(input: R) -> Self
    where
        R: Read,
    {
        Self::new(input, WaitMode::Sync(R::read))
    }

    const fn new(input: R, wait: WaitMode<R>) -> Self {
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

    /// reports signals queued for `signals`, which interrupt a wait
    #[must_use]
    pub const fn signals(mut self, signals: SignalPoller) -> Self {
        self.signals = Some(signals);
        self
    }

    /// reports `Event::Resize` whenever the terminal behind `terminal` changes
    /// size, promptly on `SIGWINCH` when `signals` is set and on a 100
    /// millisecond poll otherwise
    #[must_use]
    pub fn resize_from(mut self, terminal: OwnedFd) -> Self {
        self.resize = Some(terminal);
        self.size_initialized = false;
        self
    }

    /// how long a pending Escape sequence waits for more bytes, counted from
    /// its first byte, 50 milliseconds by default
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

    /// reads the terminal size once and treats it as reported, `None` without a
    /// resize terminal
    pub fn initial_terminal_size(&mut self) -> Option<Viewport> {
        let size = self.current_size();
        self.size_initialized = true;
        self.last_size = size;
        size
    }

    fn current_size(&self) -> Option<Viewport> {
        let terminal = self.resize.as_ref()?;
        Viewport::of(terminal).ok()
    }

    /// blocks until the next event, signal or end of input
    ///
    /// # Errors
    ///
    /// fails when reading or polling the input fails
    pub fn next_event(&mut self) -> io::Result<TerminalPoll> {
        loop {
            if let Some(item) = self.next_event_within(None)? {
                return Ok(item);
            }
        }
    }

    /// like [`Self::next_event`], gives up after `limit` and returns `None`, a
    /// blocking source ignores `limit`
    ///
    /// # Errors
    ///
    /// fails when reading or polling the input fails
    pub fn next_event_within(
        &mut self,
        limit: Option<Duration>,
    ) -> io::Result<Option<TerminalPoll>> {
        if let Some(item) = self.pending.pop_front() {
            return Ok(Some(item));
        }
        if self.ended {
            return Ok(Some(TerminalPoll::End));
        }
        if let Some(item) = self.poll_sideband()? {
            return Ok(Some(item));
        }
        let deadline = limit.map(|span| (self.clock)() + span);

        loop {
            let remaining = match deadline {
                Some(due) => {
                    let remaining = due.saturating_duration_since((self.clock)());
                    if remaining.is_zero() {
                        return Ok(None);
                    }
                    Some(remaining)
                },
                None => None,
            };
            let input = match self.wait_for_input(remaining)? {
                InputReadiness::Timeout => {
                    if let Some(event) = self.flush_due_escape() {
                        return Ok(Some(TerminalPoll::Event(event)));
                    }
                    if let Some(item) = self.poll_sideband()? {
                        return Ok(Some(item));
                    }
                    continue;
                },
                InputReadiness::Bytes(chunk) => Some(chunk),
                InputReadiness::End => None,
            };
            if let Some(chunk) = input {
                let decoded = self.decoder.feed(&chunk);
                self.queue_events(decoded);
                // only arm the deadline for a prefix `flush_escape` can
                // resolve, an incomplete CSI has to wait for
                // its final byte regardless
                if self.decoder.escape_state() == EscapeState::Ambiguous
                    && !matches!(self.wait, WaitMode::Sync(_))
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
        let timeout = cap.map_or_else(
            || self.wait_timeout(),
            |limit| self.wait_timeout().min(limit),
        );
        match &self.wait {
            WaitMode::Sync(pull) => {
                let mut buffer = [0_u8; 256];
                match pull(&mut self.input, &mut buffer) {
                    Ok(0) => Ok(InputReadiness::End),
                    Ok(read) => Ok(InputReadiness::Bytes(buffer[..read].to_vec())),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        Ok(InputReadiness::Timeout)
                    },
                    Err(error) => Err(error),
                }
            },
            WaitMode::Poll(fd) => {
                read_when_ready(fd, self.signals.map(SignalPoller::read_fd), timeout)
            },
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
            if let Some(current) = size {
                return Ok(Some(TerminalPoll::Event(resize_event(current))));
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
        let event = self.decoder.flush_escape()?;
        while let Some(next) = self.decoder.flush_escape() {
            self.pending.push_back(TerminalPoll::Event(next));
        }
        Some(event)
    }

    fn wait_timeout(&self) -> Duration {
        self.escape_since.map_or(SOURCE_POLL_INTERVAL, |since| {
            self.escape_timeout
                .saturating_sub((self.clock)().saturating_duration_since(since))
                .min(SOURCE_POLL_INTERVAL)
        })
    }
}

impl<R> TerminalEvents<R>
where
    R: AsFd,
{
    /// polls a descriptor for input and Escape deadlines without a worker
    ///
    /// # Errors
    ///
    /// fails when the descriptor is not open
    pub fn pollable(input: R) -> io::Result<Self> {
        let fd = input.as_fd().try_clone_to_owned()?;
        Ok(Self::new(input, WaitMode::Poll(fd)))
    }
}

/// the resize event reporting `size`, as [`TerminalEvents`] emits it
pub fn resize_event(size: Viewport) -> Event {
    Event::Resize {
        cols: u16::try_from(size.columns).unwrap_or(u16::MAX),
        rows: u16::try_from(size.rows).unwrap_or(u16::MAX),
    }
}

// reads the fd `poll` just reported ready, bypassing any userspace buffer a
// generic `R: Read` might hold bytes in that `poll` cannot see, takes one byte
// at a time, including inside a paste, so typeahead behind a key or a paste end
// that finishes the session stays in the kernel queue for whoever reads the
// terminal next
fn read_when_ready(
    fd: &OwnedFd,
    wake: Option<&OwnedFd>,
    timeout: Duration,
) -> io::Result<InputReadiness> {
    if !wait_for_fd(fd, wake, timeout)? {
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

fn wait_for_fd(fd: &OwnedFd, wake: Option<&OwnedFd>, timeout: Duration) -> io::Result<bool> {
    let mut descriptors = [
        PollFd::new(fd, PollFlags::IN),
        PollFd::new(wake.unwrap_or(fd), PollFlags::IN),
    ];
    let watched = if wake.is_some() { 2 } else { 1 };
    let spec = Timespec {
        tv_sec:  i64::try_from(timeout.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: timeout.subsec_nanos().into(),
    };
    match poll(&mut descriptors[..watched], Some(&spec)) {
        Ok(0) | Err(Errno::INTR) => return Ok(false),
        Ok(_) => {},
        Err(errno) => return Err(errno.into()),
    }
    let ready = descriptors[0].revents();
    if ready.contains(PollFlags::NVAL) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "input descriptor is invalid",
        ));
    }
    Ok(ready.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR))
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;
    use core::sync::atomic::{
        AtomicBool,
        Ordering,
    };
    use std::{
        fs::File,
        io::{
            BufReader,
            Cursor,
            Write as _,
        },
        os::{
            fd::BorrowedFd,
            unix::net::UnixStream,
        },
        sync::{
            OnceLock,
            mpsc,
        },
        thread,
    };

    use bang_core::Key;

    use super::*;
    use crate::{
        SignalGuard,
        testing::{
            SIGNAL_LOCK,
            pty,
            resize_pty,
        },
    };

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
        let (master, slave) = pty(80, 24).unwrap();
        let mut events = TerminalEvents::blocking(Cursor::new(b"x")).resize_from(slave);
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::Resize { cols: 80, rows: 24 }),
        );
        resize_pty(&master, 100, 30).unwrap();
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::Resize {
                cols: 100,
                rows: 30,
            }),
        );
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('x'))
        );
    }

    #[test]
    fn signals_are_sideband_outcomes_not_input_events() {
        let _serial = SIGNAL_LOCK.lock().unwrap();
        let guard = SignalGuard::install_terminal_handlers().unwrap();
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
            static BASE: OnceLock<Instant> = OnceLock::new();
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
        let (master, slave) = pty(80, 24).unwrap();
        let mut events = TerminalEvents::pollable(File::from(slave)).unwrap();
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

        // the peer is intentionally still open, resolving Escape did not depend
        // on EOF or a termios zero-byte timeout
        drop(writer);
    }

    #[test]
    fn an_arrow_key_split_by_a_gap_under_the_default_deadline_is_still_up() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let handle = thread::spawn(move || {
            writer.write_all(b"\x1b").unwrap();
            thread::sleep(Duration::from_millis(40));
            writer.write_all(b"[A").unwrap();
            writer
        });
        let mut events = TerminalEvents::pollable(reader).unwrap();
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::key(Key::Up)),
        );
        drop(handle.join().unwrap());
    }

    #[test]
    fn sigwinch_reports_a_resize_without_waiting_for_the_poll_tick() {
        let _serial = SIGNAL_LOCK.lock().unwrap();
        let guard = SignalGuard::install_terminal_handlers().unwrap();
        let (master, slave) = pty(80, 24).unwrap();
        let (_peer, reader) = UnixStream::pair().unwrap();
        let mut events = TerminalEvents::pollable(reader)
            .unwrap()
            .signals(guard.poller())
            .resize_from(slave);
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::Resize { cols: 80, rows: 24 }),
        );

        let (sent_tx, sent_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            resize_pty(&master, 100, 30).unwrap();
            sent_tx.send(Instant::now()).unwrap();
            // SAFETY: raising a signal that has a handler
            assert_eq!(unsafe { libc::raise(libc::SIGWINCH) }, 0);
            master
        });
        let polled = events.next_event().unwrap();
        let arrived = Instant::now();
        let sent = sent_rx.recv().unwrap();
        assert_eq!(
            polled,
            TerminalPoll::Event(Event::Resize {
                cols: 100,
                rows: 30,
            }),
        );
        assert!(arrived.duration_since(sent) < Duration::from_millis(40));
        drop(handle.join().unwrap());
        guard.restore().unwrap();
    }

    #[test]
    fn a_pollable_owned_fd_needs_no_read_impl() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"a").unwrap();
        let mut events = TerminalEvents::pollable(OwnedFd::from(reader)).unwrap();
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('a')),
        );
    }

    #[test]
    fn pollable_source_times_each_of_two_escapes() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"\x1b\x1b").unwrap();
        let mut events = TerminalEvents::pollable(reader)
            .unwrap()
            .escape_timeout(Duration::ZERO);
        for _ in 0..2 {
            assert_eq!(
                events.next_event().unwrap(),
                TerminalPoll::Event(Event::key(Key::Esc)),
            );
        }
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

    struct Buffered(BufReader<UnixStream>);

    impl Read for Buffered {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl AsFd for Buffered {
        fn as_fd(&self) -> BorrowedFd<'_> {
            self.0.get_ref().as_fd()
        }
    }

    #[test]
    fn pollable_source_delivers_a_burst_larger_than_one_read_without_the_next_byte() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        let alphabet = b"abcdefghijklmnopqrstuvwxyz";
        let burst: Vec<u8> = (0..300)
            .map(|index| alphabet[index % alphabet.len()])
            .collect();
        let burst_for_writer = burst.clone();
        let trailing_sent = Arc::new(AtomicBool::new(false));
        let writer_sent = Arc::clone(&trailing_sent);
        let (burst_read, wait_for_burst) = mpsc::channel::<()>();
        let handle = thread::spawn(move || {
            writer.write_all(&burst_for_writer).unwrap();
            let _outcome = wait_for_burst.recv_timeout(Duration::from_secs(5));
            writer_sent.store(true, Ordering::SeqCst);
            writer.write_all(b"z").unwrap();
        });

        let mut events = TerminalEvents::pollable(Buffered(BufReader::new(reader))).unwrap();
        for byte in burst {
            assert_eq!(
                events.next_event().unwrap(),
                TerminalPoll::Event(Event::char(byte.into())),
            );
        }
        assert!(
            !trailing_sent.load(Ordering::SeqCst),
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
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match (self.0.pop_front(), buf.first_mut()) {
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
