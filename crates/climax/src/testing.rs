// SPDX-License-Identifier: EUPL-1.2

//! testing: runs an application in-process with scripted input and captured
//! output
//!
//! `run` and `run_with` do what `climax::main` and `climax::main_with` do
//! without touching the process. prompts read from a `Script`, results and
//! diagnostics land in [`Capture`] buffers, and the exit code and error come
//! back in an [`Outcome`], reported and mapped by the code `main` uses
//!
//! `run` needs `parse` and `Script` needs `interactive` for anything but
//! `Script::new`. leftover script input panics, see `Script`

use alloc::sync::Arc;
#[cfg(feature = "interactive")] use core::iter;
use std::{
    io,
    sync::Mutex,
};

#[cfg(feature = "interactive")]
use bang::advanced::{
    Event,
    Key,
    KeyEvent,
    ScriptCheck,
    checked_scripted_interaction,
    scripted_interaction,
};

#[cfg(feature = "parse")] use crate::app::parse_completion;
use crate::{
    Context,
    Result,
    app::{
        Completion,
        CompletionStream,
        execute,
        finish,
    },
    sync::lock,
    terminal::TerminalCapabilities,
};

/// cloneable in-memory `Write + Send + 'static` sink
///
/// give a clone to `Context::with_output_writer` or `with_diagnostic_writer`
/// and read the original, while `run` and `run_with` capture for you
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
        lock(&self.0).clone()
    }
}

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        lock(&self.0).extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// what a test run produced
///
/// `stdout` holds registered results and the help and version text `main`
/// prints, `stderr` holds diagnostics, notices, statuses and the `error: ...`
/// line
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
/// without `interactive` only `Script::new` exists
///
/// ```
/// # #[cfg(feature = "interactive")]
/// # {
/// use climax::testing::Script;
///
/// let script = Script::new()
///     .select_nth(1)
///     .multi_select_nth([0, 2])
///     .text("eu-west")
///     .confirm(true)
///     .esc();
/// # drop(script);
/// # }
/// ```
#[derive(Clone, Debug, Default)]
pub struct Script(#[cfg(feature = "interactive")] Vec<Vec<Event>>);

impl Script {
    /// empty script, for an application that shows no prompt
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// answers a select or search prompt by picking row `n` and submitting
    ///
    /// the row is absolute whatever the prompt preselected, and a row past the
    /// last choice fails the prompt with `ErrorKind::Interactive`
    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn select_nth(self, n: usize) -> Self {
        let mut script = self;
        script
            .0
            .push(vec![Event::Row(n), Event::Key(KeyEvent::new(Key::Enter))]);
        script
    }

    /// answers a multi-select prompt by checking exactly the rows at `indices`,
    /// in any order, and submitting
    ///
    /// unchecks every row first, so preselected rows do not leak in, and a row
    /// past the last choice fails the prompt with `ErrorKind::Interactive`
    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn multi_select_nth(self, indices: impl IntoIterator<Item = usize>) -> Self {
        let mut sorted: Vec<usize> = indices.into_iter().collect();
        sorted.sort_unstable();
        sorted.dedup();
        let mut events = vec![Event::Key(KeyEvent::new(Key::Char('n')))];
        for index in sorted {
            events.push(Event::Row(index));
            events.push(Event::Key(KeyEvent::new(Key::Char(' '))));
        }
        events.push(Event::Key(KeyEvent::new(Key::Enter)));
        let mut script = self;
        script.0.push(events);
        script
    }

    /// answers a text, password or number prompt by typing `text` and
    /// submitting
    ///
    /// a date prompt takes no typed text, use [`Script::date`]. control
    /// characters are typed as key presses, so `text("\n")` submits an empty
    /// answer. a prompt given a `value` keeps it and the text is typed after it
    #[cfg(feature = "interactive")]
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
    #[cfg(feature = "interactive")]
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
        let years = iter::repeat_n(
            Event::Key(KeyEvent::with_modifiers(page(), Modifiers::SHIFT)),
            usize::try_from(months.unsigned_abs() / 12).unwrap_or(usize::MAX),
        );
        let rest = iter::repeat_n(
            Event::Key(KeyEvent::new(page())),
            usize::try_from(months.unsigned_abs() % 12).unwrap_or(usize::MAX),
        );
        let days = iter::repeat_n(
            Event::Key(KeyEvent::new(Key::Right)),
            usize::from(to.day.saturating_sub(1)),
        );
        let mut events: Vec<Event> = years.chain(rest).collect();
        events.push(Event::Key(KeyEvent::new(Key::Home)));
        events.extend(days);
        events.push(Event::Key(KeyEvent::new(Key::Enter)));
        let mut script = self;
        script.0.push(events);
        script
    }

    /// answers one text prompt whose validator rejects the earlier attempts,
    /// typing and submitting each in turn
    ///
    /// the prompt keeps a rejected input, so one Backspace per grapheme cluster
    /// erases it before the next attempt, and a given `value` stays. with no
    /// attempts the prompt gets no input and fails with `ErrorKind::InputEnded`
    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn text_attempts<'a>(self, attempts: impl IntoIterator<Item = &'a str>) -> Self {
        let mut keys = Vec::new();
        let mut typed = 0;
        for attempt in attempts {
            if typed > 0 {
                keys.push(Key::End);
                keys.extend(vec![Key::Backspace; typed]);
            }
            typed = unicode_segmentation::UnicodeSegmentation::graphemes(attempt, true).count();
            keys.extend(attempt.chars().map(Key::Char));
            keys.push(Key::Enter);
        }
        self.keys(keys)
    }

    /// answers any prompt by submitting its current value, such as a default
    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn enter(self) -> Self {
        self.keys([Key::Enter])
    }

    /// leaves a prompt with Esc, resolving to `PromptOutcome::Leave`
    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn esc(self) -> Self {
        self.keys([Key::Esc])
    }

    /// answers a confirm prompt with `y` or `n`, which it takes without Enter
    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn confirm(self, answer: bool) -> Self {
        self.keys([Key::Char(if answer { 'y' } else { 'n' })])
    }

    /// answers one prompt with exactly these key presses, for anything the
    /// other methods miss
    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn keys(mut self, keys: impl IntoIterator<Item = Key>) -> Self {
        self.0.push(
            keys.into_iter()
                .map(|key| Event::Key(KeyEvent::new(key)))
                .collect(),
        );
        self
    }

    /// interaction driver that replays this script, for
    /// `Context::with_interaction`
    #[cfg(feature = "interactive")]
    #[must_use]
    pub fn into_interaction(self) -> bang::Interaction {
        scripted_interaction(self.0)
    }
}

