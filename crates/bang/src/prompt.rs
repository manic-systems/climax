use std::{fmt, sync::Arc};

use bang_core::{
    Value,
    widgets::{
        MultiSelect, ReviewActionBinding, ReviewList, ReviewState, SearchSelect, Select,
        SelectItem, TextInput,
    },
};

use crate::{Error, Interaction, Result};

const DEFAULT_PAGE_SIZE: usize = 9;

/// Apply a prompt-specific configuration object to a typed prompt builder.
pub trait Configurable: Sized {
    type Config;

    #[must_use]
    fn with_config(self, config: Self::Config) -> Self;
}

#[must_use]
pub fn select<T>(id: impl Into<String>) -> SelectPrompt<T> {
    SelectPrompt::new(id)
}

#[must_use]
pub fn multi_select<T>(id: impl Into<String>) -> MultiSelectPrompt<T> {
    MultiSelectPrompt::new(id)
}

#[must_use]
pub fn search<T>(id: impl Into<String>) -> SearchPrompt<T> {
    SearchPrompt::new(id)
}

#[must_use]
pub fn review<T>(id: impl Into<String>) -> ReviewPrompt<T> {
    ReviewPrompt::new(id)
}

#[must_use]
pub fn text(prompt: impl Into<String>) -> TextPrompt {
    TextPrompt::new(prompt)
}

#[derive(Clone, Debug)]
struct ListConfig {
    header: Option<String>,
    wrap: bool,
    page_size: usize,
}

impl Default for ListConfig {
    fn default() -> Self {
        Self {
            header: None,
            wrap: true,
            page_size: DEFAULT_PAGE_SIZE,
        }
    }
}

macro_rules! list_config_methods {
    () => {
        #[must_use]
        pub fn header(mut self, header: impl Into<String>) -> Self {
            self.list.header = Some(header.into());
            self
        }

        #[must_use]
        pub const fn wrap(mut self, wrap: bool) -> Self {
            self.list.wrap = wrap;
            self
        }

        #[must_use]
        pub const fn page_size(mut self, page_size: usize) -> Self {
            self.list.page_size = page_size;
            self
        }
    };
}

#[derive(Clone, Debug, Default)]
pub struct SelectConfig {
    list: ListConfig,
    selected: Option<usize>,
}

impl SelectConfig {
    list_config_methods!();

    #[must_use]
    pub const fn selected(mut self, selected: usize) -> Self {
        self.selected = Some(selected);
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct MultiSelectConfig {
    list: ListConfig,
    selected: Option<usize>,
    checked: Vec<usize>,
}

impl MultiSelectConfig {
    list_config_methods!();

    #[must_use]
    pub const fn selected(mut self, selected: usize) -> Self {
        self.selected = Some(selected);
        self
    }

    #[must_use]
    pub fn checked(mut self, index: usize) -> Self {
        self.checked.push(index);
        self
    }

    #[must_use]
    pub fn checked_indices(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        self.checked = indices.into_iter().collect();
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchConfig {
    list: ListConfig,
    prompt: Option<String>,
    placeholder: Option<String>,
    selected: Option<usize>,
}

impl SearchConfig {
    list_config_methods!();

    #[must_use]
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    #[must_use]
    pub const fn selected(mut self, selected: usize) -> Self {
        self.selected = Some(selected);
        self
    }
}

type Validator = dyn Fn(&str) -> std::result::Result<(), String> + Send + Sync + 'static;

#[derive(Clone, Default)]
pub struct TextConfig {
    id: Option<String>,
    value: Option<String>,
    placeholder: Option<String>,
    validator: Option<Arc<Validator>>,
}

impl TextConfig {
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    #[must_use]
    pub fn validator(
        mut self,
        validator: impl Fn(&str) -> std::result::Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.validator = Some(Arc::new(validator));
        self
    }
}

impl fmt::Debug for TextConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TextConfig")
            .field("id", &self.id)
            .field("value", &self.value)
            .field("placeholder", &self.placeholder)
            .field("validator", &self.validator.as_ref().map(|_| ".."))
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct ReviewConfig {
    list: ListConfig,
    selected: Option<usize>,
    show_removed: bool,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            list: ListConfig::default(),
            selected: None,
            show_removed: true,
        }
    }
}

impl ReviewConfig {
    list_config_methods!();

    #[must_use]
    pub const fn selected(mut self, selected: usize) -> Self {
        self.selected = Some(selected);
        self
    }

    #[must_use]
    pub const fn show_removed(mut self, show_removed: bool) -> Self {
        self.show_removed = show_removed;
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

/// How a review interaction ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewExit<A> {
    Submit,
    Leave,
    Action(A),
}

/// An item returned from an accepted review.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reviewed<T> {
    value: T,
    state: ReviewState,
    changed: bool,
}

impl<T> Reviewed<T> {
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    #[must_use]
    pub const fn state(&self) -> ReviewState {
        self.state
    }

    #[must_use]
    pub const fn changed(&self) -> bool {
        self.changed
    }

    #[must_use]
    pub fn into_parts(self) -> (T, ReviewState, bool) {
        (self.value, self.state, self.changed)
    }
}

/// The exit and, when accepted, the resulting review items.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewOutcome<T, A> {
    exit: ReviewExit<A>,
    accepted_items: Option<Vec<Reviewed<T>>>,
}

impl<T, A> ReviewOutcome<T, A> {
    #[must_use]
    pub const fn exit(&self) -> &ReviewExit<A> {
        &self.exit
    }

