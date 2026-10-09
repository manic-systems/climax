// SPDX-License-Identifier: EUPL-1.2

use bang_core::{
    Value,
    widgets::{MultiSelect, Select, SelectItem},
};

use crate::{Error, Interaction, Result};

const DEFAULT_PAGE_SIZE: usize = 9;

/// Apply a prompt-specific configuration object to a typed prompt builder.
///
/// Every setting a config carries is also available as a direct builder method
/// on the prompt, so a config is only worth building when it is shared between
/// prompts.
pub trait Configurable: Sized {
    /// The configuration object for this prompt.
    type Config;

    /// Replace the prompt's presentation settings with `config`.
    ///
    /// The text given to the entry point stays as the header or prompt unless
    /// `config` sets its own.
    #[must_use]
    fn with_config(self, config: Self::Config) -> Self;
}

/// Ask the user to pick one of several choices under `header`.
#[must_use]
pub fn select<T>(header: impl Into<String>) -> SelectPrompt<T> {
    SelectPrompt::new(header)
}

/// Ask the user to pick any number of choices under `header`.
#[must_use]
pub fn multi_select<T>(header: impl Into<String>) -> MultiSelectPrompt<T> {
    MultiSelectPrompt::new(header)
}

#[derive(Clone, Debug)]
struct ListConfig {
    header: Option<String>,
    wrap: bool,
    page_size: usize,
    selected: Option<usize>,
}

impl Default for ListConfig {
    fn default() -> Self {
        Self {
            header: None,
            wrap: true,
            page_size: DEFAULT_PAGE_SIZE,
            selected: None,
        }
    }
}

impl ListConfig {
    fn with_header(header: impl Into<String>) -> Self {
        Self {
            header: Some(header.into()),
            ..Self::default()
        }
    }

    fn inherit_header(&mut self, previous: Option<String>) {
        if self.header.is_none() {
            self.header = previous;
        }
    }
}

/// Builder methods shared by every list prompt and its config, generated once
/// so the two cannot drift apart. `$list` names the path to the `ListConfig`.
macro_rules! list_options {
    ($selected_doc:literal, $($list:ident).+) => {
        /// Set the header shown above the list.
        #[must_use]
        pub fn header(mut self, header: impl Into<String>) -> Self {
            self.$($list).+.header = Some(header.into());
            self
        }

        /// Choose whether moving past either end wraps around. Defaults to
        /// `true`.
        #[must_use]
        pub const fn wrap(mut self, wrap: bool) -> Self {
            self.$($list).+.wrap = wrap;
            self
        }

        /// Set how many rows are visible at once.
        #[must_use]
        pub const fn page_size(mut self, page_size: usize) -> Self {
            self.$($list).+.page_size = page_size;
            self
        }

        #[doc = $selected_doc]
        #[must_use]
        pub const fn selected(mut self, selected: usize) -> Self {
            self.$($list).+.selected = Some(selected);
            self
        }
    };
}

/// Presentation settings for [`SelectPrompt`].
#[derive(Clone, Debug, Default)]
pub struct SelectConfig {
    list: ListConfig,
}

impl SelectConfig {
    list_options!("Start with the choice at `selected` highlighted, counting from zero.", list);
}

/// Presentation settings for [`MultiSelectPrompt`].
#[derive(Clone, Debug, Default)]
pub struct MultiSelectConfig {
    list: ListConfig,
    checked: Vec<usize>,
}

impl MultiSelectConfig {
    list_options!("Start with the choice at `selected` highlighted, counting from zero.", list);

    /// Start with the choice at `index` checked. Repeat to check several.
    #[must_use]
    pub fn checked(mut self, index: usize) -> Self {
        self.checked.push(index);
        self
    }

    /// Start with exactly the choices in `indices` checked, discarding any
    /// earlier [`checked`](Self::checked) calls.
    #[must_use]
    pub fn checked_indices(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        self.checked = indices.into_iter().collect();
        self
    }
}

/// How an ordinary typed prompt ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PromptOutcome<T> {
    /// The user submitted a value.
    Submit(T),
    /// The user left the prompt without submitting.
    Leave,
}

