// SPDX-License-Identifier: EUPL-1.2

//! command-line bridge

mod config;

use std::fmt;

use bang::terminal::Decoder;
use bang_core::{
    ActionBinding,
    ActionLayer,
    OutputFormat as CoreOutputFormat,
    Value,
    Widget,
    format_output,
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
    text_from_config,
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
}

impl From<bang::Error> for CliError {
    fn from(error: bang::Error) -> Self {
        match error.kind() {
            bang::ErrorKind::Cancelled => Self::Cancelled,
            bang::ErrorKind::Interrupted => error.signal().map_or_else(
                || Self::Failed(error.to_string()),
                |signal| {
                    let source = std::error::Error::source(&error);
                    Self::Interrupted(signal.as_raw(), cleanup_failures(source))
                },
            ),
            bang::ErrorKind::InvalidConfiguration => Self::Usage(error.to_string()),
            _ => Self::Failed(error.to_string()),
        }
    }
}

fn cleanup_failures(source: Option<&(dyn std::error::Error + 'static)>) -> Vec<String> {
    source
        .and_then(|source| source.downcast_ref::<bang::advanced::LiveSessionError>())
        .map_or(&[][..], bang::advanced::LiveSessionError::cleanup_failures)
        .iter()
        .map(|failure| format!("terminal cleanup failed, {failure}"))
        .collect()
}

impl CliError {
    /// teardown failures that accompanied an interruption
    #[must_use]
    pub fn cleanup_failures(&self) -> &[String] {
        match self {
            Self::Interrupted(_, failures) => failures,
            _ => &[],
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Failed(message) => formatter.write_str(message),
            Self::Cancelled => formatter.write_str("cancelled"),
            Self::Interrupted(signal, _) => write!(formatter, "interrupted by signal {signal}"),
        }
    }
}

impl std::error::Error for CliError {}

/// Run a single bang widget, or a widget config file, as a terminal prompt.
#[derive(Debug, Parse)]
#[pound(name = "bang", version = env!("CARGO_PKG_VERSION"))]
pub struct Cli {
    /// path to a widget config file, instead of a subcommand
    #[pound(short, long)]
    config:  Option<String>,
    /// result encoding
    #[pound(short, long, global, default = "text")]
    output:  OutputFormat,
    #[pound(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Parse)]
pub enum Command {
    /// choose one option
    Select {
        /// option label/value; may be repeated
        #[pound(short, long)]
        option:      Vec<String>,
        /// escaped terminal bytes for deterministic non-TTY execution
        #[pound(long)]
        input_bytes: Option<String>,
        /// visible result rows
        #[pound(long, default = "9", min = "1")]
        page_size:   usize,
        /// app action key in key:name form; may be repeated
        #[pound(long)]
        action:      Vec<String>,
    },
    /// choose zero or more options
    MultiSelect {
        /// option label/value; may be repeated
        #[pound(short, long)]
        option:      Vec<String>,
        /// escaped terminal bytes for deterministic non-TTY execution
        #[pound(long)]
        input_bytes: Option<String>,
        /// visible result rows
        #[pound(long, default = "9", min = "1")]
        page_size:   usize,
        /// app action key in key:name form; may be repeated
        #[pound(long)]
        action:      Vec<String>,
    },
    /// edit and submit text
    Text {
        /// escaped terminal bytes for deterministic non-TTY execution
        #[pound(long)]
        input_bytes: Option<String>,
        /// initial value
        #[pound(long, default = "")]
        value:       String,
        /// prompt shown in logical views
        #[pound(long, default = "text: ")]
        prompt:      String,
        /// app action key in key:name form; may be repeated
        #[pound(long)]
        action:      Vec<String>,
    },
    /// filter options and choose one
    Search {
        /// option label/value; may be repeated
        #[pound(short, long)]
        option:      Vec<String>,
        /// escaped terminal bytes for deterministic non-TTY execution
        #[pound(long)]
        input_bytes: Option<String>,
        /// visible result rows
        #[pound(long, default = "9", min = "1")]
        page_size:   usize,
        /// app action key in key:name form; may be repeated
        #[pound(long)]
        action:      Vec<String>,
    },
    /// review options with confirmed/denied/unconfirmed row state
    ReviewList {
        /// option label/value; may be repeated
        #[pound(short, long)]
        option:        Vec<String>,
        /// escaped terminal bytes for deterministic non-TTY execution
        #[pound(long)]
        input_bytes:   Option<String>,
        /// visible result rows
        #[pound(long, default = "9", min = "1")]
        page_size:     usize,
        /// hide rows whose initial review state is denied
        #[pound(long)]
        hide_removed:  bool,
        /// return a structured exit and rows; enables g/s/a action keys
        #[pound(long)]
        action_output: bool,
        /// extra action key in key:name form; may be repeated
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

pub fn run(cli: Cli) -> Result<String, CliError> {
    let output = cli.output;
    let result = match (cli.config, cli.command) {
        (Some(_config), Some(_command)) => {
            return Err(CliError::Usage("use either --config or a widget subcommand, not both".to_owned()));
        },
        (Some(config), None) => {
            let source = std::fs::read_to_string(&config).map_err(|error| {
                CliError::Failed(format!("failed to read config {config}: {error}"))
            })?;
            run_config(WidgetConfig::parse(&source).map_err(CliError::Usage)?)
        },
        (None, None) => return Err(CliError::Usage("expected --config or a widget subcommand".to_owned())),
        (None, Some(command)) => run_command(command),
    }?;

    Ok(format_output(&result, output.into()))
}

fn run_command(command: Command) -> Result<Value, CliError> {
    match command {
        Command::Select {
            option,
            input_bytes,
            page_size,
            action,
        } => {
            run_widget(
                Select::new("select", choice_items(option).map_err(CliError::Usage)?).with_page_size(page_size),
                input_bytes,
                action_bindings(action).map_err(CliError::Usage)?,
            )
        },
        Command::MultiSelect {
            option,
            input_bytes,
            page_size,
            action,
        } => {
            run_widget(
                MultiSelect::new("multi-select", choice_items(option).map_err(CliError::Usage)?)
                    .with_page_size(page_size),
                input_bytes,
                action_bindings(action).map_err(CliError::Usage)?,
            )
        },
        Command::Text {
            input_bytes,
            value,
            prompt,
            action,
        } => {
            run_widget(
                TextInput::new("text").with_prompt(prompt).with_value(value),
                input_bytes,
                action_bindings(action).map_err(CliError::Usage)?,
            )
        },
        Command::Search {
            option,
            input_bytes,
            page_size,
            action,
        } => {
            run_widget(
                SearchSelect::new("search", choice_items(option).map_err(CliError::Usage)?)
                    .with_page_size(page_size),
                input_bytes,
                action_bindings(action).map_err(CliError::Usage)?,
            )
        },
        Command::ReviewList {
            option,
            input_bytes,
            page_size,
            hide_removed,
            action_output,
            action,
        } => {
            let actions =
                review_action_bindings_with_defaults(action, action_output).map_err(CliError::Usage)?;
            run_widget(
                ReviewList::new("review-list", choice_items(option).map_err(CliError::Usage)?)
                    .with_page_size(page_size)
                    .with_show_removed(!hide_removed)
                    .with_exit_output(action_output || !actions.is_empty())
                    .with_custom_actions(actions),
                input_bytes,
                Vec::new(),
            )
        },
    }
}

fn run_config(config: WidgetConfig) -> Result<Value, CliError> {
    match config.kind {
        WidgetKind::Select => {
            let mut widget = Select::new("select", config.options)
                .with_page_size(config.page_size.unwrap_or(9))
                .with_wrap(config.wrap.unwrap_or(true));
            if let Some(prompt) = config.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(selected) = config.selected_indices.first() {
                widget = widget.with_selected_index(*selected);
            }
            run_widget(widget, config.input_bytes, config.actions)
        },
        WidgetKind::MultiSelect => {
            let first_selected = config.selected_indices.first().copied();
            let mut widget = MultiSelect::new("multi-select", config.options)
                .with_page_size(config.page_size.unwrap_or(9))
                .with_wrap(config.wrap.unwrap_or(true))
                .with_checked_indices(config.selected_indices);
            if let Some(prompt) = config.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(selected) = first_selected {
                widget = widget.with_selected_index(selected);
            }
            run_widget(widget, config.input_bytes, config.actions)
        },
        WidgetKind::Text => {
            run_widget(
                text_from_config(&config),
                config.input_bytes,
                config.actions,
            )
        },
        WidgetKind::Search => {
            let mut widget = SearchSelect::new("search", config.options)
                .with_page_size(config.page_size.unwrap_or(9))
                .with_wrap(config.wrap.unwrap_or(true));
            if let Some(prompt) = config.prompt {
                widget = widget.with_prompt(prompt);
            }
            if let Some(placeholder) = config.placeholder {
                widget = widget.with_placeholder(placeholder);
            }
            if let Some(selected) = config.selected_indices.first() {
                widget = widget.with_selected_match_index(*selected);
            }
            run_widget(widget, config.input_bytes, config.actions)
        },
        WidgetKind::Form => {
            let input_bytes = config.input_bytes.clone();
            let actions = config.actions.clone();
            let widget = form_from_config(config).map_err(CliError::Usage)?;
            run_widget(widget, input_bytes, actions)
        },
        WidgetKind::Date => {
            let mut widget = DatePicker::new(
                "date",
                config
                    .selected_date
                    .ok_or_else(|| "date config requires selected_date".to_owned())
                    .map_err(CliError::Usage)?,
            );
            if let Some(today) = config.today {
                widget = widget.with_today(today);
            }
            run_widget(widget, config.input_bytes, config.actions)
        },
        WidgetKind::ReviewList => {
            let first_selected = config.selected_indices.first().copied();
            let actions = review_actions_with_defaults(
                config.review_actions,
                config.action_output.unwrap_or(false),
            )
            .map_err(CliError::Usage)?;
            let mut widget = ReviewList::new("review-list", config.options)
                .with_page_size(config.page_size.unwrap_or(9))
                .with_wrap(config.wrap.unwrap_or(true))
                .with_states(config.review_states)
                .with_show_removed(config.show_removed.unwrap_or(true))
                .with_exit_output(config.action_output.unwrap_or(false) || !actions.is_empty())
                .with_custom_actions(actions);
            if let Some(prompt) = config.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(selected) = first_selected {
                widget = widget.with_selected_index(selected);
            }
            run_widget(widget, config.input_bytes, Vec::new())
        },
    }
}

fn form_from_config(config: WidgetConfig) -> Result<Form, String> {
    let mut form = Form::new("form");
    for field in config.fields {
        push_form_field(&mut form, field)?;
    }
    Ok(form)
}

fn push_form_field(form: &mut Form, field: FieldConfig) -> Result<(), String> {
    match field.kind {
        WidgetKind::Select => {
            let mut widget = Select::new(field.name.clone(), field.options)
                .with_page_size(field.page_size.unwrap_or(9))
                .with_wrap(field.wrap.unwrap_or(true));
            if let Some(prompt) = field.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(selected) = field.selected_indices.first() {
                widget = widget.with_selected_index(*selected);
            }
            push_action_field(form, field.name, widget, field.actions);
        },
        WidgetKind::MultiSelect => {
            let first_selected = field.selected_indices.first().copied();
            let mut widget = MultiSelect::new(field.name.clone(), field.options)
                .with_page_size(field.page_size.unwrap_or(9))
                .with_wrap(field.wrap.unwrap_or(true))
                .with_checked_indices(field.selected_indices);
            if let Some(prompt) = field.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(selected) = first_selected {
                widget = widget.with_selected_index(selected);
            }
            push_action_field(form, field.name, widget, field.actions);
        },
        WidgetKind::Text => {
            let mut widget = TextInput::new(field.name.clone());
            if let Some(prompt) = field.prompt {
                widget = widget.with_prompt(prompt);
            } else {
                widget = widget.with_prompt(format!("{}: ", field.name));
            }
            if let Some(placeholder) = field.placeholder {
                widget = widget.with_placeholder(placeholder);
            }
            if let Some(value) = field.value {
                widget = widget.with_value(value);
            }
            push_action_field(form, field.name, widget, field.actions);
        },
        WidgetKind::Search => {
            let mut widget = SearchSelect::new(field.name.clone(), field.options)
                .with_page_size(field.page_size.unwrap_or(9))
                .with_wrap(field.wrap.unwrap_or(true));
            if let Some(prompt) = field.prompt {
                widget = widget.with_prompt(prompt);
            }
            if let Some(placeholder) = field.placeholder {
                widget = widget.with_placeholder(placeholder);
            }
            if let Some(selected) = field.selected_indices.first() {
                widget = widget.with_selected_match_index(*selected);
            }
            push_action_field(form, field.name, widget, field.actions);
        },
        WidgetKind::Date => {
            let mut widget = DatePicker::new(
                field.name.clone(),
                field
                    .selected_date
                    .ok_or_else(|| format!("field '{}' requires selected_date", field.name))?,
            );
            if let Some(today) = field.today {
                widget = widget.with_today(today);
            }
            push_action_field(form, field.name, widget, field.actions);
        },
        WidgetKind::ReviewList => {
            let first_selected = field.selected_indices.first().copied();
            let actions = review_actions_with_defaults(
                field.review_actions,
                field.action_output.unwrap_or(false),
            )?;
            let mut widget = ReviewList::new(field.name.clone(), field.options)
                .with_page_size(field.page_size.unwrap_or(9))
                .with_wrap(field.wrap.unwrap_or(true))
                .with_states(field.review_states)
                .with_show_removed(field.show_removed.unwrap_or(true))
                .with_exit_output(field.action_output.unwrap_or(false) || !actions.is_empty())
                .with_custom_actions(actions)
                .with_leave_output(false);
            if let Some(prompt) = field.prompt {
                widget = widget.with_header(prompt);
            }
            if let Some(selected) = first_selected {
                widget = widget.with_selected_index(selected);
            }
            form.push_field(field.name, widget);
        },
        WidgetKind::Form => return Err("nested form fields are not supported yet".to_owned()),
    }
    Ok(())
}

fn push_action_field<W>(form: &mut Form, name: String, widget: W, actions: Vec<ActionBinding>)
where
    W: Widget + 'static,
{
    if actions.is_empty() {
        form.push_field(name, widget);
    } else {
        form.push_field(name, ActionLayer::new(widget).with_actions(actions));
    }
}

fn choice_items(options: Vec<String>) -> Result<Vec<SelectItem>, String> {
    if options.is_empty() {
        return Err("at least one --option is required".to_owned());
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
        bindings.push(push_unique_action(&mut seen, parse_action_binding(&action)?)?);
    }
    Ok(bindings)
}

fn review_action_bindings_with_defaults(
    actions: Vec<String>,
    defaults: bool,
) -> Result<Vec<ReviewActionBinding>, String> {
    let mut bindings = if defaults {
        default_review_actions()
    } else {
        Vec::new()
    };
    let mut seen: Vec<char> = bindings.iter().map(ReviewActionBinding::key).collect();
    for action in actions {
        bindings.push(push_unique_review_action(
            &mut seen,
            parse_review_action_binding(&action)?,
        )?);
    }
    Ok(bindings)
}

fn review_actions_with_defaults(
    actions: Vec<ReviewActionBinding>,
    defaults: bool,
) -> Result<Vec<ReviewActionBinding>, String> {
    let mut bindings = if defaults {
        default_review_actions()
    } else {
        Vec::new()
    };
    let mut seen: Vec<char> = bindings.iter().map(ReviewActionBinding::key).collect();
    for action in actions {
        bindings.push(push_unique_review_action(&mut seen, action)?);
    }
    Ok(bindings)
}

fn default_review_actions() -> Vec<ReviewActionBinding> {
    vec![
        ReviewActionBinding::new('g', "regen").with_help("regenerate"),
        ReviewActionBinding::new('s', "search"),
        ReviewActionBinding::new('a', "add"),
    ]
}

fn run_widget(
    widget: impl Widget + 'static,
    input_bytes: Option<String>,
    actions: Vec<ActionBinding>,
) -> Result<Value, CliError> {
    if let Some(input_bytes) = input_bytes {
        let widget = ActionLayer::new(widget).with_actions(actions);
        let bytes = decode_escaped(&input_bytes).map_err(CliError::Usage)?;
        let mut decoder = Decoder::new();
        let events = decoder.feed(&bytes).into_iter().chain(decoder.flush());
        bang::advanced::replay_events(widget, events).map_err(CliError::from)
    } else {
        bang::advanced::interact_widget(widget, actions).map_err(CliError::from)
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
    let high = high
        .to_digit(16)
        .ok_or_else(|| format!("invalid hex digit '{high}' in --input-bytes"))?;
    let low = low
        .to_digit(16)
        .ok_or_else(|| format!("invalid hex digit '{low}' in --input-bytes"))?;
    u8::try_from((high << 4) | low).map_err(|_| "hex byte out of range".to_owned())
}

#[cfg(test)]
mod tests {
    use bang::{
        advanced::LiveSessionError,
        terminal::{CleanupFailure, CleanupFailures, CleanupStage, Signal},
    };

    use super::{CliError, cleanup_failures};

    #[test]
    fn an_interruption_keeps_its_cleanup_failures() {
        let session = LiveSessionError::Cleanup {
            primary: Some(Box::new(LiveSessionError::Signalled(Signal::TERM))),
            failures: CleanupFailures::new(vec![CleanupFailure::new(
                CleanupStage::RawMode,
                std::io::Error::from_raw_os_error(5),
            )]),
        };
        let failures = cleanup_failures(Some(&session));

        assert_eq!(failures.len(), 1);
        assert!(failures[0].starts_with("terminal cleanup failed, RawMode"));
        assert_eq!(CliError::Interrupted(15, failures).exit_code(), 143);
    }
}