    /// Returns `None` after [`ReviewExit::Leave`], since provisional edits were
    /// discarded.
    #[must_use]
    pub fn accepted_items(&self) -> Option<&[Reviewed<T>]> {
        self.accepted_items.as_deref()
    }

    #[must_use]
    pub fn into_parts(self) -> (ReviewExit<A>, Option<Vec<Reviewed<T>>>) {
        (self.exit, self.accepted_items)
    }
}

#[derive(Clone, Debug)]
struct ReviewPromptCore<T> {
    id: String,
    choices: Vec<ReviewChoice<T>>,
    config: ReviewConfig,
    interaction: Interaction,
}

impl<T> ReviewPromptCore<T> {
    #[must_use]
    fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            choices: Vec::new(),
            config: ReviewConfig::default(),
            interaction: Interaction::default(),
        }
    }

    fn into_widget<A>(
        self,
        actions: &[ReviewPromptAction<A>],
    ) -> (ReviewList, Vec<ReviewChoice<T>>) {
        let items = self
            .choices
            .iter()
            .enumerate()
            .map(|(index, choice)| SelectItem::new(choice.label.clone(), index.to_string()));
        let mut widget = ReviewList::new(self.id, items)
            .with_page_size(self.config.list.page_size)
            .with_wrap(self.config.list.wrap)
            .with_show_removed(self.config.show_removed)
            .with_exit_output(true)
            .with_states(self.choices.iter().map(|choice| choice.state))
            .with_custom_actions(actions.iter().enumerate().map(|(index, action)| {
                ReviewActionBinding::new(action.key, index.to_string())
                    .with_help(action.help.clone())
            }));
        if let Some(header) = self.config.list.header {
            widget = widget.with_header(header);
        }
        if let Some(selected) = self.config.selected {
            widget = widget.with_selected_index(selected);
        }
        (widget, self.choices)
    }
}

macro_rules! review_prompt_methods {
    () => {
        #[must_use]
        pub fn header(mut self, header: impl Into<String>) -> Self {
            self.core.config = self.core.config.header(header);
            self
        }

        #[must_use]
        pub fn item(mut self, label: impl Into<String>, value: T, state: ReviewState) -> Self {
            self.core.choices.push(ReviewChoice {
                label: label.into(),
                value,
                state,
            });
            self
        }

        #[must_use]
        pub fn page_size(mut self, page_size: usize) -> Self {
            self.core.config = self.core.config.page_size(page_size);
            self
        }

        #[must_use]
        pub fn wrap(mut self, wrap: bool) -> Self {
            self.core.config = self.core.config.wrap(wrap);
            self
        }

        #[must_use]
        pub fn selected(mut self, selected: usize) -> Self {
            self.core.config = self.core.config.selected(selected);
            self
        }

        #[must_use]
        pub fn show_removed(mut self, show_removed: bool) -> Self {
            self.core.config = self.core.config.show_removed(show_removed);
            self
        }

        #[must_use]
        pub fn interaction(mut self, interaction: Interaction) -> Self {
            self.core.interaction = interaction;
            self
        }
    };
}