/// Builder methods every prompt shares. `$base` is the path from `self` to the
/// struct holding `id` and `interaction`, empty when `self` holds them.
macro_rules! prompt_options {
    ($($base:ident).*) => {
        /// Set the widget id used to identify the prompt to a custom driver.
        #[must_use]
        pub fn id(mut self, id: impl Into<String>) -> Self {
            self$(.$base)*.id = id.into();
            self
        }

        /// Run the prompt on `interaction` instead of the live terminal.
        #[must_use]
        pub fn interaction(mut self, interaction: Interaction) -> Self {
            self$(.$base)*.interaction = interaction;
            self
        }
    };
}

macro_rules! choice_option {
    () => {
        /// Add a choice shown as `label` that yields `value` when picked.
        #[must_use]
        pub fn choice(mut self, label: impl Into<String>, value: T) -> Self {
            self.choices.push(Choice {
                label: label.into(),
                value,
            });
            self
        }
    };
}

fn require_choices(kind: &str, count: usize) -> Result<()> {
    if count == 0 {
        return Err(Error::invalid_configuration(format!(
            "{kind} prompt needs at least one choice"
        )));
    }
    Ok(())
}

/// A prompt that picks one of several choices.
///
/// The prompt needs at least one choice. Create it with [`select`].
#[derive(Clone, Debug)]
pub struct SelectPrompt<T> {
    id: String,
    choices: Vec<Choice<T>>,
    config: SelectConfig,
    interaction: Interaction,
}

impl<T> SelectPrompt<T> {
    /// Create a select prompt with `header` above the choices.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            id: "select".to_owned(),
            choices: Vec::new(),
            config: SelectConfig {
                list: ListConfig::with_header(header),
            },
            interaction: Interaction::default(),
        }
    }

    choice_option!();
    prompt_options!();
    list_options!(
        "Start with the choice at `selected` highlighted, counting from zero.",
        config.list
    );

    /// Run the prompt to completion.
    ///
    /// Returns `Ok(PromptOutcome::Leave)` when the user leaves without
    /// submitting. Fails with `ErrorKind::InvalidConfiguration` if no choice
    /// was added, with `ErrorKind::InputEnded` if input ends first, and with
    /// `ErrorKind::InteractionUnavailable` if the driver's terminal isn't
    /// interactive.
    pub fn interact(self) -> Result<PromptOutcome<T>> {
        let interaction = self.interaction.clone();
        let (widget, choices) = self.into_widget()?;
        resolve_prompt(interaction.interact(widget, []), |value| {
            resolve_one(&value, choices)
        })
    }

    fn into_widget(self) -> Result<(Select, Vec<Choice<T>>)> {
        require_choices("select", self.choices.len())?;
        let items = choice_items(&self.choices);
        let mut widget = Select::new(self.id, items)
            .with_page_size(self.config.list.page_size)
            .with_wrap(self.config.list.wrap);
        if let Some(header) = self.config.list.header {
            widget = widget.with_header(header);
        }
        if let Some(selected) = self.config.list.selected {
            widget = widget.with_selected_index(selected);
        }
        Ok((widget, self.choices))
    }
}

impl<T> Configurable for SelectPrompt<T> {
    type Config = SelectConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        let header = self.config.list.header.take();
        self.config = config;
        self.config.list.inherit_header(header);
        self
    }
}

/// A prompt that picks any number of choices, including none.
///
/// Submitting with nothing checked yields an empty list. At least one choice
/// must be added. Create it with [`multi_select`].
#[derive(Clone, Debug)]
pub struct MultiSelectPrompt<T> {
    id: String,
    choices: Vec<Choice<T>>,
    config: MultiSelectConfig,
    interaction: Interaction,
}

