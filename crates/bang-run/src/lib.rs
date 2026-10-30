// SPDX-License-Identifier: EUPL-1.2

//! bang-run: command-line bridge from argv or a config file to a bang prompt

extern crate alloc;

mod config;

use core::{
    error::Error as StdError,
    fmt,
};
use std::fs;

use bang::{
    Interaction,
    advanced::{
        Headed,
        LiveSessionError,
        OutputFormat as CoreOutputFormat,
        format_output,
        replay_events,
    },
    terminal::Decoder,
};
use bang_core::{
    ActionBinding,
    ActionLayer,
    Value,
    Widget,
    widgets::{
        DatePicker,
        Form,
        MultiSelect,
        ReviewActionBinding,
        ReviewList,
        SearchSelect,
        Select,
        SelectItem,
        TextInput,
    },
};
use config::{
    FieldConfig,
    WidgetConfig,
    WidgetKind,
    parse_action_binding,
    parse_review_action_binding,
    push_unique_action,
    push_unique_review_action,
};
use pound::{
    Parse,
    ValueEnum,
};

/// failure from [`run`]
#[derive(Debug)]
pub enum CliError {
    /// invalid invocation or widget configuration
    Usage(String),
    /// the user cancelled the prompt
    Cancelled,
    /// a signal ended the prompt, carrying its number and any terminal
    /// cleanup failures reported alongside it
    Interrupted(i32, Vec<String>),
    /// terminal, input, or file failure
    Failed(String),
}

impl CliError {
    /// process exit status for this failure
    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        match self {
            Self::Usage(_) => 2,
            Self::Cancelled => 130,
            Self::Interrupted(signal, _) => 128 + *signal,
            Self::Failed(_) => 1,
        }
    }

    /// teardown failures that accompanied an interruption
    #[must_use]
    pub fn cleanup_failures(&self) -> &[String] {
        match self {
            Self::Interrupted(_, failures) => failures,
            Self::Usage(_) | Self::Cancelled | Self::Failed(_) => &[],
        }
    }
}

impl From<bang::Error> for CliError {
    fn from(error: bang::Error) -> Self {
        match error.kind() {
            bang::ErrorKind::Cancelled => Self::Cancelled,
            bang::ErrorKind::Interrupted => {
                error.signal().map_or_else(
                    || Self::Failed(error.to_string()),
                    |signal| {
                        let source = StdError::source(&error);
                        Self::Interrupted(signal.as_raw(), cleanup_failures(source))
                    },
                )
            },
            bang::ErrorKind::InvalidConfiguration => Self::Usage(error.to_string()),
            bang::ErrorKind::InputEnded
            | bang::ErrorKind::InteractionBusy
            | bang::ErrorKind::InteractionUnavailable
            | bang::ErrorKind::Terminal
            | bang::ErrorKind::UnexpectedValue
            | _ => Self::Failed(error.to_string()),
        }
    }
}

fn cleanup_failures(source: Option<&(dyn StdError + 'static)>) -> Vec<String> {
    source
        .and_then(|inner| inner.downcast_ref::<LiveSessionError>())
        .map_or(&[][..], LiveSessionError::cleanup_failures)
        .iter()
        .map(|failure| format!("terminal cleanup failed, {failure}"))
        .collect()
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Failed(message) => f.write_str(message),
            Self::Cancelled => f.write_str("cancelled"),
            Self::Interrupted(signal, _) => write!(f, "interrupted by signal {signal}"),
        }
    }
}

impl StdError for CliError {}

/// run a single bang widget, or a widget config file, as a terminal prompt
#[derive(Debug, Parse)]
#[pound(name = "bang", version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    /// path to a widget config file, instead of a subcommand, exit 1 when it
    /// cannot be read
    #[pound(short, long)]
    config:  Option<String>,
    /// result encoding
    #[pound(short, long, global, default = "text")]
    output:  OutputFormat,
    #[pound(subcommand)]
    command: Option<Command>,
}

/// bytes to replay instead of reading input
#[derive(Debug, Parse)]
pub struct Replay {
    /// bytes to replay instead of reading input, with \n \r \t \e \\ and \xHH
    /// escapes
    #[pound(long)]
    input_bytes: Option<String>,
}