/// A review prompt with ordinary submit/leave outcomes.
#[derive(Clone, Debug)]
pub struct ReviewPrompt<T> {
    core: ReviewPromptCore<T>,
}

impl<T> ReviewPrompt<T> {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            core: ReviewPromptCore::new(id),
        }
    }

    review_prompt_methods!();

    /// Add the first intrinsic review action and transition to an
    /// action-bearing review prompt.
    #[must_use]
    pub fn action<A>(
        self,
        key: char,
        help: impl Into<String>,
        value: A,
    ) -> ReviewPromptWithActions<T, A> {
        ReviewPromptWithActions {
            core: self.core,
            actions: vec![ReviewPromptAction {
                key,
                help: help.into(),
                value,
            }],
        }
    }

    pub fn interact(self) -> Result<PromptOutcome<Vec<Reviewed<T>>>> {
        let interaction = self.core.interaction.clone();
        let (widget, choices) = self
            .core
            .into_widget(&[] as &[ReviewPromptAction<std::convert::Infallible>]);
        let outcome: ReviewOutcome<T, std::convert::Infallible> =
            resolve_review(interaction.interact(widget, [])?, choices, Vec::new())?;
        let (exit, items) = outcome.into_parts();
        match exit {
            ReviewExit::Submit => items
                .map(PromptOutcome::Submit)
                .ok_or_else(|| Error::unexpected("accepted review items")),
            ReviewExit::Leave => Ok(PromptOutcome::Leave),
            ReviewExit::Action(never) => match never {},
        }
    }
}

impl<T> Configurable for ReviewPrompt<T> {
    type Config = ReviewConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.core.config = config;
        self
    }
}

/// A review prompt with one or more typed intrinsic actions.
#[derive(Clone, Debug)]
pub struct ReviewPromptWithActions<T, A> {
    core: ReviewPromptCore<T>,
    actions: Vec<ReviewPromptAction<A>>,
}

impl<T, A> ReviewPromptWithActions<T, A> {
    review_prompt_methods!();

    #[must_use]
    pub fn action(mut self, key: char, help: impl Into<String>, value: A) -> Self {
        self.actions.push(ReviewPromptAction {
            key,
            help: help.into(),
            value,
        });
        self
    }

    pub fn interact(self) -> Result<ReviewOutcome<T, A>> {
        validate_review_actions(&self.actions)?;
        let interaction = self.core.interaction.clone();
        let (widget, choices) = self.core.into_widget(&self.actions);
        resolve_review(interaction.interact(widget, [])?, choices, self.actions)
    }
}

impl<T, A> Configurable for ReviewPromptWithActions<T, A> {
    type Config = ReviewConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.core.config = config;
        self
    }
}

fn validate_review_actions<A>(actions: &[ReviewPromptAction<A>]) -> Result<()> {
    const RESERVED: &str = "jkrycxnu ";
    let mut seen = Vec::new();
    for action in actions {
        let key = action.key.to_ascii_lowercase();
        if key.is_control() || RESERVED.contains(key) {
            return Err(Error::invalid_configuration(format!(
                "review action key '{key}' is reserved"
            )));
        }
        if seen.contains(&key) {
            return Err(Error::invalid_configuration(format!(
                "duplicate review action key '{key}'"
            )));
        }
        seen.push(key);
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct ReviewChoice<T> {
    label: String,
    value: T,
    state: ReviewState,
}

#[derive(Clone, Debug)]
struct ReviewPromptAction<A> {
    key: char,
    help: String,
    value: A,
}

#[derive(Clone, Debug)]
pub struct SelectPrompt<T> {
    id: String,
    choices: Vec<Choice<T>>,
    config: SelectConfig,
    interaction: Interaction,
}

impl<T> SelectPrompt<T> {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            choices: Vec::new(),
            config: SelectConfig::default(),
            interaction: Interaction::default(),
        }
    }

    #[must_use]
    pub fn choice(mut self, label: impl Into<String>, value: T) -> Self {
        self.choices.push(Choice {
            label: label.into(),
            value,
        });
        self
    }

    #[must_use]
    pub fn page_size(mut self, page_size: usize) -> Self {
        self.config = self.config.page_size(page_size);
        self
    }

    #[must_use]
    pub fn header(mut self, header: impl Into<String>) -> Self {
        self.config = self.config.header(header);
        self
    }

    #[must_use]
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.config = self.config.wrap(wrap);
        self
    }

    #[must_use]
    pub fn selected(mut self, selected: usize) -> Self {
        self.config = self.config.selected(selected);
        self
    }

    #[must_use]
    pub fn interaction(mut self, interaction: Interaction) -> Self {
        self.interaction = interaction;
        self
    }

    pub fn interact(self) -> Result<PromptOutcome<T>> {
        let interaction = self.interaction.clone();
        let (widget, choices) = self.into_widget();
        resolve_prompt(interaction.interact(widget, []), |value| {
            resolve_one(&value, choices)
        })
    }

    fn into_widget(self) -> (Select, Vec<Choice<T>>) {
        let items = choice_items(&self.choices);
        let mut widget = Select::new(self.id, items)
            .with_page_size(self.config.list.page_size)
            .with_wrap(self.config.list.wrap);
        if let Some(header) = self.config.list.header {
            widget = widget.with_header(header);
        }
        if let Some(selected) = self.config.selected {
            widget = widget.with_selected_index(selected);
        }
        (widget, self.choices)
    }
}

