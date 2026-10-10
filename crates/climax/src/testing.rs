// SPDX-License-Identifier: EUPL-1.2

//! testing: runs an application in-process with scripted input and captured
//! output.
//!
//! [`run`] and [`run_with`] do what `climax::main` and `climax::main_with` do
//! without touching the process. prompts read from a [`Script`], results and
//! diagnostics land in [`Capture`] buffers, and the exit code and error come
//! back in an [`Outcome`], reported and mapped by the code `main` uses.
//!
//! [`Capture`] needs no feature. [`Script`] and [`run_with`] need
//! `interactive`, and [`run`] needs `interactive` and `parse`. Leftover script
//! input fails the run, see [`Script`].
//!
//! ```
//! use climax::{
//!     prelude::*,
//!     testing::{
//!         self,
//!         Script,
//!     },
//! };
//!
//! let outcome = testing::run_with(Script::new().confirm(false), |cx| {
//!     if cx.confirm("Deploy to prod?").interact()?.unwrap_or(false) {
//!         return Ok(());
//!     }
//!     Err(Error::message("not deploying").with_exit_code(3))
//! });
//! assert_eq!(outcome.exit_code, 3);
//! assert_eq!(outcome.stderr, "error: not deploying\n");
//! ```

use std::{
    io,
    sync::{
        Arc,
        Mutex,
        PoisonError,
    },
};

#[cfg(feature = "interactive")]
use crate::{
    Context,
    Result,
    app::{
        CompletionStream,
        execute,
        finish,
    },
};

/// cloneable in-memory `Write + Send + 'static` sink
///
/// give a clone to `Context::with_output_writer` or `with_diagnostic_writer`
/// and read the original, while [`run`] and [`run_with`] capture for you
#[derive(Clone, Debug, Default)]
pub struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    /// everything written so far, with invalid UTF-8 replaced
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes()).into_owned()
    }

    /// raw bytes written so far
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// what a test run produced
///
/// `stdout` holds registered results and, for help and version, the text
/// `main` prints on stdout. `stderr` holds diagnostics, notices, statuses and
/// the `error: ...` line `main` prints.
///
/// ```
/// use climax::testing::{
///     self,
///     Script,
/// };
///
/// let outcome = testing::run_with(Script::new(), |_cx| Ok(()));
/// assert_eq!(outcome.exit_code, 0);
/// assert!(outcome.error.is_none());
/// assert!(outcome.stdout.is_empty() && outcome.stderr.is_empty());
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub struct Outcome {
    /// exit code `main` would return
    pub exit_code: u8,
    /// everything written to the output stream
    pub stdout:    String,
    /// everything written to the diagnostic stream, plus the reported error
    pub stderr:    String,
    /// error the application or the argument parser returned
    pub error:     Option<crate::Error>,
}

/// scripted answers for the prompts of one run
///
/// every method answers exactly one prompt, in the order the application shows
/// them. the run panics with the leftover count when the script answers more
/// prompts than were shown or leaves part of an answer unread, and a prompt
/// shown after the script ran out fails with `ErrorKind::InputEnded`
///
/// ```
/// use climax::testing::Script;
///
/// let script = Script::new()
///     .select_nth(1)
///     .multi_select_nth([0, 2])
///     .text("eu-west")
///     .confirm(true)
///     .esc();
/// # drop(script);
/// ```
#[cfg(feature = "interactive")]
#[derive(Clone, Debug, Default)]
pub struct Script(Vec<Vec<bang::advanced::Event>>);

#[cfg(feature = "interactive")]
impl Script {
    /// empty script, for an application that shows no prompt
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer a select or search prompt by picking row `n` and submitting.
    ///
    /// The script presses Home first, so the row is absolute whatever the
    /// prompt preselected. A search prompt starts on its first row already.
    ///
    /// presses Ctrl-Home first, so the row is absolute whatever the prompt
    /// preselected, and a search prompt keeps plain Home for its query
    /// cursor
    #[must_use]
    pub fn select_nth(self, n: usize) -> Self {
        let mut keys = vec![bang::advanced::Key::Home];
        keys.extend(vec![bang::advanced::Key::Down; n]);
        keys.push(bang::advanced::Key::Enter);
        self.keys(keys)
    }