#[cfg(feature = "interactive")]
type Check = ScriptCheck;
#[cfg(not(feature = "interactive"))]
struct Check;

#[cfg(feature = "interactive")]
fn with_script(script: Script) -> (Context, Check) {
    let (interaction, check) = checked_scripted_interaction(script.0);
    (Context::new().with_interaction(interaction), check)
}

#[cfg(not(feature = "interactive"))]
fn with_script(_script: Script) -> (Context, Check) {
    (Context::new(), Check)
}

#[cfg(feature = "interactive")]
fn leftover(check: &Check) -> Option<String> {
    let (scripts, events) = (check.unused_scripts(), check.unused_events());
    (scripts > 0 || events > 0)
        .then(|| format!("{scripts} unused scripts and {events} unused events"))
}

#[cfg(not(feature = "interactive"))]
const fn leftover(_check: &Check) -> Option<String> {
    None
}

#[expect(
    clippy::expect_used,
    reason = "a fresh context is idle, so capability and writer changes apply"
)]
fn context(script: Script, stdout: &Capture, stderr: &Capture) -> (Context, Check) {
    let (scripted, check) = with_script(script);
    let idle = scripted
        .with_terminal_capabilities(TerminalCapabilities::new(false, false, false))
        .expect("capabilities apply to an idle context");
    #[cfg(feature = "render")]
    let ready = idle
        .with_transient_writer(stderr.clone())
        .expect("the transient writer applies to an idle context");
    #[cfg(not(feature = "render"))]
    let ready = idle;
    let wired = ready
        .with_output_writer(stdout.clone())
        .with_diagnostic_writer(stderr.clone());
    (wired, check)
}

fn outcome(
    stdout: &Capture,
    stderr: &Capture,
    completion: Completion,
    error: Option<crate::Error>,
) -> Outcome {
    let (exit_code, report) = completion.into_parts();
    #[cfg_attr(
        not(feature = "parse"),
        expect(unused_mut, reason = "only parse writes help to stdout")
    )]
    let mut stdout_text = stdout.text();
    let mut stderr_text = stderr.text();
    if let Some((stream, message)) = report {
        let target = match stream {
            #[cfg(feature = "parse")]
            CompletionStream::Stdout => &mut stdout_text,
            CompletionStream::Stderr => &mut stderr_text,
        };
        target.push_str(&message);
        target.push('\n');
    }
    Outcome {
        exit_code,
        stdout: stdout_text,
        stderr: stderr_text,
        error,
    }
}

#[expect(
    clippy::panic,
    reason = "an unconsumed script fails the test that owns it"
)]
fn assert_consumed(check: &Check, failure: Option<&crate::Error>) {
    let Some(left) = leftover(check) else {
        return;
    };
    match failure {
        Some(error) => {
            panic!(
                "the testing script was not fully consumed, it had {left}, and the run failed \
                 with {error}"
            )
        },
        None => panic!("the testing script was not fully consumed, it had {left}"),
    }
}

/// runs an application without arguments like `climax::main_with`, with
/// `script` answering its prompts
///
/// ```
/// # #[cfg(feature = "interactive")]
/// # {
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
/// # }
/// ```
pub fn run_with<F>(script: Script, application: F) -> Outcome
where
    F: FnOnce(Context) -> Result<()>,
{
    let stdout = Capture::default();
    let stderr = Capture::default();
    let (cx, check) = context(script, &stdout, &stderr);
    let result = execute(cx, (), |scoped, ()| application(scoped));
    let completion = finish(&result);
    let finished = outcome(&stdout, &stderr, completion, result.err());
    assert_consumed(&check, finished.error.as_ref());
    finished
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
#[cfg(feature = "parse")]
pub fn run<C, F, A>(args: A, script: Script, application: F) -> Outcome
where
    C: pound::Parse,
    F: FnOnce(Context, C) -> Result<()>,
    A: IntoIterator,
    A::Item: AsRef<str>,
{
    let collected: Vec<A::Item> = args.into_iter().collect();
    let stdout = Capture::default();
    let stderr = Capture::default();
    let (cx, check) = context(script, &stdout, &stderr);
    let (result, completion) = match C::try_parse_from(collected.iter().map(AsRef::as_ref)) {
        Ok(command) => {
            let result = execute(cx, command, application);
            let completion = finish(&result);
            (result, completion)
        },
        Err(error) => {
            drop(cx);
            let plain = pound::Term::plain();
            let completion = parse_completion(&error, plain, plain);
            (Err(crate::Error::from(error)), completion)
        },
    };
    let finished = outcome(&stdout, &stderr, completion, result.err());
    assert_consumed(&check, finished.error.as_ref());
    finished
}