impl<T> Configurable for SelectPrompt<T> {
    type Config = SelectConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.config = config;
        self
    }
}

#[derive(Clone, Debug)]
pub struct MultiSelectPrompt<T> {
    id: String,
    choices: Vec<Choice<T>>,
    config: MultiSelectConfig,
    interaction: Interaction,
}

impl<T> MultiSelectPrompt<T> {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            choices: Vec::new(),
            config: MultiSelectConfig::default(),
            interaction: Interaction::default(),
        }
    }

    #[must_use]
    pub fn choice(mut self, label: impl Into<String>, value: T) -> Self {
        self.choices.push(Choice {
            label: label.into(),
            value,
        });
        self
    }

    #[must_use]
    pub fn page_size(mut self, page_size: usize) -> Self {
        self.config = self.config.page_size(page_size);
        self
    }

    #[must_use]
    pub fn header(mut self, header: impl Into<String>) -> Self {
        self.config = self.config.header(header);
        self
    }

    #[must_use]
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.config = self.config.wrap(wrap);
        self
    }

    #[must_use]
    pub fn selected(mut self, selected: usize) -> Self {
        self.config = self.config.selected(selected);
        self
    }

    #[must_use]
    pub fn checked(mut self, index: usize) -> Self {
        self.config = self.config.checked(index);
        self
    }

    #[must_use]
    pub fn interaction(mut self, interaction: Interaction) -> Self {
        self.interaction = interaction;
        self
    }

    pub fn interact(self) -> Result<PromptOutcome<Vec<T>>> {
        let interaction = self.interaction.clone();
        let (widget, choices) = self.into_widget();
        resolve_prompt(interaction.interact(widget, []), |value| {
            resolve_many(value, choices)
        })
    }

    fn into_widget(self) -> (MultiSelect, Vec<Choice<T>>) {
        let mut widget = MultiSelect::new(self.id, choice_items(&self.choices))
            .with_page_size(self.config.list.page_size)
            .with_wrap(self.config.list.wrap)
            .with_checked_indices(self.config.checked);
        if let Some(header) = self.config.list.header {
            widget = widget.with_header(header);
        }
        if let Some(selected) = self.config.selected {
            widget = widget.with_selected_index(selected);
        }
        (widget, self.choices)
    }
}

impl<T> Configurable for MultiSelectPrompt<T> {
    type Config = MultiSelectConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.config = config;
        self
    }
}

#[derive(Clone, Debug)]
pub struct SearchPrompt<T> {
    id: String,
    choices: Vec<Choice<T>>,
    config: SearchConfig,
    interaction: Interaction,
}