impl<T> MultiSelectPrompt<T> {
    /// Create a multi-select prompt with `header` above the choices.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            id: "multi_select".to_owned(),
            choices: Vec::new(),
            config: MultiSelectConfig {
                list: ListConfig::with_header(header),
                checked: Vec::new(),
            },
            interaction: Interaction::default(),
        }
    }

    choice_option!();
    prompt_options!();
    list_options!(
        "Start with the choice at `selected` highlighted, counting from zero.",
        config.list
    );

    /// Start with the choice at `index` checked. Repeat to check several.
    #[must_use]
    pub fn checked(mut self, index: usize) -> Self {
        self.config.checked.push(index);
        self
    }

    /// Start with exactly the choices in `indices` checked, discarding any
    /// earlier [`checked`](Self::checked) calls.
    #[must_use]
    pub fn checked_indices(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        self.config.checked = indices.into_iter().collect();
        self
    }

    /// Run the prompt to completion.
    ///
    /// Returns `Ok(PromptOutcome::Leave)` when the user leaves without
    /// submitting. Fails with `ErrorKind::InvalidConfiguration` if no choice
    /// was added, with `ErrorKind::InputEnded` if input ends first, and with
    /// `ErrorKind::InteractionUnavailable` if the driver's terminal isn't
    /// interactive.
    pub fn interact(self) -> Result<PromptOutcome<Vec<T>>> {
        let interaction = self.interaction.clone();
        let (widget, choices) = self.into_widget()?;
        resolve_prompt(interaction.interact(widget, []), |value| {
            resolve_many(value, choices)
        })
    }

    fn into_widget(self) -> Result<(MultiSelect, Vec<Choice<T>>)> {
        require_choices("multi-select", self.choices.len())?;
        let mut widget = MultiSelect::new(self.id, choice_items(&self.choices))
            .with_page_size(self.config.list.page_size)
            .with_wrap(self.config.list.wrap)
            .with_checked_indices(self.config.checked);
        if let Some(header) = self.config.list.header {
            widget = widget.with_header(header);
        }
        if let Some(selected) = self.config.list.selected {
            widget = widget.with_selected_index(selected);
        }
        Ok((widget, self.choices))
    }
}

impl<T> Configurable for MultiSelectPrompt<T> {
    type Config = MultiSelectConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        let header = self.config.list.header.take();
        self.config = config;
        self.config.list.inherit_header(header);
        self
    }
}

#[derive(Clone, Debug)]
struct Choice<T> {
    label: String,
    value: T,
}

fn choice_items<T>(choices: &[Choice<T>]) -> Vec<SelectItem> {
    choices
        .iter()
        .enumerate()
        .map(|(index, choice)| SelectItem::new(choice.label.clone(), index.to_string()))
        .collect()
}

pub(crate) fn resolve_prompt<T>(
    result: Result<Value>,
    resolve: impl FnOnce(Value) -> Result<T>,
) -> Result<PromptOutcome<T>> {
    match result {
        Ok(value) => resolve(value).map(PromptOutcome::Submit),
        Err(error) if error.kind() == crate::ErrorKind::Cancelled => Ok(PromptOutcome::Leave),
        Err(error) => Err(error),
    }
}

fn resolve_one<T>(value: &Value, mut choices: Vec<Choice<T>>) -> Result<T> {
    let index = value
        .as_str()
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| Error::unexpected("a choice"))?;
    if index >= choices.len() {
        return Err(Error::unexpected("a known choice"));
    }
    Ok(choices.swap_remove(index).value)
}