    /// answers a multi-select prompt by checking exactly the rows at `indices`,
    /// in any order, and submitting
    ///
    /// unchecks every row and moves to the first row, so preselected rows do
    /// not leak in
    #[must_use]
    pub fn multi_select_nth(self, indices: impl IntoIterator<Item = usize>) -> Self {
        let mut indices: Vec<usize> = indices.into_iter().collect();
        indices.sort_unstable();
        indices.dedup();
        let mut keys = vec![bang::advanced::Key::Home, bang::advanced::Key::Char('n')];
        let mut row = 0;
        for index in indices {
            keys.extend(vec![bang::advanced::Key::Down; index - row]);
            keys.push(bang::advanced::Key::Char(' '));
            row = index;
        }
        keys.push(bang::advanced::Key::Enter);
        self.keys(keys)
    }

    /// answers a text, password or number prompt by typing `text` and
    /// submitting
    ///
    /// a date prompt takes no typed text, use [`Script::date`]
    #[must_use]
    pub fn text(self, text: &str) -> Self {
        self.text_attempts([text])
    }

    /// answers a date prompt that opens on `from` by moving to `to` and
    /// submitting
    ///
    /// no key jumps to a fixed date, so `from` must be the date the prompt
    /// opens on, its default or today, and a wrong `from` submits a shifted
    /// date
    #[must_use]
    pub fn date(self, from: bang::Date, to: bang::Date) -> Self {
        use bang::advanced::{
            Event,
            Key,
            KeyEvent,
            Modifiers,
        };

        let months = (i64::from(to.year) - i64::from(from.year)) * 12 + i64::from(to.month)
            - i64::from(from.month);
        let page = || {
            if months < 0 {
                Key::PageUp
            } else {
                Key::PageDown
            }
        };
        let years = (0..months.unsigned_abs() / 12)
            .map(|_| Event::Key(KeyEvent::with_modifiers(page(), Modifiers::SHIFT)));
        let rest = (0..months.unsigned_abs() % 12).map(|_| Event::Key(KeyEvent::new(page())));
        let days = (1..to.day).map(|_| Event::Key(KeyEvent::new(Key::Right)));
        let mut events: Vec<Event> = years.chain(rest).collect();
        events.push(Event::Key(KeyEvent::new(Key::Home)));
        events.extend(days);
        events.push(Event::Key(KeyEvent::new(Key::Enter)));
        let mut script = self;
        script.0.push(events);
        script
    }

    /// Answer one text prompt whose validator rejects the earlier attempts, by
    /// typing and submitting each attempt in turn.
    ///
    /// The prompt keeps a rejected input, so the script erases what it typed
    /// before the next attempt.
    ///
    /// the prompt keeps a rejected input, so one Backspace per grapheme cluster
    /// erases it before the next attempt
    #[must_use]
    pub fn text_attempts<'a>(self, attempts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut keys = Vec::new();
        let mut typed = 0;
        for attempt in attempts {
            if typed > 0 {
                keys.push(bang::advanced::Key::End);
                keys.extend(vec![bang::advanced::Key::Backspace; typed]);
            }
            typed = attempt.chars().count();
            keys.extend(attempt.chars().map(bang::advanced::Key::Char));
            keys.push(bang::advanced::Key::Enter);
        }
        self.keys(keys)
    }

    /// answers any prompt by submitting its current value, such as a default
    #[must_use]
    pub fn enter(self) -> Self {
        self.keys([bang::advanced::Key::Enter])
    }

    /// leaves a prompt with Esc, resolving to `PromptOutcome::Leave`
    #[must_use]
    pub fn esc(self) -> Self {
        self.keys([bang::advanced::Key::Esc])
    }

    /// answers a confirm prompt with `y` or `n`, which it takes without Enter
    #[must_use]
    pub fn confirm(self, answer: bool) -> Self {
        self.keys([bang::advanced::Key::Char(if answer { 'y' } else { 'n' })])
    }

    /// answers one prompt with exactly these key presses, for anything the
    /// other methods miss
    #[must_use]
    pub fn keys(mut self, keys: impl IntoIterator<Item = bang::advanced::Key>) -> Self {
        self.0.push(
            keys.into_iter()
                .map(|key| bang::advanced::Event::Key(bang::advanced::KeyEvent::new(key)))
                .collect(),
        );
        self
    }

    /// interaction driver that replays this script, for
    /// `Context::with_interaction`
    #[must_use]
    pub fn into_interaction(self) -> bang::Interaction {
        bang::advanced::scripted_interaction(self.0)
    }
}