impl<T> SearchPrompt<T> {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            choices: Vec::new(),
            config: SearchConfig::default(),
            interaction: Interaction::default(),
        }
    }

    #[must_use]
    pub fn choice(mut self, label: impl Into<String>, value: T) -> Self {
        self.choices.push(Choice {
            label: label.into(),
            value,
        });
        self
    }

    #[must_use]
    pub fn page_size(mut self, page_size: usize) -> Self {
        self.config = self.config.page_size(page_size);
        self
    }

    #[must_use]
    pub fn header(mut self, header: impl Into<String>) -> Self {
        self.config = self.config.header(header);
        self
    }

    #[must_use]
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.config = self.config.wrap(wrap);
        self
    }

    #[must_use]
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config = self.config.prompt(prompt);
        self
    }

    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.config = self.config.placeholder(placeholder);
        self
    }

    #[must_use]
    pub fn selected(mut self, selected: usize) -> Self {
        self.config = self.config.selected(selected);
        self
    }

    #[must_use]
    pub fn interaction(mut self, interaction: Interaction) -> Self {
        self.interaction = interaction;
        self
    }

    pub fn interact(self) -> Result<PromptOutcome<T>> {
        let interaction = self.interaction.clone();
        let (widget, choices) = self.into_widget();
        resolve_prompt(interaction.interact(widget, []), |value| {
            resolve_one(&value, choices)
        })
    }

    fn into_widget(self) -> (SearchSelect, Vec<Choice<T>>) {
        let mut widget = SearchSelect::new(self.id, choice_items(&self.choices))
            .with_page_size(self.config.list.page_size)
            .with_wrap(self.config.list.wrap);
        if let Some(header) = self.config.list.header {
            widget = widget.with_header(header);
        }
        if let Some(prompt) = self.config.prompt {
            widget = widget.with_prompt(prompt);
        }
        if let Some(placeholder) = self.config.placeholder {
            widget = widget.with_placeholder(placeholder);
        }
        if let Some(selected) = self.config.selected {
            widget = widget.with_selected_match_index(selected);
        }
        (widget, self.choices)
    }
}

impl<T> Configurable for SearchPrompt<T> {
    type Config = SearchConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.config = config;
        self
    }
}

#[derive(Clone, Debug)]
pub struct TextPrompt {
    prompt: String,
    config: TextConfig,
    interaction: Interaction,
}

impl TextPrompt {
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            config: TextConfig::default(),
            interaction: Interaction::default(),
        }
    }

    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.config = self.config.id(id);
        self
    }

    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.config = self.config.placeholder(placeholder);
        self
    }

    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.config = self.config.value(value);
        self
    }

    #[must_use]
    pub fn validator(
        mut self,
        validator: impl Fn(&str) -> std::result::Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.config = self.config.validator(validator);
        self
    }

    #[must_use]
    pub fn interaction(mut self, interaction: Interaction) -> Self {
        self.interaction = interaction;
        self
    }

    pub fn interact(self) -> Result<PromptOutcome<String>> {
        let mut widget = TextInput::new(self.config.id.unwrap_or_else(|| "text".to_owned()))
            .with_prompt(self.prompt);
        if let Some(placeholder) = self.config.placeholder {
            widget = widget.with_placeholder(placeholder);
        }
        if let Some(value) = self.config.value {
            widget = widget.with_value(value);
        }
        if let Some(validator) = self.config.validator {
            widget = widget.with_validator(move |value| validator(value));
        }
        resolve_prompt(self.interaction.interact(widget, []), |value| match value {
            Value::String(value) => Ok(value),
            _ => Err(Error::unexpected("text")),
        })
    }
}

