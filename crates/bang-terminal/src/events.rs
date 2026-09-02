// SPDX-License-Identifier: EUPL-1.2

use std::os::fd::{AsFd, AsRawFd as _};
use std::{
    collections::VecDeque,
    io::{self, Read},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use bang_core::Event;

use crate::{Decoder, SignalGuard, TerminalSize, terminal_size};

const DEFAULT_ESCAPE_TIMEOUT: Duration = Duration::from_millis(35);
const SOURCE_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalPoll {
    Event(Event),
    Signal(i32),
    End,
}

pub trait SignalSource {
    fn poll_signal(&mut self) -> io::Result<Option<i32>>;
}

pub trait TerminalSizeSource {
    fn terminal_size(&mut self) -> io::Result<Option<TerminalSize>>;
}

pub trait Clock {
    fn now(&self) -> Instant;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoSignals;

impl SignalSource for NoSignals {
    fn poll_signal(&mut self) -> io::Result<Option<i32>> {
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoTerminalSize;

impl TerminalSizeSource for NoTerminalSize {
    fn terminal_size(&mut self) -> io::Result<Option<TerminalSize>> {
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessTerminalSize;

impl TerminalSizeSource for ProcessTerminalSize {
    fn terminal_size(&mut self) -> io::Result<Option<TerminalSize>> {
        Ok(terminal_size())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

impl SignalSource for SignalGuard {
    fn poll_signal(&mut self) -> io::Result<Option<i32>> {
        self.poll_signal()
    }
}

impl<T> SignalSource for &mut T
where
    T: SignalSource + ?Sized,
{
    fn poll_signal(&mut self) -> io::Result<Option<i32>> {
        T::poll_signal(self)
    }
}

impl<T> TerminalSizeSource for &mut T
where
    T: TerminalSizeSource + ?Sized,
{
    fn terminal_size(&mut self) -> io::Result<Option<TerminalSize>> {
        T::terminal_size(self)
    }
}

enum WaitMode {
    Threaded(Receiver<ReadMessage>),
    Tty(libc::c_int),
}

enum ReadMessage {
    Bytes(Vec<u8>),
    End,
    Error(io::Error),
}

enum InputReadiness {
    Bytes(Vec<u8>),
    End,
    Timeout,
    TtyReady,
}

pub struct TerminalEvents<R, S = NoSignals, Z = NoTerminalSize, C = SystemClock> {
    input: Option<R>,
    signals: S,
    sizes: Z,
    clock: C,
    wait: WaitMode,
    decoder: Decoder,
    pending: VecDeque<TerminalPoll>,
    last_size: Option<TerminalSize>,
    size_initialized: bool,
    escape_timeout: Duration,
    escape_since: Option<Instant>,
    ended: bool,
}

impl<R> TerminalEvents<R>
where
    R: Read + Send + 'static,
{
    #[must_use]
    pub fn blocking(input: R) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || read_worker(input, &sender));
        Self::new(None, WaitMode::Threaded(receiver))
    }
}

impl<R> TerminalEvents<R>
where
    R: Read + AsFd,
{
    pub fn tty(input: R) -> io::Result<Self> {
        let fd = input.as_fd().as_raw_fd();
        // SAFETY: F_GETFL only inspects the descriptor owned by input.
        if unsafe { libc::fcntl(fd, libc::F_GETFL) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self::new(Some(input), WaitMode::Tty(fd)))
    }
}

impl<R> TerminalEvents<R> {
    const fn new(input: Option<R>, wait: WaitMode) -> Self {
        Self {
            input,
            signals: NoSignals,
            sizes: NoTerminalSize,
            clock: SystemClock,
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
}

impl<R, S, Z, C> TerminalEvents<R, S, Z, C> {
    #[must_use]
    pub fn with_signals<T>(self, signals: T) -> TerminalEvents<R, T, Z, C> {
        TerminalEvents {
            input: self.input,
            signals,
            sizes: self.sizes,
            clock: self.clock,
            wait: self.wait,
            decoder: self.decoder,
            pending: self.pending,
            last_size: self.last_size,
            size_initialized: self.size_initialized,
            escape_timeout: self.escape_timeout,
            escape_since: self.escape_since,
            ended: self.ended,
        }
    }

    #[must_use]
    pub fn with_resize_source<T>(self, sizes: T) -> TerminalEvents<R, S, T, C> {
        TerminalEvents {
            input: self.input,
            signals: self.signals,
            sizes,
            clock: self.clock,
            wait: self.wait,
            decoder: self.decoder,
            pending: self.pending,
            last_size: self.last_size,
            size_initialized: false,
            escape_timeout: self.escape_timeout,
            escape_since: self.escape_since,
            ended: self.ended,
        }
    }

    #[must_use]
    pub const fn escape_timeout(mut self, timeout: Duration) -> Self {
        self.escape_timeout = timeout;
        self
    }

    #[must_use]
    pub fn with_clock<T>(self, clock: T) -> TerminalEvents<R, S, Z, T> {
        TerminalEvents {
            input: self.input,
            signals: self.signals,
            sizes: self.sizes,
            clock,
            wait: self.wait,
            decoder: self.decoder,
            pending: self.pending,
            last_size: self.last_size,
            size_initialized: self.size_initialized,
            escape_timeout: self.escape_timeout,
            escape_since: self.escape_since,
            ended: self.ended,
        }
    }
}

impl<R, S, Z, C> TerminalEvents<R, S, Z, C>
where
    R: Read,
    S: SignalSource,
    Z: TerminalSizeSource,
    C: Clock,
{
    pub(crate) fn initial_terminal_size(&mut self) -> io::Result<Option<TerminalSize>> {
        let size = self.sizes.terminal_size()?;
        self.size_initialized = true;
        self.last_size = size;
        Ok(size)
    }

    pub fn next_event(&mut self) -> io::Result<TerminalPoll> {
        if let Some(item) = self.pending.pop_front() {
            return Ok(item);
        }
        if self.ended {
            return Ok(TerminalPoll::End);
        }
        if let Some(item) = self.poll_sideband()? {
            return Ok(item);
        }

        loop {
            let readiness = self.wait_for_input()?;
            if matches!(readiness, InputReadiness::Timeout) {
                if let Some(event) = self.flush_due_escape() {
                    return Ok(TerminalPoll::Event(event));
                }
                if let Some(item) = self.poll_sideband()? {
                    return Ok(item);
                }
                continue;
            }

            let bytes = match readiness {
                InputReadiness::Bytes(bytes) => Some(bytes),
                InputReadiness::End => None,
                InputReadiness::Timeout => continue,
                InputReadiness::TtyReady => {
                    let mut buffer = [0_u8; 256];
                    let Some(input) = self.input.as_mut() else {
                        return Err(io::Error::other("TTY event source lost its reader"));
                    };
                    let read = match input.read(&mut buffer) {
                        Ok(read) => read,
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                            if let Some(item) = self.poll_sideband()? {
                                return Ok(item);
                            }
                            continue;
                        },
                        Err(error) => return Err(error),
                    };
                    (read > 0).then(|| buffer[..read].to_vec())
                },
            };
            if let Some(bytes) = bytes {
                let decoded = self.decoder.feed(&bytes);
                self.queue_events(decoded);
                if self.decoder.escape_pending() {
                    let now = self.clock.now();
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
                return Ok(item);
            }
            if let Some(item) = self.poll_sideband()? {
                return Ok(item);
            }
        }
    }

    fn wait_for_input(&self) -> io::Result<InputReadiness> {
        match &self.wait {
            WaitMode::Threaded(receiver) => match receiver.recv_timeout(self.wait_timeout()) {
                Ok(ReadMessage::Bytes(bytes)) => Ok(InputReadiness::Bytes(bytes)),
                Ok(ReadMessage::End) | Err(RecvTimeoutError::Disconnected) => {
                    Ok(InputReadiness::End)
                },
                Ok(ReadMessage::Error(error)) => Err(error),
                Err(RecvTimeoutError::Timeout) => Ok(InputReadiness::Timeout),
            },
            WaitMode::Tty(fd) => wait_for_fd(*fd, self.wait_timeout()).map(|ready| {
                if ready {
                    InputReadiness::TtyReady
                } else {
                    InputReadiness::Timeout
                }
            }),
        }
    }

    fn poll_sideband(&mut self) -> io::Result<Option<TerminalPoll>> {
        if let Some(signal) = self.signals.poll_signal()? {
            return Ok(Some(TerminalPoll::Signal(signal)));
        }
        let size = self.sizes.terminal_size()?;
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
        if self.clock.now().saturating_duration_since(since) < self.escape_timeout {
            return None;
        }
        self.escape_since = None;
        self.decoder.flush_escape()
    }

    fn wait_timeout(&self) -> Duration {
        self.escape_since.map_or(SOURCE_POLL_INTERVAL, |since| {
            self.escape_timeout
                .saturating_sub(self.clock.now().saturating_duration_since(since))
                .min(SOURCE_POLL_INTERVAL)
        })
    }
}

fn read_worker(mut input: impl Read, sender: &mpsc::Sender<ReadMessage>) {
    loop {
        let mut buffer = vec![0_u8; 256];
        match input.read(&mut buffer) {
            Ok(0) => {
                let _sent = sender.send(ReadMessage::End);
                return;
            },
            Ok(read) => {
                buffer.truncate(read);
                if sender.send(ReadMessage::Bytes(buffer)).is_err() {
                    return;
                }
            },
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
            Err(error) => {
                let _sent = sender.send(ReadMessage::Error(error));
                return;
            },
        }
    }
}

const fn resize_event(size: TerminalSize) -> Event {
    Event::Resize {
        cols: size.cols,
        rows: size.rows,
    }
}

fn wait_for_fd(fd: libc::c_int, timeout: Duration) -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
    // SAFETY: descriptor points to one initialized pollfd for the duration of the call.
    let result = unsafe { libc::poll(&raw mut descriptor, 1, timeout) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::Interrupted {
            return Ok(false);
        }
        return Err(error);
    }
    if result == 0 {
        return Ok(false);
    }
    if descriptor.revents & libc::POLLNVAL != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "terminal input descriptor is invalid",
        ));
    }
    if descriptor.revents & libc::POLLERR != 0 {
        return Err(io::Error::other(
            "terminal input descriptor reported an error",
        ));
    }
    Ok(descriptor.revents & (libc::POLLIN | libc::POLLHUP) != 0)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::{io::Write as _, os::unix::net::UnixStream};

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

    struct Sizes(VecDeque<Option<TerminalSize>>);

    impl TerminalSizeSource for Sizes {
        fn terminal_size(&mut self) -> io::Result<Option<TerminalSize>> {
            Ok(self.0.pop_front().flatten())
        }
    }

    #[test]
    fn initial_and_changed_sizes_are_emitted_without_input_policy() {
        let sizes = Sizes(VecDeque::from([
            Some(TerminalSize { cols: 80, rows: 24 }),
            Some(TerminalSize {
                cols: 100,
                rows: 30,
            }),
            Some(TerminalSize {
                cols: 100,
                rows: 30,
            }),
        ]));
        let mut events = TerminalEvents::blocking(Cursor::new(b"x")).with_resize_source(sizes);
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::Resize { cols: 80, rows: 24 }),
        );
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

    struct Signals(Option<i32>);

    impl SignalSource for Signals {
        fn poll_signal(&mut self) -> io::Result<Option<i32>> {
            Ok(self.0.take())
        }
    }

    #[test]
    fn signals_are_sideband_outcomes_not_input_events() {
        let mut events =
            TerminalEvents::blocking(Cursor::new(b"x")).with_signals(Signals(Some(15)));
        assert_eq!(events.next_event().unwrap(), TerminalPoll::Signal(15));
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::char('x')),
        );
    }

    #[test]
    fn tty_poll_distinguishes_escape_deadline_from_hangup() {
        struct FixedClock(Instant);

        impl Clock for FixedClock {
            fn now(&self) -> Instant {
                self.0
            }
        }

        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"\x1b").unwrap();
        let mut events = TerminalEvents::tty(reader)
            .unwrap()
            .with_clock(FixedClock(Instant::now()))
            .escape_timeout(Duration::ZERO);
        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::key(Key::Esc)),
        );
        drop(writer);
        assert_eq!(events.next_event().unwrap(), TerminalPoll::End);
    }

    #[test]
    fn blocking_source_honours_escape_deadline_while_reader_remains_open() {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(b"\x1b").unwrap();
        let mut events = TerminalEvents::blocking(reader).escape_timeout(Duration::ZERO);

        assert_eq!(
            events.next_event().unwrap(),
            TerminalPoll::Event(Event::key(Key::Esc)),
        );

        // The peer is intentionally still open: resolving Escape did not
        // depend on EOF or a termios zero-byte timeout.
        drop(writer);
    }
}