/// key bindings that submit the value so far under a name
#[derive(Debug, Parse)]
pub struct Bindings {
    /// bind a key to a result as key:name, may be repeated
    ///
    /// the key is one character, ctrl- and a letter, or enter, esc, tab,
    /// backtab, backspace, delete, up, down, left, right, home, end, page-up or
    /// page-down. pressing it submits an object with `action` as the name and
    /// `value` as the value so far, and a bound character can no longer be
    /// typed
    #[pound(long)]
    action: Vec<String>,
}

#[derive(Debug, Parse)]
pub enum Command {
    /// choose one option
    Select {
        /// option label/value, may be repeated
        #[pound(long)]
        option:    Vec<String>,
        #[pound(flatten)]
        replay:    Replay,
        /// visible result rows
        #[pound(long, default = "9", min = "1")]
        page_size: usize,
        #[pound(flatten)]
        bindings:  Bindings,
    },
    /// choose zero or more options
    MultiSelect {
        /// option label/value, may be repeated
        #[pound(long)]
        option:    Vec<String>,
        #[pound(flatten)]
        replay:    Replay,
        /// visible result rows
        #[pound(long, default = "9", min = "1")]
        page_size: usize,
        #[pound(flatten)]
        bindings:  Bindings,
    },
    /// edit and submit text
    Text {
        #[pound(flatten)]
        replay:   Replay,
        /// initial value
        #[pound(long, default = "")]
        value:    String,
        /// text shown before the input, also names the prompt in errors
        #[pound(long, default = "text: ")]
        prompt:   String,
        #[pound(flatten)]
        bindings: Bindings,
    },
    /// filter options and choose one
    Search {
        /// option label/value, may be repeated
        #[pound(long)]
        option:    Vec<String>,
        #[pound(flatten)]
        replay:    Replay,
        /// visible result rows
        #[pound(long, default = "9", min = "1")]
        page_size: usize,
        #[pound(flatten)]
        bindings:  Bindings,
    },
    /// review options with confirmed/denied/unconfirmed row state
    ReviewList {
        /// option label/value, may be repeated
        #[pound(long)]
        option:        Vec<String>,
        #[pound(flatten)]
        replay:        Replay,
        /// visible result rows
        #[pound(long, default = "9", min = "1")]
        page_size:     usize,
        /// return a structured exit and rows, and bind g, s and a as the
        /// regen, search and add actions, needed by --action
        ///
        /// without it the result is the bare rows and esc cancels with exit 130
        #[pound(long)]
        action_output: bool,
        /// bind a key to a result as key:name, may be repeated, needs
        /// --action-output
        ///
        /// the key is one character the review does not use, and not g, s or
        /// a. pressing it submits the review result with `exit` set to action
        /// and `action` set to the name
        #[pound(long)]
        action:        Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

impl From<OutputFormat> for CoreOutputFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Text => Self::Text,
            OutputFormat::Json => Self::Json,
        }
    }
}

/// run the parsed command line and return the text to print
///
/// # Errors
///
/// fails with a [`CliError`] when the invocation or config is invalid, the
/// prompt is cancelled or interrupted, or the terminal or config file fails
pub fn run(cli: Cli) -> Result<String, CliError> {
    let output = cli.output;
    let result = match (cli.config, cli.command) {
        (Some(_config), Some(_command)) => {
            return Err(CliError::Usage(
                "use either --config or a widget subcommand, not both".to_owned(),
            ));
        },
        (Some(config), None) => {
            let source = fs::read_to_string(&config).map_err(|error| {
                CliError::Failed(format!("failed to read config {config}: {error}"))
            })?;
            run_config(WidgetConfig::parse(&source).map_err(CliError::Usage)?)
        },
        (None, None) => {
            return Err(CliError::Usage(
                "expected --config or a widget subcommand".to_owned(),
            ));
        },
        (None, Some(command)) => run_command(command),
    }?;

    Ok(format_output(&result, output.into()))
}