impl Configurable for TextPrompt {
    type Config = TextConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.config = config;
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

fn resolve_prompt<T>(
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

fn resolve_review<T, A>(
    value: Value,
    choices: Vec<ReviewChoice<T>>,
    actions: Vec<ReviewPromptAction<A>>,
) -> Result<ReviewOutcome<T, A>> {
    let Value::Object(mut output) = value else {
        return Err(Error::unexpected("a review outcome"));
    };
    let exit = output
        .remove("exit")
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| Error::unexpected("a review exit"))?;
    if exit == "leave" {
        return Ok(ReviewOutcome {
            exit: ReviewExit::Leave,
            accepted_items: None,
        });
    }

    let exit = match exit.as_str() {
        "submit" => ReviewExit::Submit,
        "action" => {
            let index = output
                .remove("action")
                .and_then(|value| value.as_str().and_then(|value| value.parse::<usize>().ok()))
                .ok_or_else(|| Error::unexpected("a review action"))?;
            let action = actions
                .into_iter()
                .nth(index)
                .ok_or_else(|| Error::unexpected("a known review action"))?;
            ReviewExit::Action(action.value)
        },
        _ => return Err(Error::unexpected("a known review exit")),
    };

    let Some(Value::List(rows)) = output.remove("rows") else {
        return Err(Error::unexpected("review rows"));
    };
    if rows.len() != choices.len() {
        return Err(Error::unexpected("all review rows"));
    }
    let mut choices = choices.into_iter().map(Some).collect::<Vec<_>>();
    let mut accepted_items = Vec::with_capacity(rows.len());
    for row in rows {
        let Value::Object(mut row) = row else {
            return Err(Error::unexpected("a review row"));
        };
        let index = row
            .remove("value")
            .and_then(|value| value.as_str().and_then(|value| value.parse::<usize>().ok()))
            .ok_or_else(|| Error::unexpected("a known review item"))?;
        let state = row
            .remove("state")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| Error::unexpected("a review state"))?;
        let state = ReviewState::try_from(state.as_str())
            .map_err(|_error| Error::unexpected("a known review state"))?;
        let choice = choices
            .get_mut(index)
            .and_then(Option::take)
            .ok_or_else(|| Error::unexpected("distinct known review items"))?;
        accepted_items.push(Reviewed {
            value: choice.value,
            state,
            changed: state != choice.state,
        });
    }
    Ok(ReviewOutcome {
        exit,
        accepted_items: Some(accepted_items),
    })
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
        assert_eq!(select.config.selected, Some(2));

        let multi = multi_select::<()>("multi").with_config(
            MultiSelectConfig::default()
                .header("choose several")
                .wrap(false)
                .page_size(5)
                .selected(1)
                .checked_indices([0, 2]),
        );
        assert_eq!(multi.config.selected, Some(1));
        assert_eq!(multi.config.checked, [0, 2]);

        let search = search::<()>("search").with_config(
            SearchConfig::default()
                .header("find one")
                .wrap(false)
                .page_size(6)
                .prompt("query: ")
                .placeholder("type here")
                .selected(3),
        );
        assert_eq!(search.config.list.header.as_deref(), Some("find one"));
        assert_eq!(search.config.prompt.as_deref(), Some("query: "));
        assert_eq!(search.config.placeholder.as_deref(), Some("type here"));
        assert_eq!(search.config.selected, Some(3));

        let text = text("value: ").with_config(
            TextConfig::default()
                .id("value")
                .value("initial")
                .placeholder("required")
                .validator(|value| {
                    (!value.is_empty())
                        .then_some(())
                        .ok_or_else(|| "empty".to_owned())
                }),
        );
        assert_eq!(text.config.id.as_deref(), Some("value"));
        assert_eq!(text.config.value.as_deref(), Some("initial"));
        assert!(text.config.validator.is_some());

