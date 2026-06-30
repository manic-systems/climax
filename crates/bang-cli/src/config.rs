// SPDX-License-Identifier: EUPL-1.2

use std::{
    collections::BTreeMap,
    fs,
    path::Path,
};

use bang_core::{
    ActionBinding,
    Date,
    Key,
    KeyEvent,
    Modifiers,
    Number,
    Value,
    widgets::{
        ReviewActionBinding,
        ReviewState,
        SelectItem,
        TextInput,
    },
};
use serde::Deserialize;

/// parsed widget config
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetConfig {
    pub kind:             WidgetKind,
    pub input_bytes:      Option<String>,
    pub page_size:        Option<usize>,
    pub prompt:           Option<String>,
    pub placeholder:      Option<String>,
    pub value:            Option<String>,
    pub wrap:             Option<bool>,
    pub show_removed:     Option<bool>,
    pub action_output:    Option<bool>,
    pub actions:          Vec<ActionBinding>,
    pub review_actions:   Vec<ReviewActionBinding>,
    pub options:          Vec<SelectItem>,
    pub selected_indices: Vec<usize>,
    pub review_states:    Vec<ReviewState>,
    pub fields:           Vec<FieldConfig>,
    pub selected_date:    Option<Date>,
    pub today:            Option<Date>,
}

/// a single form field
#[derive(Clone, Debug, PartialEq)]
pub struct FieldConfig {
    pub name:             String,
    pub kind:             WidgetKind,
    pub page_size:        Option<usize>,
    pub prompt:           Option<String>,
    pub placeholder:      Option<String>,
    pub value:            Option<String>,
    pub wrap:             Option<bool>,
    pub show_removed:     Option<bool>,
    pub action_output:    Option<bool>,
    pub actions:          Vec<ActionBinding>,
    pub review_actions:   Vec<ReviewActionBinding>,
    pub options:          Vec<SelectItem>,
    pub selected_indices: Vec<usize>,
    pub review_states:    Vec<ReviewState>,
    pub selected_date:    Option<Date>,
    pub today:            Option<Date>,
}

impl WidgetConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let source = fs::read_to_string(path)
            .map_err(|error| format!("failed to read config {}: {error}", path.display()))?;
        Self::parse(&source)
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        toml::from_str::<RawConfig>(source)
            .map_err(|error| format!("failed to parse config: {error}"))?
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidgetKind {
    Select,
    MultiSelect,
    Text,
    Search,
    Form,
    Date,
    ReviewList,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(rename = "type")]
    kind:          RawWidgetKind,
    input_bytes:   Option<String>,
    page_size:     Option<usize>,
    prompt:        Option<String>,
    placeholder:   Option<String>,
    value:         Option<String>,
    selected_date: Option<String>,
    today:         Option<String>,
    wrap:          Option<bool>,
    show_removed:  Option<bool>,
    action_output: Option<bool>,
    #[serde(default)]
    actions:       Vec<RawAction>,
    #[serde(default)]
    options:       Vec<OptionEntry>,
    #[serde(default)]
    fields:        Vec<RawFieldConfig>,
}

