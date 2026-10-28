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
        ReviewList,
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

const LISTS: &[RawWidgetKind] = &[
    RawWidgetKind::Select,
    RawWidgetKind::MultiSelect,
    RawWidgetKind::Search,
    RawWidgetKind::ReviewList,
];

const APPLICABLE: [(&str, &[RawWidgetKind]); 10] = [
    ("placeholder", &[RawWidgetKind::Text, RawWidgetKind::Search]),
    ("value", &[RawWidgetKind::Text]),
    ("options", LISTS),
    ("state", &[RawWidgetKind::ReviewList]),
    ("page_size", LISTS),
    ("wrap", LISTS),
    ("show_removed", &[RawWidgetKind::ReviewList]),
    ("action_output", &[RawWidgetKind::ReviewList]),
    ("selected_date", &[RawWidgetKind::Date]),
    ("today", &[RawWidgetKind::Date]),
];

/// `set` follows the order of [`APPLICABLE`]
fn reject_inapplicable(owner: &str, kind: RawWidgetKind, set: [bool; 10]) -> Result<(), String> {
    for ((key, kinds), is_set) in APPLICABLE.into_iter().zip(set) {
        if is_set && !kinds.contains(&kind) {
            return Err(inapplicable(owner, key, kind));
        }
    }
    Ok(())
}

fn inapplicable(owner: &str, key: &str, kind: RawWidgetKind) -> String {
    format!(
        "{owner} key '{key}' does not apply to type '{}'",
        kind.name()
    )
}

fn any_state(options: &[OptionEntry]) -> bool {
    options
        .iter()
        .any(|option| matches!(option, OptionEntry::Detailed(detailed) if detailed.state.is_some()))
}

impl RawConfig {
    fn finish(self) -> Result<WidgetConfig, String> {
        reject_inapplicable("config", self.kind, [
            self.placeholder.is_some(),
            self.value.is_some(),
            !self.options.is_empty(),
            any_state(&self.options),
            self.page_size.is_some(),
            self.wrap.is_some(),
            self.show_removed.is_some(),
            self.action_output.is_some(),
            self.selected_date.is_some(),
            self.today.is_some(),
        ])?;
        if !self.fields.is_empty() && self.kind != RawWidgetKind::Form {
            return Err(inapplicable("config", "fields", self.kind));
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
        reject_inapplicable(&format!("field '{}'", self.name), self.kind, [
            self.placeholder.is_some(),
            self.value.is_some(),
            !self.options.is_empty(),
            any_state(&self.options),
            self.page_size.is_some(),
            self.wrap.is_some(),
            self.show_removed.is_some(),
            self.action_output.is_some(),
            self.selected_date.is_some(),
            self.today.is_some(),
        ])?;
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
    if ReviewList::is_reserved_key(action.key()) {
        return Err(format!(
            "review action key '{}' is reserved by built-in review controls",
            action.key()
        ));
    }
    let key = ReviewList::fold_key(action.key());
    if seen.contains(&key) {
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
    let folded = letter.to_ascii_lowercase();
    let read_as = match folded {
        'm' | 'j' => Some("enter"),
        'i' => Some("tab"),
        'h' => Some("backspace"),
        _ => None,
    };
    if let Some(name) = read_as {
        return Err(format!(
            "control action key '{original}' is read as {name}, so bind {name} instead"
        ));
    }
    Ok(folded)
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
    value
        .parse()
        .map_err(|_error| format!("date '{value}' does not exist"))
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
    fn an_impossible_date_is_said_not_to_exist() {
        let error = WidgetConfig::parse("type = \"date\"\nselected_date = \"2024-02-30\"\n")
            .expect_err("a date off the calendar should be rejected");
        assert!(error.contains("'2024-02-30' does not exist"), "{error}");
    }

    #[test]
    fn keys_that_do_not_apply_to_the_type_are_rejected() {
        for (source, message) in [
            (
                "type = \"text\"\noptions = [\"a\"]\n",
                "config key 'options' does not apply to type 'text'",
            ),
            (
                "type = \"select\"\nplaceholder = \"x\"\noptions = [\"a\"]\n",
                "config key 'placeholder' does not apply to type 'select'",
            ),
            (
                "type = \"date\"\nselected_date = \"2024-02-29\"\npage_size = 3\n",
                "config key 'page_size' does not apply to type 'date'",
            ),
            (
                "type = \"select\"\noptions = [{ label = \"a\", state = \"denied\" }]\n",
                "config key 'state' does not apply to type 'select'",
            ),
            (
                "type = \"form\"\n\n[[fields]]\nname = \"a\"\ntype = \"select\"\nvalue = \
                 \"x\"\noptions = [\"o\"]\n",
                "field 'a' key 'value' does not apply to type 'select'",
            ),
        ] {
            let error = WidgetConfig::parse(source).expect_err("the key should be rejected");
            assert!(error.contains(message), "{error}");
        }
        WidgetConfig::parse("type = \"search\"\nplaceholder = \"x\"\noptions = [\"a\"]\n").unwrap();
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