        let review = review::<()>("review").with_config(
            ReviewConfig::default()
                .header("review")
                .wrap(false)
                .page_size(7)
                .selected(2)
                .show_removed(false),
        );
        assert_eq!(review.core.config.list.header.as_deref(), Some("review"));
        assert!(!review.core.config.list.wrap);
        assert_eq!(review.core.config.list.page_size, 7);
        assert_eq!(review.core.config.selected, Some(2));
        assert!(!review.core.config.show_removed);
    }

    #[test]
    fn typed_choice_is_recovered() {
        let prompt = select("shell").choice("Bash", 10).choice("Zsh", 20);
        let (_widget, choices) = prompt.into_widget();
        assert_eq!(resolve_one(&Value::from("1"), choices).unwrap(), 20);
    }

    #[test]
    fn typed_choices_are_recovered_in_selection_order() {
        let prompt = multi_select("shells").choice("Bash", 10).choice("Zsh", 20);
        let (_widget, choices) = prompt.into_widget();
        assert_eq!(
            resolve_many(
                Value::List(vec![Value::from("0"), Value::from("1")]),
                choices
            )
            .unwrap(),
            vec![10, 20]
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
    fn review_leave_discards_provisional_items() {
        let value = Value::Object(std::collections::BTreeMap::from([
            ("exit".to_owned(), Value::from("leave")),
            ("rows".to_owned(), Value::List(Vec::new())),
        ]));
        let outcome = resolve_review::<i32, ()>(
            value,
            vec![ReviewChoice {
                label: "one".to_owned(),
                value: 1,
                state: ReviewState::Unconfirmed,
            }],
            Vec::new(),
        )
        .unwrap();
        assert_eq!(outcome.exit(), &ReviewExit::Leave);
        assert!(outcome.accepted_items().is_none());
    }

    #[test]
    fn review_action_recovers_typed_action_and_changed_items() {
        let row = Value::Object(std::collections::BTreeMap::from([
            ("value".to_owned(), Value::from("0")),
            ("state".to_owned(), Value::from("confirmed")),
        ]));
        let value = Value::Object(std::collections::BTreeMap::from([
            ("exit".to_owned(), Value::from("action")),
            ("action".to_owned(), Value::from("0")),
            ("rows".to_owned(), Value::List(vec![row])),
        ]));
        let outcome = resolve_review(
            value,
            vec![ReviewChoice {
                label: "one".to_owned(),
                value: 42,
                state: ReviewState::Unconfirmed,
            }],
            vec![ReviewPromptAction {
                key: 'g',
                help: "regenerate".to_owned(),
                value: "regen",
            }],
        )
        .unwrap();
        assert_eq!(outcome.exit(), &ReviewExit::Action("regen"));
        let items = outcome.accepted_items().unwrap();
        assert_eq!(items[0].value(), &42);
        assert_eq!(items[0].state(), ReviewState::Confirmed);
        assert!(items[0].changed());
    }

    #[test]
    fn review_rejects_reserved_and_duplicate_action_keys() {
        let reserved = review::<()>("review").action('J', "jump", 1);
        assert_eq!(
            validate_review_actions(&reserved.actions)
                .unwrap_err()
                .kind(),
            crate::ErrorKind::InvalidConfiguration
        );

        let duplicate = review::<()>("review")
            .action('g', "generate", 1)
            .action('G', "go", 2);
        assert_eq!(
            validate_review_actions(&duplicate.actions)
                .unwrap_err()
                .kind(),
            crate::ErrorKind::InvalidConfiguration
        );
    }

    #[test]
    fn action_free_review_uses_the_ordinary_prompt_outcome() {
        let outcome = review("review")
            .item("one", 42, ReviewState::Unconfirmed)
            .interaction(crate::advanced::scripted_interaction([vec![
                bang_core::Event::key(bang_core::Key::Enter),
            ]]))
            .interact()
            .unwrap();

        let PromptOutcome::Submit(items) = outcome else {
            panic!("review should submit");
        };
        assert_eq!(items[0].value(), &42);
    }

    #[test]
    fn scripted_interaction_drives_typed_prompts_without_a_terminal() {
        let interaction = crate::advanced::scripted_interaction([
            vec![
                bang_core::Event::key(bang_core::Key::Down),
                bang_core::Event::key(bang_core::Key::Enter),
            ],
            vec![
                bang_core::Event::char('A'),
                bang_core::Event::char('d'),
                bang_core::Event::char('a'),
                bang_core::Event::key(bang_core::Key::Enter),
            ],
        ]);
        let selected = select("shell")
            .choice("Bash", 10)
            .choice("Zsh", 20)
            .interaction(interaction.clone())
            .interact()
            .unwrap();
        let text = TextPrompt::new("name")
            .interaction(interaction)
            .interact()
            .unwrap();
        assert_eq!(selected, PromptOutcome::Submit(20));
        assert_eq!(text, PromptOutcome::Submit("Ada".to_owned()));
    }

    #[test]
    fn cancellation_leaves_each_ordinary_typed_prompt() {
        let interaction = crate::advanced::scripted_interaction([
            vec![bang_core::Event::key(bang_core::Key::Esc)],
            vec![bang_core::Event::key(bang_core::Key::Esc)],
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
                .interaction(interaction.clone())
                .interact()
                .unwrap(),
            PromptOutcome::Leave
        );
        assert_eq!(
            search("shell")
                .choice("Bash", 10)
                .interaction(interaction.clone())
                .interact()
                .unwrap(),
            PromptOutcome::Leave
        );
        assert_eq!(
            text("name").interaction(interaction).interact().unwrap(),
            PromptOutcome::Leave
        );
    }

    #[test]
    fn input_ending_without_a_decision_remains_an_error() {
        let interaction = crate::advanced::scripted_interaction([Vec::new()]);
        let error = text("name")
            .interaction(interaction)
            .interact()
            .unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::InputEnded);
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
}