fn resolve_many<T>(value: Value, choices: Vec<Choice<T>>) -> Result<Vec<T>> {
    let Value::List(values) = value else {
        return Err(Error::unexpected("choices"));
    };
    let mut indices = values
        .into_iter()
        .map(|value| {
            value
                .as_str()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| Error::unexpected("known choices"))
        })
        .collect::<Result<Vec<_>>>()?;
    if indices.iter().any(|index| *index >= choices.len()) {
        return Err(Error::unexpected("known choices"));
    }
    indices.sort_unstable();
    if indices.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(Error::unexpected("distinct choices"));
    }
    let mut output = Vec::with_capacity(indices.len());
    let mut choices = choices.into_iter().map(Some).collect::<Vec<_>>();
    for index in indices {
        output.push(choices[index].take().expect("indices are distinct").value);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_specific_configs_cover_normal_presentation_state() {
        let select = select::<()>("select").with_config(
            SelectConfig::default()
                .header("choose")
                .wrap(false)
                .page_size(4)
                .selected(2),
        );
        assert_eq!(select.config.list.header.as_deref(), Some("choose"));
        assert!(!select.config.list.wrap);
        assert_eq!(select.config.list.page_size, 4);
        assert_eq!(select.config.list.selected, Some(2));

        let multi = multi_select::<()>("multi").with_config(
            MultiSelectConfig::default()
                .header("choose several")
                .wrap(false)
                .page_size(5)
                .selected(1)
                .checked_indices([0, 2]),
        );
        assert_eq!(multi.config.list.selected, Some(1));
        assert_eq!(multi.config.checked, [0, 2]);
    }

    #[test]
    fn typed_choice_is_recovered() {
        let prompt = select("shell").choice("Bash", 10).choice("Zsh", 20);
        let (_widget, choices) = prompt.into_widget().unwrap();
        assert_eq!(resolve_one(&Value::from("1"), choices).unwrap(), 20);
    }

    #[test]
    fn typed_choices_are_recovered_in_selection_order() {
        let prompt = multi_select("shells")
            .choice("Bash", 10)
            .choice("Zsh", 20)
            .choice("Fish", 30);
        let (_widget, choices) = prompt.into_widget().unwrap();
        assert_eq!(
            resolve_many(
                Value::List(vec![Value::from("2"), Value::from("0")]),
                choices
            )
            .unwrap(),
            vec![10, 30]
        );
    }

    #[test]
    fn unknown_choice_is_an_owned_error() {
        let choices = vec![Choice {
            label: "Bash".to_owned(),
            value: 10,
        }];
        let error = resolve_one(&Value::from("9"), choices).unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::UnexpectedValue);
    }

    #[test]
    fn malformed_choice_is_an_owned_error() {
        let choices = vec![Choice {
            label: "Bash".to_owned(),
            value: 10,
        }];
        let error = resolve_one(&Value::from("not-an-index"), choices).unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::UnexpectedValue);
    }

    #[test]
    fn choice_from_an_empty_prompt_is_an_owned_error() {
        let error = resolve_one::<i32>(&Value::from("0"), Vec::new()).unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::UnexpectedValue);
    }

    #[test]
    fn scripted_interaction_drives_typed_prompts_without_a_terminal() {
        let interaction = crate::advanced::scripted_interaction([
            vec![
                bang_core::Event::key(bang_core::Key::Down),
                bang_core::Event::key(bang_core::Key::Enter),
            ],
        ]);
        let selected = select("shell")
            .choice("Bash", 10)
            .choice("Zsh", 20)
            .interaction(interaction)
            .interact()
            .unwrap();
        assert_eq!(selected, PromptOutcome::Submit(20));
    }

    #[test]
    fn cancellation_leaves_each_ordinary_typed_prompt() {
        let interaction = crate::advanced::scripted_interaction([
            vec![bang_core::Event::key(bang_core::Key::Esc)],
            vec![bang_core::Event::key(bang_core::Key::Esc)],
        ]);

        assert_eq!(
            select("shell")
                .choice("Bash", 10)
                .interaction(interaction.clone())
                .interact()
                .unwrap(),
            PromptOutcome::Leave
        );
        assert_eq!(
            multi_select("shells")
                .choice("Bash", 10)
                .interaction(interaction)
                .interact()
                .unwrap(),
            PromptOutcome::Leave
        );
    }

    #[test]
    fn disabled_interaction_fails_before_touching_the_terminal() {
        let error = select("shell")
            .choice("Bash", 10)
            .interaction(Interaction::disabled())
            .interact()
            .unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::InteractionUnavailable);
    }

    #[test]
    fn every_list_prompt_takes_the_direct_builders() {
        let select = select::<()>("pick")
            .id("one")
            .header("renamed")
            .wrap(false)
            .page_size(4)
            .selected(2);
        assert_eq!(select.id, "one");
        assert_eq!(select.config.list.header.as_deref(), Some("renamed"));
        assert!(!select.config.list.wrap);
        assert_eq!(select.config.list.page_size, 4);
        assert_eq!(select.config.list.selected, Some(2));

        let multi = multi_select::<()>("pick").selected(1).checked(0).checked(2);
        assert_eq!(multi.config.list.selected, Some(1));
        assert_eq!(multi.config.checked, [0, 2]);
        assert_eq!(multi.checked_indices([3]).config.checked, [3]);
    }

    #[test]
    fn entry_point_text_is_the_header_and_ids_default() {
        let select = select::<()>("pick a shell");
        assert_eq!(select.config.list.header.as_deref(), Some("pick a shell"));
        assert_eq!(select.id, "select");
        assert_eq!(multi_select::<()>("x").id, "multi_select");
    }

    #[test]
    fn with_config_keeps_the_entry_point_header_unless_overridden() {
        let kept = select::<()>("original").with_config(SelectConfig::default().page_size(3));
        assert_eq!(kept.config.list.header.as_deref(), Some("original"));
        assert_eq!(kept.config.list.page_size, 3);
    }

    #[test]
    fn every_list_prompt_rejects_zero_choices() {
        let kinds = [
            select::<i32>("x").interact().unwrap_err().kind(),
            multi_select::<i32>("x").interact().unwrap_err().kind(),
        ];
        assert!(
            kinds
                .iter()
                .all(|kind| *kind == crate::ErrorKind::InvalidConfiguration)
        );
    }
}