fn command_spec(command: Command) -> Result<(FieldConfig, Option<String>), String> {
    Ok(match command {
        Command::Select {
            option,
            replay,
            page_size,
            bindings,
        } => {
            (
                FieldConfig {
                    options: choice_items(option)?,
                    page_size: Some(page_size),
                    actions: action_bindings(bindings.action)?,
                    ..FieldConfig::new(WidgetKind::Select)
                },
                replay.input_bytes,
            )
        },
        Command::MultiSelect {
            option,
            replay,
            page_size,
            bindings,
        } => {
            (
                FieldConfig {
                    options: choice_items(option)?,
                    page_size: Some(page_size),
                    actions: action_bindings(bindings.action)?,
                    ..FieldConfig::new(WidgetKind::MultiSelect)
                },
                replay.input_bytes,
            )
        },
        Command::Text {
            replay,
            value,
            prompt,
            bindings,
        } => {
            (
                FieldConfig {
                    prompt: Some(prompt),
                    value: Some(value),
                    actions: action_bindings(bindings.action)?,
                    ..FieldConfig::new(WidgetKind::Text)
                },
                replay.input_bytes,
            )
        },
        Command::Search {
            option,
            replay,
            page_size,
            bindings,
        } => {
            (
                FieldConfig {
                    options: choice_items(option)?,
                    page_size: Some(page_size),
                    actions: action_bindings(bindings.action)?,
                    ..FieldConfig::new(WidgetKind::Search)
                },
                replay.input_bytes,
            )
        },
        Command::ReviewList {
            option,
            replay,
            page_size,
            action_output,
            action,
        } => {
            (
                FieldConfig {
                    options: choice_items(option)?,
                    page_size: Some(page_size),
                    action_output: Some(action_output),
                    review_actions: action
                        .iter()
                        .map(|binding| parse_review_action_binding(binding))
                        .collect::<Result<_, _>>()?,
                    ..FieldConfig::new(WidgetKind::ReviewList)
                },
                replay.input_bytes,
            )
        },
    })
}

fn run_command(command: Command) -> Result<Value, CliError> {
    let (spec, input_bytes) = command_spec(command).map_err(CliError::Usage)?;
    run_spec(spec, Vec::new(), input_bytes)
}

fn run_config(config: WidgetConfig) -> Result<Value, CliError> {
    run_spec(config.base, config.fields, config.input_bytes)
}

fn run_spec(
    spec: FieldConfig,
    fields: Vec<FieldConfig>,
    input_bytes: Option<String>,
) -> Result<Value, CliError> {
    let sink = RunSink {
        name: spec
            .prompt
            .clone()
            .unwrap_or_else(|| spec.kind.name().to_owned()),
        input_bytes,
    };
    if spec.kind == WidgetKind::Form {
        let mut form = Form::new("form");
        for field in fields {
            push_form_field(&mut form, field).map_err(CliError::Usage)?;
        }
        return headed(sink, spec.prompt, form, spec.actions);
    }
    build(spec, false, sink).map_err(CliError::Usage)?
}

trait WidgetSink {
    type Out;

    fn accept(self, widget: impl Widget + 'static, actions: Vec<ActionBinding>) -> Self::Out;
}

struct RunSink {
    name:        String,
    input_bytes: Option<String>,
}

impl WidgetSink for RunSink {
    type Out = Result<Value, CliError>;

    fn accept(self, widget: impl Widget + 'static, actions: Vec<ActionBinding>) -> Self::Out {
        run_widget(&self.name, widget, self.input_bytes, actions)
    }
}

struct FieldSink<'form> {
    form: &'form mut Form,
    name: String,
}

impl WidgetSink for FieldSink<'_> {
    type Out = ();

    fn accept(self, widget: impl Widget + 'static, actions: Vec<ActionBinding>) -> Self::Out {
        if actions.is_empty() {
            self.form.push_field(self.name, widget);
        } else {
            self.form
                .push_field(self.name, ActionLayer::new(widget).with_actions(actions));
        }
    }
}

fn push_form_field(form: &mut Form, field: FieldConfig) -> Result<(), String> {
    let sink = FieldSink {
        form,
        name: field.name.clone(),
    };
    build(field, true, sink)
}

fn headed<S: WidgetSink>(
    sink: S,
    prompt: Option<String>,
    widget: impl Widget + 'static,
    actions: Vec<ActionBinding>,
) -> S::Out {
    match prompt {
        Some(heading) => sink.accept(Headed::new(heading, widget), actions),
        None => sink.accept(widget, actions),
    }
}

