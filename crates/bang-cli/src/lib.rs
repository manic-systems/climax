// SPDX-License-Identifier: EUPL-1.2

//! command-line bridge

mod config;

use bang_core::{
    ActionBinding,
    ActionLayer,
    OutputFormat as CoreOutputFormat,
    Reaction,
    Session,
    SessionStatus,
    Value,
    Widget,
    format_output,
    widgets::{
        DatePicker,
        Form,
        MultiSelect,
        ReviewList,
        SearchSelect,
        Select,
        SelectItem,
        TextInput,
    },
};
use bang_terminal::Decoder;
use config::{
    FieldConfig,
    WidgetConfig,
    WidgetKind,
    parse_action_binding,
    parse_review_action_binding,
    text_from_config,
};
use pound::{
    Parse,
    ValueEnum,
};

/// toplevel CLI
#[derive(Debug, Parse)]
#[pound(name = "bang", version = "0.1.0")]
pub struct Cli {
    #[pound(short, long)]
    config:  Option<String>,
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
        /// return an object with action and rows; enables g/s/a action keys
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

pub fn run_from_env() -> Result<String, String> {
    let cli = Cli::try_parse().map_err(|error| error.to_string())?;
    run(cli)
}

pub fn run_from_args<'a>(args: impl IntoIterator<Item = &'a str>) -> Result<String, String> {
    let cli = Cli::try_parse_from(args).map_err(|error| error.to_string())?;
    run(cli)
}

pub fn run(cli: Cli) -> Result<String, String> {
    let output = cli.output;
    let result = match (cli.config, cli.command) {
        (Some(_config), Some(_command)) => {
            return Err("use either --config or a widget subcommand, not both".to_owned());
        },
        (Some(config), None) => run_config(WidgetConfig::load(config)?),
        (None, None) => return Err("expected --config or a widget subcommand".to_owned()),
        (None, Some(command)) => run_command(command),
    }?;

    Ok(format_output(&result, output.into()))
}