impl RawConfig {
    fn finish(self) -> Result<WidgetConfig, String> {
        if matches!(
            self.kind,
            RawWidgetKind::Select
                | RawWidgetKind::MultiSelect
                | RawWidgetKind::Search
                | RawWidgetKind::ReviewList
        ) && self.options.is_empty()
        {
            return Err(format!(
                "config type '{}' requires at least one option",
                self.kind.name()
            ));
        }
        if self.kind == RawWidgetKind::Form && self.fields.is_empty() {
            return Err("config type 'form' requires at least one field".to_owned());
        }
        if self.kind == RawWidgetKind::Date && self.selected_date.is_none() {
            return Err("config type 'date' requires selected_date".to_owned());
        }

        if matches!(self.page_size, Some(0)) {
            return Err("config page_size must be at least 1".to_owned());
        }

        let options = finish_options(self.options)?;
        let fields = self
            .fields
            .into_iter()
            .map(RawFieldConfig::finish)
            .collect::<Result<Vec<_>, _>>()?;

        let is_review_list = self.kind == RawWidgetKind::ReviewList;
        let (actions, review_actions) = finish_actions(self.actions, is_review_list)?;

        Ok(WidgetConfig {
            kind: self.kind.into(),
            input_bytes: self.input_bytes,
            page_size: self.page_size,
            prompt: self.prompt,
            placeholder: self.placeholder,
            value: self.value,
            wrap: self.wrap,
            show_removed: self.show_removed,
            action_output: self.action_output,
            actions,
            review_actions,
            options: options.items,
            selected_indices: options.selected_indices,
            review_states: options.review_states,
            fields,
            selected_date: self.selected_date.as_deref().map(parse_date).transpose()?,
            today: self.today.as_deref().map(parse_date).transpose()?,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum RawWidgetKind {
    Select,
    MultiSelect,
    Text,
    Search,
    Form,
    Date,
    ReviewList,
}

impl RawWidgetKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::MultiSelect => "multi-select",
            Self::Text => "text",
            Self::Search => "search",
            Self::Form => "form",
            Self::Date => "date",
            Self::ReviewList => "review-list",
        }
    }
}

impl From<RawWidgetKind> for WidgetKind {
    fn from(value: RawWidgetKind) -> Self {
        match value {
            RawWidgetKind::Select => Self::Select,
            RawWidgetKind::MultiSelect => Self::MultiSelect,
            RawWidgetKind::Text => Self::Text,
            RawWidgetKind::Search => Self::Search,
            RawWidgetKind::Form => Self::Form,
            RawWidgetKind::Date => Self::Date,
            RawWidgetKind::ReviewList => Self::ReviewList,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawFieldConfig {
    name:          String,
    #[serde(rename = "type")]
    kind:          RawWidgetKind,
    page_size:     Option<usize>,
    prompt:        Option<String>,
    placeholder:   Option<String>,
    value:         Option<String>,
    selected_date: Option<String>,
    today:         Option<String>,
    wrap:          Option<bool>,
    show_removed:  Option<bool>,
    action_output: Option<bool>,
    #[serde(default)]
    actions:       Vec<RawAction>,
    #[serde(default)]
    options:       Vec<OptionEntry>,
}

impl RawFieldConfig {
    fn finish(self) -> Result<FieldConfig, String> {
        if self.kind == RawWidgetKind::Form {
            return Err("nested form fields are not supported yet".to_owned());
        }
        if matches!(
            self.kind,
            RawWidgetKind::Select
                | RawWidgetKind::MultiSelect
                | RawWidgetKind::Search
                | RawWidgetKind::ReviewList
        ) && self.options.is_empty()
        {
            return Err(format!(
                "field '{}' type '{}' requires at least one option",
                self.name,
                self.kind.name()
            ));
        }
        if matches!(self.page_size, Some(0)) {
            return Err(format!(
                "field '{}' page_size must be at least 1",
                self.name
            ));
        }
        if self.kind == RawWidgetKind::Date && self.selected_date.is_none() {
            return Err(format!(
                "field '{}' type 'date' requires selected_date",
                self.name
            ));
        }

        let options = finish_options(self.options)?;

        let is_review_list = self.kind == RawWidgetKind::ReviewList;
        let (actions, review_actions) = finish_actions(self.actions, is_review_list)?;

        Ok(FieldConfig {
            name: self.name,
            kind: self.kind.into(),
            page_size: self.page_size,
            prompt: self.prompt,
            placeholder: self.placeholder,
            value: self.value,
            wrap: self.wrap,
            show_removed: self.show_removed,
            action_output: self.action_output,
            actions,
            review_actions,
            options: options.items,
            selected_indices: options.selected_indices,
            review_states: options.review_states,
            selected_date: self.selected_date.as_deref().map(parse_date).transpose()?,
            today: self.today.as_deref().map(parse_date).transpose()?,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OptionEntry {
    Label(String),
    Detailed(DetailedOption),
}

impl OptionEntry {
    fn finish(self) -> Result<FinishedOption, String> {
        match self {
            Self::Label(label) => {
                Ok(FinishedOption {
                    item:         SelectItem::new(label.clone(), label),
                    selected:     false,
                    review_state: ReviewState::Unconfirmed,
                })
            },
            Self::Detailed(option) => option.finish(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct DetailedOption {
    label:    String,
    value:    Option<toml::Value>,
    #[serde(default)]
    selected: bool,
    state:    Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAction {
    key:  String,
    name: String,
    help: Option<String>,
}

impl RawAction {
    fn finish_review(self) -> Result<ReviewActionBinding, String> {
        let key = parse_action_key(&self.key)?;
        if self.name.is_empty() {
            return Err("review action name cannot be empty".to_owned());
        }
        let action = ReviewActionBinding::new(key, self.name);
        Ok(match self.help {
            Some(help) => action.with_help(help),
            None => action,
        })
    }

    fn finish_action(self) -> Result<ActionBinding, String> {
        let key = parse_action_key_event(&self.key)?;
        if self.name.is_empty() {
            return Err("action name cannot be empty".to_owned());
        }
        let action = ActionBinding::new(key, self.name);
        Ok(match self.help {
            Some(help) => action.with_help(help),
            None => action,
        })
    }
}

impl DetailedOption {
    fn finish(self) -> Result<FinishedOption, String> {
        let review_state = self
            .state
            .as_deref()
            .map(ReviewState::try_from)
            .transpose()?
            .unwrap_or_default();
        Ok(FinishedOption {
            item: SelectItem::new(
                self.label.clone(),
                self.value
                    .map(toml_value_to_bang)
                    .transpose()?
                    .unwrap_or_else(|| Value::from(self.label)),
            ),
            selected: self.selected,
            review_state,
        })
    }
}

#[derive(Debug, PartialEq)]
struct FinishedOption {
    item:         SelectItem,
    selected:     bool,
    review_state: ReviewState,
}

#[derive(Debug, PartialEq)]
struct FinishedOptions {
    items:            Vec<SelectItem>,
    selected_indices: Vec<usize>,
    review_states:    Vec<ReviewState>,
}

fn finish_options(options: Vec<OptionEntry>) -> Result<FinishedOptions, String> {
    let mut selected_indices = Vec::new();
    let mut items = Vec::new();
    let mut review_states = Vec::new();
    for (index, option) in options.into_iter().enumerate() {
        let option = option.finish()?;
        if option.selected {
            selected_indices.push(index);
        }
        review_states.push(option.review_state);
        items.push(option.item);
    }
    Ok(FinishedOptions {
        items,
        selected_indices,
        review_states,
    })
}

fn finish_actions(
    actions: Vec<RawAction>,
    review_list: bool,
) -> Result<(Vec<ActionBinding>, Vec<ReviewActionBinding>), String> {
    if review_list {
        finish_review_actions(actions).map(|actions| (Vec::new(), actions))
    } else {
        finish_widget_actions(actions).map(|actions| (actions, Vec::new()))
    }
}

fn finish_review_actions(actions: Vec<RawAction>) -> Result<Vec<ReviewActionBinding>, String> {
    let mut seen = Vec::new();
    let mut finished = Vec::new();
    for action in actions {
        let action = action.finish_review()?;
        if is_reserved_action_key(action.key()) {
            return Err(format!(
                "review action key '{}' is reserved by built-in review controls",
                action.key()
            ));
        }
        if seen.contains(&action.key()) {
            return Err(format!("duplicate review action key '{}'", action.key()));
        }
        seen.push(action.key());
        finished.push(action);
    }
    Ok(finished)
}

fn finish_widget_actions(actions: Vec<RawAction>) -> Result<Vec<ActionBinding>, String> {
    let mut seen = Vec::new();
    let mut finished = Vec::new();
    for action in actions {
        let action = action.finish_action()?;
        if seen.contains(action.key_event()) {
            return Err(format!(
                "duplicate action key '{}'",
                format_action_key_event(action.key_event())
            ));
        }
        seen.push(action.key_event().clone());
        finished.push(action);
    }
    Ok(finished)
}

pub fn parse_action_binding(value: &str) -> Result<ActionBinding, String> {
    let Some((key, name)) = value.split_once(':') else {
        return Err("action must use key:name syntax".to_owned());
    };
    let key = parse_action_key_event(key)?;
    if name.is_empty() {
        return Err("action name cannot be empty".to_owned());
    }
    Ok(ActionBinding::new(key, name))
}

pub fn parse_review_action_binding(value: &str) -> Result<ReviewActionBinding, String> {
    let Some((key, name)) = value.split_once(':') else {
        return Err("review action must use key:name syntax".to_owned());
    };
    let key = parse_action_key(key)?;
    if is_reserved_action_key(key) {
        return Err(format!(
            "review action key '{key}' is reserved by built-in review controls"
        ));
    }
    if name.is_empty() {
        return Err("review action name cannot be empty".to_owned());
    }
    Ok(ReviewActionBinding::new(key, name))
}

fn parse_action_key_event(value: &str) -> Result<KeyEvent, String> {
    let normalized = value.to_ascii_lowercase();
    if let Some(key) = normalized
        .strip_prefix("ctrl-")
        .or_else(|| normalized.strip_prefix("control-"))
    {
        let key = parse_control_action_key(key, value)?;
        return Ok(KeyEvent::with_modifiers(Key::Char(key), Modifiers::CONTROL));
    }

    let key = match normalized.as_str() {
        "enter" => Key::Enter,
        "esc" | "escape" => Key::Esc,
        "tab" => Key::Tab,
        "backtab" | "shift-tab" => Key::Backtab,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "page-up" | "pageup" => Key::PageUp,
        "page-down" | "pagedown" => Key::PageDown,
        _ => Key::Char(parse_action_key(value)?),
    };
    Ok(KeyEvent::new(key))
}

fn parse_control_action_key(key: &str, original: &str) -> Result<char, String> {
    let mut chars = key.chars();
    let Some(key) = chars.next() else {
        return Err("control action key cannot be empty".to_owned());
    };
    if chars.next().is_some() || !key.is_ascii_alphabetic() {
        return Err(format!(
            "control action key '{original}' must use ctrl- plus one letter"
        ));
    }
    Ok(key.to_ascii_lowercase())
}

fn parse_action_key(value: &str) -> Result<char, String> {
    let mut chars = value.chars();
    let Some(key) = chars.next() else {
        return Err("review action key cannot be empty".to_owned());
    };
    if chars.next().is_some() || key.is_control() {
        return Err(format!(
            "review action key '{value}' must be one printable character"
        ));
    }
    Ok(key)
}

fn format_action_key_event(key: &KeyEvent) -> String {
    match (&key.key, key.modifiers) {
        (Key::Char(value), modifiers) if modifiers == Modifiers::CONTROL => {
            format!("ctrl-{value}")
        },
        (Key::Char(value), modifiers) if modifiers.bits() == 0 => value.to_string(),
        (key, modifiers) if modifiers.bits() == 0 => format!("{key:?}").to_ascii_lowercase(),
        (key, modifiers) => format!("{key:?}+{}", modifiers.bits()).to_ascii_lowercase(),
    }
}

const fn is_reserved_action_key(key: char) -> bool {
    matches!(
        key,
        ' ' | '\t'
            | 'a'
            | 'A'
            | 'c'
            | 'C'
            | 'g'
            | 'G'
            | 'j'
            | 'J'
            | 'k'
            | 'K'
            | 'n'
            | 'N'
            | 'r'
            | 'R'
            | 's'
            | 'S'
            | 'u'
            | 'U'
            | 'x'
            | 'X'
            | 'y'
            | 'Y'
    )
}

fn toml_value_to_bang(value: toml::Value) -> Result<Value, String> {
    Ok(match value {
        toml::Value::String(value) => Value::String(value),
        toml::Value::Integer(value) => Value::Number(Number::Integer(value)),
        toml::Value::Float(value) => Value::Number(Number::Float(value)),
        toml::Value::Boolean(value) => Value::Bool(value),
        toml::Value::Datetime(value) => Value::String(value.to_string()),
        toml::Value::Array(values) => {
            Value::List(
                values
                    .into_iter()
                    .map(toml_value_to_bang)
                    .collect::<Result<Vec<_>, _>>()?,
            )
        },
        toml::Value::Table(values) => {
            Value::Object(
                values
                    .into_iter()
                    .map(|(key, value)| Ok((key, toml_value_to_bang(value)?)))
                    .collect::<Result<BTreeMap<_, _>, String>>()?,
            )
        },
    })
}

fn parse_date(value: &str) -> Result<Date, String> {
    value.parse()
}

pub fn text_from_config(config: &WidgetConfig) -> TextInput {
    let mut input = TextInput::new("text");
    if let Some(prompt) = &config.prompt {
        input = input.with_prompt(prompt.clone());
    } else {
        input = input.with_prompt("text: ");
    }
    if let Some(placeholder) = &config.placeholder {
        input = input.with_placeholder(placeholder.clone());
    }
    if let Some(value) = &config.value {
        input = input.with_value(value.clone());
    }
    input
}