fn build<S: WidgetSink>(spec: FieldConfig, in_form: bool, sink: S) -> Result<S::Out, String> {
    let id = spec.name;
    let page_size = spec.page_size.unwrap_or(9);
    let wrap = spec.wrap.unwrap_or(true);
    let first_selected = spec.selected_indices.first().copied();
    Ok(match spec.kind {
        WidgetKind::Select => {
            let mut widget = Select::new(id, spec.options)
                .with_page_size(page_size)
                .with_wrap(wrap);
            if let Some(prompt) = spec.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(selected) = first_selected {
                widget = widget.with_selected_index(selected);
            }
            sink.accept(widget, spec.actions)
        },
        WidgetKind::MultiSelect => {
            let mut widget = MultiSelect::new(id, spec.options)
                .with_page_size(page_size)
                .with_wrap(wrap)
                .with_checked_indices(spec.selected_indices);
            if let Some(prompt) = spec.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(selected) = first_selected {
                widget = widget.with_selected_index(selected);
            }
            sink.accept(widget, spec.actions)
        },
        WidgetKind::Text => {
            let prompt = spec.prompt.unwrap_or_else(|| format!("{id}: "));
            let mut widget = TextInput::new(id).with_prompt(prompt);
            if let Some(placeholder) = spec.placeholder {
                widget = widget.with_placeholder(placeholder);
            }
            if let Some(value) = spec.value {
                widget = widget.with_value(value);
            }
            sink.accept(widget, spec.actions)
        },
        WidgetKind::Search => {
            let mut widget = SearchSelect::new(id, spec.options)
                .with_page_size(page_size)
                .with_wrap(wrap);
            if let Some(prompt) = spec.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(placeholder) = spec.placeholder {
                widget = widget.with_placeholder(placeholder);
            }
            if let Some(selected) = first_selected {
                widget = widget.with_selected_match_index(selected);
            }
            sink.accept(widget, spec.actions)
        },
        WidgetKind::Date => {
            let selected = spec
                .selected_date
                .ok_or_else(|| format!("{id} requires selected_date"))?;
            let mut picker = DatePicker::new(id, selected);
            if let Some(today) = spec.today {
                picker = picker.with_today(today);
            }
            headed(sink, spec.prompt, picker, spec.actions)
        },
        WidgetKind::ReviewList => {
            let action_output = spec.action_output.unwrap_or(false);
            let mut widget = ReviewList::new(id, spec.options)
                .with_page_size(page_size)
                .with_wrap(wrap)
                .with_states(spec.review_states)
                .with_show_removed(spec.show_removed.unwrap_or(true))
                .with_exit_output(action_output)
                .with_custom_actions(review_actions(spec.review_actions, action_output)?);
            if in_form {
                widget = widget.with_leave_output(false);
            }
            if let Some(prompt) = spec.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(selected) = first_selected {
                widget = widget.with_selected_index(selected);
            }
            sink.accept(widget, Vec::new())
        },
        WidgetKind::Form => return Err("nested form fields are not supported yet".to_owned()),
    })
}

fn choice_items(options: Vec<String>) -> Result<Vec<SelectItem>, String> {
    if options.is_empty() {
        return Err("at least one --option is required".to_owned());
    }
    if options.iter().any(String::is_empty) {
        return Err("--option cannot be empty".to_owned());
    }
    Ok(options
        .into_iter()
        .map(|option| SelectItem::new(option.clone(), option))
        .collect())
}

fn action_bindings(actions: Vec<String>) -> Result<Vec<ActionBinding>, String> {
    let mut seen = Vec::new();
    let mut bindings = Vec::new();
    for action in actions {
        bindings.push(push_unique_action(
            &mut seen,
            parse_action_binding(&action)?,
        )?);
    }
    Ok(bindings)
}