fn run_command(command: Command) -> Result<Value, String> {
    match command {
        Command::Select {
            option,
            input_bytes,
            page_size,
            action,
        } => {
            run_widget(
                Select::new("select", choice_items(option)?).with_page_size(page_size),
                input_bytes,
                action_bindings(action)?,
            )
        },
        Command::MultiSelect {
            option,
            input_bytes,
            page_size,
            action,
        } => {
            run_widget(
                MultiSelect::new("multi-select", choice_items(option)?).with_page_size(page_size),
                input_bytes,
                action_bindings(action)?,
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
                action_bindings(action)?,
            )
        },
        Command::Search {
            option,
            input_bytes,
            page_size,
            action,
        } => {
            run_widget(
                SearchSelect::new("search", choice_items(option)?).with_page_size(page_size),
                input_bytes,
                action_bindings(action)?,
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
            run_widget(
                ReviewList::new("review-list", choice_items(option)?)
                    .with_page_size(page_size)
                    .with_show_removed(!hide_removed)
                    .with_action_output(action_output || !action.is_empty())
                    .with_custom_actions(review_action_bindings(action)?),
                input_bytes,
                Vec::new(),
            )
        },
    }
}

fn run_config(config: WidgetConfig) -> Result<Value, String> {
    match config.kind {
        WidgetKind::Select => {
            let mut widget = Select::new("select", config.options)
                .with_page_size(config.page_size.unwrap_or(9))
                .with_wrap(config.wrap.unwrap_or(true));
            if let Some(prompt) = config.prompt {
                widget = widget.with_prompt(prompt);
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
                widget = widget.with_prompt(prompt);
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
            let widget = form_from_config(config)?;
            run_widget(widget, input_bytes, actions)
        },
        WidgetKind::Date => {
            let mut widget = DatePicker::new(
                "date",
                config
                    .selected_date
                    .ok_or_else(|| "date config requires selected_date".to_owned())?,
            );
            if let Some(today) = config.today {
                widget = widget.with_today(today);
            }
            run_widget(widget, config.input_bytes, config.actions)
        },
        WidgetKind::ReviewList => {
            let first_selected = config.selected_indices.first().copied();
            let mut widget = ReviewList::new("review-list", config.options)
                .with_page_size(config.page_size.unwrap_or(9))
                .with_wrap(config.wrap.unwrap_or(true))
                .with_states(config.review_states)
                .with_show_removed(config.show_removed.unwrap_or(true))
                .with_action_output(
                    config.action_output.unwrap_or(false) || !config.review_actions.is_empty(),
                )
                .with_custom_actions(config.review_actions);
            if let Some(prompt) = config.prompt {
                widget = widget.with_prompt(prompt);
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
                widget = widget.with_prompt(prompt);
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
                widget = widget.with_prompt(prompt);
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
            let mut widget = ReviewList::new(field.name.clone(), field.options)
                .with_page_size(field.page_size.unwrap_or(9))
                .with_wrap(field.wrap.unwrap_or(true))
                .with_states(field.review_states)
                .with_show_removed(field.show_removed.unwrap_or(true))
                .with_action_output(
                    field.action_output.unwrap_or(false) || !field.review_actions.is_empty(),
                )
                .with_custom_actions(field.review_actions);
            if let Some(prompt) = field.prompt {
                widget = widget.with_prompt(prompt);
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
        let binding = parse_action_binding(&action)?;
        if seen.contains(binding.key_event()) {
            return Err(format!(
                "duplicate action key '{}'",
                action_key_label(binding.key_event())
            ));
        }
        seen.push(binding.key_event().clone());
        bindings.push(binding);
    }
    Ok(bindings)
}

fn review_action_bindings(
    actions: Vec<String>,
) -> Result<Vec<bang_core::widgets::ReviewActionBinding>, String> {
    let mut seen = Vec::new();
    let mut bindings = Vec::new();
    for action in actions {
        let binding = parse_review_action_binding(&action)?;
        if seen.contains(&binding.key()) {
            return Err(format!("duplicate review action key '{}'", binding.key()));
        }
        seen.push(binding.key());
        bindings.push(binding);
    }
    Ok(bindings)
}

fn action_key_label(key: &bang_core::KeyEvent) -> String {
    match (&key.key, key.modifiers) {
        (bang_core::Key::Char(value), modifiers) if modifiers == bang_core::Modifiers::CONTROL => {
            format!("ctrl-{value}")
        },
        (bang_core::Key::Char(value), modifiers) if modifiers.bits() == 0 => value.to_string(),
        (key, modifiers) if modifiers.bits() == 0 => format!("{key:?}").to_ascii_lowercase(),
        (key, modifiers) => format!("{key:?}+{}", modifiers.bits()).to_ascii_lowercase(),
    }
}

fn run_widget(
    widget: impl Widget + 'static,
    input_bytes: Option<String>,
    actions: Vec<ActionBinding>,
) -> Result<Value, String> {
    let widget = ActionLayer::new(widget).with_actions(actions);
    if let Some(input_bytes) = input_bytes {
        run_replayed_session(widget, &input_bytes)
    } else {
        bang_screw::run_live_session(widget).map_err(|error| error.to_string())
    }
}

fn run_replayed_session(widget: impl Widget + 'static, input_bytes: &str) -> Result<Value, String> {
    let mut session = Session::new(widget);
    let mut decoder = Decoder::new();
    let bytes = decode_escaped(input_bytes)?;
    for event in decoder.feed(&bytes).into_iter().chain(decoder.flush()) {
        match session.handle(event) {
            Reaction::Submit(value) => return Ok(value),
            Reaction::Cancel => return Err("cancelled".to_owned()),
            Reaction::Ignored | Reaction::Changed | Reaction::Focus(_) => {},
        }
        if !matches!(session.status(), SessionStatus::Running) {
            break;
        }
    }

    match session.status() {
        SessionStatus::Submitted(value) => Ok(value.clone()),
        SessionStatus::Cancelled => Err("cancelled".to_owned()),
        SessionStatus::Running => Err("input ended before submit".to_owned()),
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
