// SPDX-License-Identifier: EUPL-1.2

//! Run an application in-process with scripted input and captured output.
//!
//! [`run`] and [`run_with`] do what `climax::main` and `climax::main_with` do,
//! except that nothing touches the process. Prompts read from a [`Script`],
//! results and diagnostics land in [`Capture`] buffers, and the exit code and
//! the error come back in an [`Outcome`]. Errors are reported and mapped to exit
//! codes by the code `main` uses, so a test sees exactly what a user's shell
//! would.
//!
//! [`Capture`] needs no feature. [`Script`] and [`run_with`] need `interactive`,
//! and [`run`] needs `interactive` and `parse`. Leftover script input fails the
//! run, see [`Script`].
//!
//! ```
//! # #[cfg(feature = "interactive")]
//! # {
//! use climax::{
//!     prelude::*,
//!     testing::{self, Script},
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
//! # }
//! ```

use std::{
    io,
    sync::{Arc, Mutex, PoisonError},
};

#[cfg(feature = "interactive")]
use crate::{
    Context,
    Result,
    app::{CompletionStream, execute, finish},
};

/// A cloneable in-memory sink that implements `Write + Send + 'static`.
///
/// Hand a clone to `Context::with_output_writer` or `with_diagnostic_writer`
/// and read what was written through the original. [`run`] and [`run_with`]
/// capture for you.
///
/// ```
/// use std::io::Write as _;
///
/// use climax::testing::Capture;
///
/// let capture = Capture::default();
/// let mut sink = capture.clone();
/// sink.write_all(b"hello").unwrap();
/// assert_eq!(capture.text(), "hello");
/// assert_eq!(capture.bytes(), b"hello");
/// ```
#[derive(Clone, Debug, Default)]
pub struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    /// Everything written so far, with invalid UTF-8 replaced.
    #[must_use]
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes()).into_owned()
    }

    /// The raw bytes written so far.
    #[must_use]
    pub fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
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

/// What a test run produced.
///
/// `stdout` holds registered results and, for help and version, the text
/// `main` prints on stdout. `stderr` holds diagnostics, notices, statuses and
/// the `error: ...` line `main` prints.
///
/// ```
/// # #[cfg(feature = "interactive")]
/// # {
/// use climax::testing::{self, Script};
///
/// let outcome = testing::run_with(Script::new(), |_cx| Ok(()));
/// assert_eq!(outcome.exit_code, 0);
/// assert!(outcome.error.is_none());
/// assert!(outcome.stdout.is_empty() && outcome.stderr.is_empty());
/// # }
/// ```
#[derive(Debug)]
#[non_exhaustive]
pub struct Outcome {
    /// The process exit code `main` would return.
    pub exit_code: u8,
    /// Everything written to the output stream.
    pub stdout:    String,
    /// Everything written to the diagnostic stream, plus the reported error.
    pub stderr:    String,
    /// The error the application, or the argument parser, returned.
    pub error:     Option<crate::Error>,
}

