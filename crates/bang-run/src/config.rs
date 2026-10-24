// SPDX-License-Identifier: EUPL-1.2

use alloc::collections::BTreeMap;
use core::fmt;

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
use serde::{
    Deserialize,
    Deserializer,
    de::{
        self,
        MapAccess,
        Visitor,
        value::MapAccessDeserializer,
    },
};

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

impl WidgetKind {
    pub const fn name(self) -> &'static str {
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
        for (index, field) in fields.iter().enumerate() {
            if fields[..index]
                .iter()
                .any(|earlier| earlier.name == field.name)
            {
                return Err(format!("duplicate form field name '{}'", field.name));
            }
        }

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
#[serde(deny_unknown_fields)]
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
        if self.name.is_empty() {
            return Err("form field name cannot be empty".to_owned());
        }
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

#[derive(Debug)]
enum OptionEntry {
    Label(String),
    Detailed(DetailedOption),
}

impl<'de> Deserialize<'de> for OptionEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntryVisitor;

        impl<'de> Visitor<'de> for EntryVisitor {
            type Value = OptionEntry;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an option label or a table with a label")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<OptionEntry, E> {
                Ok(OptionEntry::Label(v.to_owned()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<OptionEntry, A::Error> {
                DetailedOption::deserialize(MapAccessDeserializer::new(map))
                    .map(OptionEntry::Detailed)
            }
        }

        deserializer.deserialize_any(EntryVisitor)
    }
}

impl OptionEntry {
    fn finish(self) -> Result<FinishedOption, String> {
        match self {
            Self::Label(label) if label.is_empty() => {
                Err("option label cannot be empty".to_owned())
            },
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
#[serde(deny_unknown_fields)]
struct DetailedOption {
    label:    String,
    value:    Option<toml::Value>,
    #[serde(default)]
    selected: bool,
    state:    Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAction {
    key:  String,
    name: String,
    help: Option<String>,
}

impl RawAction {
    fn finish_review(self) -> Result<ReviewActionBinding, String> {
        let key = parse_action_key(&self.key, "review action")?;
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
        if self.label.is_empty() {
            return Err("option label cannot be empty".to_owned());
        }
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
        let finished = option.finish()?;
        if finished.selected {
            selected_indices.push(index);
        }
        review_states.push(finished.review_state);
        items.push(finished.item);
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
        finish_review_actions(actions).map(|resolved| (Vec::new(), resolved))
    } else {
        finish_widget_actions(actions).map(|resolved| (resolved, Vec::new()))
    }
}

fn finish_review_actions(actions: Vec<RawAction>) -> Result<Vec<ReviewActionBinding>, String> {
    let mut seen = Vec::new();
    let mut finished = Vec::new();
    for action in actions {
        finished.push(push_unique_review_action(
            &mut seen,
            action.finish_review()?,
        )?);
    }
    Ok(finished)
}

fn finish_widget_actions(actions: Vec<RawAction>) -> Result<Vec<ActionBinding>, String> {
    let mut seen = Vec::new();
    let mut finished = Vec::new();
    for action in actions {
        finished.push(push_unique_action(&mut seen, action.finish_action()?)?);
    }
    Ok(finished)
}

/// the one place bang-run enforces that an [`ActionBinding`] key is unused so
/// far in the list being built, shared by the `--action` and config paths
pub(crate) fn push_unique_action(
    seen: &mut Vec<KeyEvent>,
    action: ActionBinding,
) -> Result<ActionBinding, String> {
    if seen.contains(action.key_event()) {
        return Err(format!(
            "duplicate action key '{}'",
            format_action_key_event(action.key_event())
        ));
    }
    seen.push(action.key_event().clone());
    Ok(action)
}

/// the one place bang-run enforces that a [`ReviewActionBinding`] key is
/// neither reserved nor already used, shared by the `--action` and config
/// paths
pub(crate) fn push_unique_review_action(
    seen: &mut Vec<char>,
    action: ReviewActionBinding,
) -> Result<ReviewActionBinding, String> {
    if is_reserved_action_key(action.key()) {
        return Err(format!(
            "review action key '{}' is reserved by built-in review controls",
            action.key()
        ));
    }
    let key = action.key().to_ascii_lowercase();
    if seen
        .iter()
        .any(|existing: &char| existing.eq_ignore_ascii_case(&key))
    {
        return Err(format!("duplicate review action key '{key}'"));
    }
    seen.push(key);
    Ok(action)
}

pub fn parse_action_binding(value: &str) -> Result<ActionBinding, String> {
    let Some((key_text, name)) = value.split_once(':') else {
        return Err("action must use key:name syntax".to_owned());
    };
    let key = parse_action_key_event(key_text)?;
    if name.is_empty() {
        return Err("action name cannot be empty".to_owned());
    }
    Ok(ActionBinding::new(key, name))
}

pub fn parse_review_action_binding(value: &str) -> Result<ReviewActionBinding, String> {
    let Some((key_text, name)) = value.split_once(':') else {
        return Err("review action must use key:name syntax".to_owned());
    };
    let key = parse_action_key(key_text, "review action")?;
    if name.is_empty() {
        return Err("review action name cannot be empty".to_owned());
    }
    Ok(ReviewActionBinding::new(key, name))
}

fn parse_action_key_event(value: &str) -> Result<KeyEvent, String> {
    let normalized = value.to_ascii_lowercase();
    if let Some(stripped) = normalized
        .strip_prefix("ctrl-")
        .or_else(|| normalized.strip_prefix("control-"))
    {
        let key = parse_control_action_key(stripped, value)?;
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
        _ => Key::Char(parse_action_key(value, "action")?),
    };
    Ok(KeyEvent::new(key))
}

fn parse_control_action_key(key: &str, original: &str) -> Result<char, String> {
    let mut chars = key.chars();
    let Some(letter) = chars.next() else {
        return Err("control action key cannot be empty".to_owned());
    };
    if chars.next().is_some() || !letter.is_ascii_alphabetic() {
        return Err(format!(
            "control action key '{original}' must use ctrl- plus one letter"
        ));
    }
    Ok(letter.to_ascii_lowercase())
}

fn parse_action_key(value: &str, label: &str) -> Result<char, String> {
    let mut chars = value.chars();
    let Some(key) = chars.next() else {
        return Err(format!("{label} key cannot be empty"));
    };
    if chars.next().is_some() || key.is_control() {
        return Err(format!(
            "{label} key '{value}' must be one printable character"
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
        (code, modifiers) if modifiers.bits() == 0 => format!("{code:?}").to_ascii_lowercase(),
        (code, modifiers) => format!("{code:?}+{}", modifiers.bits()).to_ascii_lowercase(),
    }
}

const fn is_reserved_action_key(key: char) -> bool {
    matches!(
        key,
        ' ' | '\t'
            | 'c'
            | 'C'
            | 'j'
            | 'J'
            | 'k'
            | 'K'
            | 'n'
            | 'N'
            | 'r'
            | 'R'
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
        toml::Value::String(text) => Value::String(text),
        toml::Value::Integer(integer) => Value::Number(Number::Integer(integer)),
        toml::Value::Float(float) => Value::Number(Number::Float(float)),
        toml::Value::Boolean(flag) => Value::Bool(flag),
        toml::Value::Datetime(moment) => Value::String(moment.to_string()),
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
                    .map(|(key, field)| Ok((key, toml_value_to_bang(field)?)))
                    .collect::<Result<BTreeMap<_, _>, String>>()?,
            )
        },
    })
}

fn parse_date(value: &str) -> Result<Date, String> {
    let digits = |part: &str, len: usize| {
        part.len() == len && part.bytes().all(|byte| byte.is_ascii_digit())
    };
    let well_formed = matches!(
        value.split('-').collect::<Vec<_>>()[..],
        [year, month, day] if digits(year, 4) && digits(month, 2) && digits(day, 2) && year != "0000"
    );
    if !well_formed {
        return Err(format!("invalid date '{value}', expected YYYY-MM-DD"));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_form_field_names_are_rejected() {
        let error = WidgetConfig::parse(
            r#"
type = "form"

[[fields]]
name = "who"
type = "text"

[[fields]]
name = "who"
type = "text"
"#,
        )
        .expect_err("duplicate names should be rejected");
        assert!(error.contains("duplicate form field name 'who'"), "{error}");
    }

    #[test]
    fn empty_names_and_labels_are_rejected() {
        for (source, message) in [
            (
                "type = \"form\"\n\n[[fields]]\nname = \"\"\ntype = \"text\"\n",
                "form field name cannot be empty",
            ),
            (
                "type = \"select\"\noptions = [\"\"]\n",
                "option label cannot be empty",
            ),
            (
                "type = \"select\"\noptions = [{ label = \"\" }]\n",
                "option label cannot be empty",
            ),
        ] {
            let error = WidgetConfig::parse(source).expect_err("empty names should be rejected");
            assert!(error.contains(message), "{error}");
        }
    }

    #[test]
    fn malformed_dates_are_rejected() {
        for date in [
            "2024-2-9",
            "99999-01-01",
            "0000-01-01",
            "-2024-01-01",
            "24-01-01",
            "2024-01-1x",
        ] {
            let source = format!("type = \"date\"\nselected_date = \"{date}\"\n");
            let error =
                WidgetConfig::parse(&source).expect_err("malformed dates should be rejected");
            assert!(error.contains("expected YYYY-MM-DD"), "{date} {error}");
        }
        WidgetConfig::parse("type = \"date\"\nselected_date = \"2024-02-29\"\n").unwrap();
    }

    #[test]
    fn unknown_keys_are_rejected_with_the_key_and_its_line() {
        for (source, key, line) in [
            (
                "type = \"select\"\npromt = \"Pick\"\noptions = [\"a\"]\n",
                "promt",
                "line 2",
            ),
            (
                "type = \"form\"\n\n[[fields]]\nname = \"a\"\ntype = \"text\"\nplaceholdr = \
                 \"x\"\n",
                "placeholdr",
                "line 6",
            ),
            (
                "type = \"select\"\noptions = [{ label = \"a\", selectd = true }]\n",
                "selectd",
                "line 2",
            ),
            (
                "type = \"text\"\n\n[[actions]]\nkey = \"a\"\nname = \"go\"\nhelp2 = \"x\"\n",
                "help2",
                "line 6",
            ),
        ] {
            let error = WidgetConfig::parse(source).expect_err("unknown keys should be rejected");
            assert!(error.contains(key), "{error}");
            assert!(error.contains(line), "{error}");
        }
    }
}