#[cfg(feature = "interactive")]
fn context(script: Script, stdout: &Capture, stderr: &Capture) -> Context {
    let context = Context::new()
        .with_interaction(script.into_interaction())
        .with_terminal_capabilities(crate::terminal::TerminalCapabilities::new(
            false, false, false,
        ))
        .expect("capabilities apply to an idle context");
    #[cfg(feature = "render")]
    let context = context
        .with_transient_writer(stderr.clone())
        .expect("the transient writer applies to an idle context");
    context
        .with_output_writer(stdout.clone())
        .with_diagnostic_writer(stderr.clone())
}

#[cfg(feature = "interactive")]
fn outcome(
    stdout: &Capture,
    stderr: &Capture,
    completion: crate::app::Completion,
    error: Option<crate::Error>,
) -> Outcome {
    let (exit_code, report) = completion.into_parts();
    #[cfg_attr(not(feature = "parse"), expect(unused_mut))]
    let mut stdout = stdout.text();
    let mut stderr = stderr.text();
    if let Some((stream, message)) = report {
        let target = match stream {
            #[cfg(feature = "parse")]
            CompletionStream::Stdout => &mut stdout,
            CompletionStream::Stderr => &mut stderr,
        };
        target.push_str(&message);
        target.push('\n');
    }
    Outcome {
        exit_code,
        stdout,
        stderr,
        error,
    }
}

#[cfg(feature = "interactive")]
fn guarded<T>(run: impl FnOnce() -> T) -> T {
    let payload = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(value) => return value,
        Err(payload) => payload,
    };
    let text = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied());
    match text.and_then(|text| text.strip_prefix("scripted interaction dropped with ")) {
        Some(detail) => panic!("the testing script was not fully consumed, it had {detail}"),
        None => std::panic::resume_unwind(payload),
    }
}

/// runs an application without arguments like `climax::main_with`, with
/// `script` answering its prompts
///
/// ```
/// use climax::{
///     prelude::*,
///     testing::{
///         self,
///         Script,
///     },
/// };
///
/// let outcome = testing::run_with(Script::new().text("eu"), |cx| {
///     let PromptOutcome::Submit(region) = cx.text("Region").interact()? else {
///         return Err(Error::cancelled());
///     };
///     cx.diagnostic().notice(format!("deploying to {region}"))
/// });
/// assert_eq!(outcome.exit_code, 0);
/// assert!(outcome.stderr.contains("deploying to eu"));
/// ```
#[cfg(feature = "interactive")]
pub fn run_with<F>(script: Script, f: F) -> Outcome
where
    F: FnOnce(Context) -> Result<()>,
{
    let stdout = Capture::default();
    let stderr = Capture::default();
    let result = guarded(|| execute(context(script, &stdout, &stderr), (), |cx, ()| f(cx)));
    let completion = finish(&result);
    outcome(&stdout, &stderr, completion, result.err())
}

/// parses `args` and runs an application like `climax::main`, with `script`
/// answering its prompts
///
/// `args` excludes the program name, as in `try_run_from`, and help, version
/// and parse failures come back with the exit code and text `main` would use
///
/// ```
/// # #[cfg(feature = "derive")]
/// # {
/// use climax::{
///     prelude::*,
///     testing::{
///         self,
///         Script,
///     },
/// };
///
/// /// greet someone
/// #[derive(Parse)]
/// struct Args {
///     /// who to greet
///     #[pound(long)]
///     name: String,
/// }
///
/// let outcome = testing::run(["--name", "ada"], Script::new(), |cx, args: Args| {
///     cx.diagnostic().notice(format!("hello {}", args.name))
/// });
/// assert_eq!(outcome.exit_code, 0);
/// assert!(outcome.stderr.contains("hello ada"));
///
/// let outcome = testing::run(["--wat"], Script::new(), |_cx, _args: Args| Ok(()));
/// assert_eq!(outcome.exit_code, 2);
/// # }
/// ```
#[cfg(all(feature = "interactive", feature = "parse"))]
pub fn run<C, F, A>(args: A, script: Script, f: F) -> Outcome
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
    A: IntoIterator,
    A::Item: AsRef<str>,
{
    let args: Vec<A::Item> = args.into_iter().collect();
    let stdout = Capture::default();
    let stderr = Capture::default();
    let (result, completion) = guarded(|| {
        match C::try_parse_from(args.iter().map(AsRef::as_ref)) {
            Ok(command) => {
                let result = execute(context(script, &stdout, &stderr), command, f);
                let completion = finish(&result);
                (result, completion)
            },
            Err(error) => {
                drop(script.into_interaction());
                let completion = crate::app::parse_completion(&error);
                (Err(crate::Error::from(error)), completion)
            },
        }
    });
    outcome(&stdout, &stderr, completion, result.err())
}