/// Scripted answers for the prompts of one run.
///
/// Every method adds the answer to exactly one prompt, so a script reads as the
/// list of prompts the application is expected to show, in order. The run fails
/// with a panic that names the leftover count when the application asked for
/// fewer prompts than the script answers, or left part of an answer unread,
/// which keeps a test from passing while scripting more than the flow used. A
/// prompt shown after the script ran out fails with `ErrorKind::InputEnded`.
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
    /// An empty script, for an application that shows no prompt.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer a select or search prompt by picking row `n` and submitting.
    ///
    /// The script presses Ctrl-Home first, so the row is absolute whatever the
    /// prompt preselected. A search prompt keeps plain Home for its query
    /// cursor, so Ctrl-Home is the key that jumps to its first match.
    ///
    /// ```
    /// # use climax::testing::Script;
    /// let script = Script::new().select_nth(2);
    /// # drop(script);
    /// ```
    #[must_use]
    pub fn select_nth(self, n: usize) -> Self {
        use bang::advanced::{Event, Key, KeyEvent, Modifiers};

        let mut events = vec![Event::Key(KeyEvent::with_modifiers(Key::Home, Modifiers::CONTROL))];
        events.extend(vec![Event::Key(KeyEvent::new(Key::Down)); n]);
        events.push(Event::Key(KeyEvent::new(Key::Enter)));
        let mut script = self;
        script.0.push(events);
        script
    }

    /// Answer a multi-select prompt by checking exactly the rows at `indices`
    /// and submitting. The indices may come in any order.
    ///
    /// The script unchecks every row and moves to the first row before it
    /// starts, so preselected rows do not leak into the answer.
    ///
    /// ```
    /// # use climax::testing::Script;
    /// let script = Script::new().multi_select_nth([0, 3]);
    /// # drop(script);
    /// ```
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

    /// Answer a text, password or number prompt by typing `text` and
    /// submitting. A date prompt takes no typed text, use [`Script::date`].
    ///
    /// ```
    /// # use climax::testing::Script;
    /// let script = Script::new().text("eu-west");
    /// # drop(script);
    /// ```
    #[must_use]
    pub fn text(self, text: &str) -> Self {
        self.text_attempts([text])
    }

    /// Answer a date prompt that opens on `from` by moving to `to` and
    /// submitting.
    ///
    /// A date prompt has no key that jumps to a fixed date, so `from` must be
    /// the date the prompt opens on, which is its default or today. A wrong
    /// `from` submits a shifted date, so pass the default the application sets.
    ///
    /// ```
    /// # use climax::testing::Script;
    /// use climax::bang::Date;
    ///
    /// let from = Date::new(2026, 10, 10).unwrap();
    /// let to = Date::new(2027, 1, 31).unwrap();
    /// let script = Script::new().date(from, to);
    /// # drop(script);
    /// ```
    #[must_use]
    pub fn date(self, from: bang::Date, to: bang::Date) -> Self {
        use bang::advanced::{Event, Key, KeyEvent, Modifiers};

        let months = (i64::from(to.year) - i64::from(from.year)) * 12 + i64::from(to.month)
            - i64::from(from.month);
        let page = || if months < 0 { Key::PageUp } else { Key::PageDown };
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
    /// before the next attempt, one Backspace per grapheme cluster.
    ///
    /// ```
    /// # use climax::testing::Script;
    /// let script = Script::new().text_attempts(["ab", "abc"]);
    /// # drop(script);
    /// ```
    #[must_use]
    pub fn text_attempts<'a>(self, attempts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut keys = Vec::new();
        let mut typed = 0;
        for attempt in attempts {
            if typed > 0 {
                keys.push(bang::advanced::Key::End);
                keys.extend(vec![bang::advanced::Key::Backspace; typed]);
            }
            typed = unicode_segmentation::UnicodeSegmentation::graphemes(attempt, true).count();
            keys.extend(attempt.chars().map(bang::advanced::Key::Char));
            keys.push(bang::advanced::Key::Enter);
        }
        self.keys(keys)
    }

    /// Answer any prompt by submitting its current value, such as a default.
    ///
    /// ```
    /// # use climax::testing::Script;
    /// let script = Script::new().enter();
    /// # drop(script);
    /// ```
    #[must_use]
    pub fn enter(self) -> Self {
        self.keys([bang::advanced::Key::Enter])
    }

    /// Leave a prompt with Esc, which resolves to `PromptOutcome::Leave`.
    ///
    /// ```
    /// # use climax::testing::Script;
    /// let script = Script::new().esc();
    /// # drop(script);
    /// ```
    #[must_use]
    pub fn esc(self) -> Self {
        self.keys([bang::advanced::Key::Esc])
    }

    /// Answer a confirm prompt with `y` or `n`, which it takes without Enter.
    ///
    /// ```
    /// # use climax::testing::Script;
    /// let script = Script::new().confirm(true);
    /// # drop(script);
    /// ```
    #[must_use]
    pub fn confirm(self, answer: bool) -> Self {
        self.keys([bang::advanced::Key::Char(if answer { 'y' } else { 'n' })])
    }

    /// Answer one prompt with exactly these key presses, for anything the
    /// other methods do not spell.
    ///
    /// ```
    /// # use climax::testing::Script;
    /// use climax::bang::advanced::Key;
    ///
    /// let script = Script::new().keys([Key::Down, Key::Char(' '), Key::Enter]);
    /// # drop(script);
    /// ```
    #[must_use]
    pub fn keys(mut self, keys: impl IntoIterator<Item = bang::advanced::Key>) -> Self {
        self.0.push(
            keys.into_iter()
                .map(|key| bang::advanced::Event::Key(bang::advanced::KeyEvent::new(key)))
                .collect(),
        );
        self
    }

    /// The interaction driver that replays this script, for
    /// `Context::with_interaction`.
    ///
    /// ```
    /// use climax::{Context, testing::Script};
    ///
    /// let context = Context::new().with_interaction(Script::new().into_interaction());
    /// # drop(context);
    /// ```
    #[must_use]
    pub fn into_interaction(self) -> bang::Interaction {
        bang::advanced::scripted_interaction(self.0)
    }
}

#[cfg(feature = "interactive")]
fn context(script: Script, stdout: &Capture, stderr: &Capture) -> Context {
    let context = Context::new()
        .with_interaction(script.into_interaction())
        .with_terminal_capabilities(crate::terminal::TerminalCapabilities::new(false, false, false))
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

/// Run an application that takes no arguments, the way `climax::main_with`
/// does, with `script` answering its prompts.
///
/// ```
/// use climax::{
///     prelude::*,
///     testing::{self, Script},
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

/// Parse `args` and run an application, the way `climax::main` does, with
/// `script` answering its prompts.
///
/// `args` excludes the program name, as in `try_run_from`. Help, version and
/// parse failures come back with the exit code and text `main` would use.
///
/// ```
/// # #[cfg(feature = "derive")]
/// # {
/// use climax::{
///     prelude::*,
///     testing::{self, Script},
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
    let (result, completion) = guarded(|| match C::try_parse_from(args.iter().map(AsRef::as_ref)) {
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
    });
    outcome(&stdout, &stderr, completion, result.err())
}