fn review_actions(
    actions: Vec<ReviewActionBinding>,
    action_output: bool,
) -> Result<Vec<ReviewActionBinding>, String> {
    if !action_output {
        return if actions.is_empty() {
            Ok(Vec::new())
        } else {
            Err("review actions need action_output".to_owned())
        };
    }
    let mut bindings = vec![
        ReviewActionBinding::new('g', "regen").with_help("regenerate"),
        ReviewActionBinding::new('s', "search"),
        ReviewActionBinding::new('a', "add"),
    ];
    let defaults = bindings.len();
    let mut seen: Vec<char> = bindings.iter().map(ReviewActionBinding::key).collect();
    for action in actions {
        let key = ReviewList::fold_key(action.key());
        if let Some(bound) = bindings
            .iter()
            .take(defaults)
            .find(|default| default.key() == key)
        {
            return Err(format!(
                "duplicate review action key '{key}', action_output binds it to {}",
                bound.name()
            ));
        }
        bindings.push(push_unique_review_action(&mut seen, action)?);
    }
    Ok(bindings)
}

fn run_widget(
    name: &str,
    widget: impl Widget + 'static,
    input_bytes: Option<String>,
    actions: Vec<ActionBinding>,
) -> Result<Value, CliError> {
    if let Some(escaped) = input_bytes {
        let layered = ActionLayer::new(widget).with_actions(actions);
        let bytes = decode_escaped(&escaped).map_err(CliError::Usage)?;
        let mut decoder = Decoder::new();
        let events = decoder.feed(&bytes).into_iter().chain(decoder.flush());
        replay_events(layered, events).map_err(|error| CliError::from(error.naming_prompt(name)))
    } else {
        Interaction::live_or_tty()
            .interact(widget, actions)
            .map_err(|error| CliError::from(error.naming_prompt(name)))
    }
}

fn decode_escaped(value: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut chars = value.chars();
    while let Some(next) = chars.next() {
        if next != '\\' {
            push_utf8(&mut bytes, next);
            continue;
        }

        let Some(escaped) = chars.next() else {
            return Err("trailing backslash in --input-bytes".to_owned());
        };
        match escaped {
            'n' => bytes.push(b'\n'),
            'r' => bytes.push(b'\r'),
            't' => bytes.push(b'\t'),
            'e' => bytes.push(0x1B),
            '\\' => bytes.push(b'\\'),
            'x' => {
                let high = chars
                    .next()
                    .ok_or_else(|| "incomplete \\x escape in --input-bytes".to_owned())?;
                let low = chars
                    .next()
                    .ok_or_else(|| "incomplete \\x escape in --input-bytes".to_owned())?;
                bytes.push(hex_byte(high, low)?);
            },
            other => {
                return Err(format!("unsupported escape \\{other} in --input-bytes"));
            },
        }
    }
    Ok(bytes)
}

fn push_utf8(out: &mut Vec<u8>, value: char) {
    let mut buffer = [0; 4];
    out.extend_from_slice(value.encode_utf8(&mut buffer).as_bytes());
}

fn hex_byte(high: char, low: char) -> Result<u8, String> {
    let high_nibble = high
        .to_digit(16)
        .ok_or_else(|| format!("invalid hex digit '{high}' in --input-bytes"))?;
    let low_nibble = low
        .to_digit(16)
        .ok_or_else(|| format!("invalid hex digit '{low}' in --input-bytes"))?;
    u8::try_from((high_nibble << 4) | low_nibble)
        .map_err(|_overflow| "hex byte out of range".to_owned())
}

#[cfg(test)]
mod tests {
    use std::io;

    use bang::{
        advanced::LiveSessionError,
        terminal::{
            CleanupFailure,
            CleanupFailures,
            CleanupStage,
            Signal,
        },
    };

    use super::{
        CliError,
        cleanup_failures,
    };

    #[test]
    fn an_interruption_keeps_its_cleanup_failures() {
        let session = LiveSessionError::Cleanup {
            primary:  Some(Box::new(LiveSessionError::Signalled(Signal::TERM))),
            failures: CleanupFailures::new(vec![CleanupFailure::new(
                CleanupStage::RawMode,
                io::Error::from_raw_os_error(5),
            )]),
        };
        let failures = cleanup_failures(Some(&session));

        assert_eq!(failures.len(), 1);
        assert!(failures[0].starts_with("terminal cleanup failed, raw mode failed"));
        assert_eq!(CliError::Interrupted(15, failures).exit_code(), 143);
    }
}
