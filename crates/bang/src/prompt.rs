// SPDX-License-Identifier: EUPL-1.2

use std::{
    fmt,
    rc::Rc,
};

use bang_core::{
    Value,
    widgets::{
        MultiSelect,
        ReviewActionBinding,
        ReviewList,
        ReviewState,
        SearchSelect,
        Select,
        SelectItem,
        TextInput,
    },
};

use crate::{
    Error,
    Interaction,
    Result,
    interaction::Summary,
};

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

/// Ask the user to filter and pick one of several choices under `header`.
#[must_use]
pub fn search<T>(header: impl Into<String>) -> SearchPrompt<T> {
    SearchPrompt::new(header)
}

/// Ask the user to confirm, change or reject a list of items under `header`.
#[must_use]
pub fn review<T>(header: impl Into<String>) -> ReviewPrompt<T> {
    ReviewPrompt::new(header)
}

/// Ask the user for a line of text shown after `prompt`.
#[must_use]
pub fn text(prompt: impl Into<String>) -> TextPrompt {
    TextPrompt::new(prompt)
}

#[derive(Clone, Debug)]
struct ListConfig {
    header:    Option<String>,
    wrap:      bool,
    page_size: usize,
    selected:  Option<usize>,
    summary:   Option<bool>,
}

impl Default for ListConfig {
    fn default() -> Self {
        Self {
            header:    None,
            wrap:      true,
            page_size: DEFAULT_PAGE_SIZE,
            selected:  None,
            summary:   None,
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

        /// Choose whether submitting leaves a one-line summary in the
        /// scrollback, overriding the interaction's setting.
        #[must_use]
        pub const fn summary(mut self, summary: bool) -> Self {
            self.$($list).+.summary = Some(summary);
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
    list_options!(
        "Start with the choice at `selected` highlighted, counting from zero.",
        list
    );
}

/// Presentation settings for [`MultiSelectPrompt`].
#[derive(Clone, Debug, Default)]
pub struct MultiSelectConfig {
    list:    ListConfig,
    checked: Vec<usize>,
}

impl MultiSelectConfig {
    list_options!(
        "Start with the choice at `selected` highlighted, counting from zero.",
        list
    );

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

/// Presentation settings for [`SearchPrompt`].
#[derive(Clone, Debug, Default)]
pub struct SearchConfig {
    list:        ListConfig,
    prompt:      Option<String>,
    placeholder: Option<String>,
}

impl SearchConfig {
    list_options!(
        "Start with the choice at `selected` highlighted, counting from zero. The query starts \
         empty, so every choice is listed.",
        list
    );

    /// Set the text shown before the query.
    #[must_use]
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Set the hint shown while the query is empty.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }
}

type Validator = dyn Fn(&str) -> std::result::Result<(), String> + 'static;

/// Presentation settings for [`TextPrompt`].
#[derive(Clone, Default)]
pub struct TextConfig {
    id:                 Option<String>,
    value:              Option<String>,
    placeholder:        Option<String>,
    validator:          Option<Rc<Validator>>,
    pub(crate) summary: Option<bool>,
}

impl TextConfig {
    /// Choose whether submitting leaves a one-line summary in the scrollback,
    /// overriding the interaction's setting.
    #[must_use]
    pub const fn summary(mut self, summary: bool) -> Self {
        self.summary = Some(summary);
        self
    }

    /// Set the widget id. Defaults to `text`.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Start with `value` already typed.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Set the hint shown while nothing is typed.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Reject submission with the returned message until `validator` accepts
    /// the text.
    #[must_use]
    pub fn validator(
        mut self,
        validator: impl Fn(&str) -> std::result::Result<(), String> + 'static,
    ) -> Self {
        self.validator = Some(Rc::new(validator));
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
            .field("summary", &self.summary)
            .finish()
    }
}

/// Presentation settings for [`ReviewPrompt`] and [`ReviewPromptWithActions`].
#[derive(Clone, Debug)]
pub struct ReviewConfig {
    list:         ListConfig,
    show_removed: bool,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            list:         ListConfig::default(),
            show_removed: true,
        }
    }
}

impl ReviewConfig {
    list_options!(
        "Start with the item at `selected` highlighted, counting from zero.",
        list
    );

    /// Choose whether removed items stay in view. Defaults to `true`.
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

impl<T> PromptOutcome<T> {
    /// Whether the user submitted a value.
    #[must_use]
    pub const fn is_submit(&self) -> bool {
        matches!(self, Self::Submit(_))
    }

    /// Whether the user left without submitting.
    #[must_use]
    pub const fn is_leave(&self) -> bool {
        matches!(self, Self::Leave)
    }

    /// The submitted value, or `None` when the user left.
    ///
    /// ```
    /// use bang::PromptOutcome;
    ///
    /// assert_eq!(PromptOutcome::Submit(3).into_option(), Some(3));
    /// assert_eq!(PromptOutcome::<u8>::Leave.into_option(), None);
    /// ```
    #[must_use]
    pub fn into_option(self) -> Option<T> {
        match self {
            Self::Submit(value) => Some(value),
            Self::Leave => None,
        }
    }

    /// The submitted value, or `default` when the user left.
    ///
    /// ```
    /// use bang::PromptOutcome;
    ///
    /// assert_eq!(PromptOutcome::Submit(3).unwrap_or(0), 3);
    /// assert_eq!(PromptOutcome::Leave.unwrap_or(0), 0);
    /// ```
    #[must_use]
    pub fn unwrap_or(self, default: T) -> T {
        self.into_option().unwrap_or(default)
    }

    /// The submitted value, or an
    /// [`ErrorKind::Cancelled`](crate::ErrorKind::Cancelled) error when the
    /// user left.
    ///
    /// This lets a command that cannot continue without an answer end through
    /// `?`. Applications built on `climax` exit with status 130 on it, the
    /// same as an unhandled Ctrl-C.
    ///
    /// ```
    /// use bang::{
    ///     ErrorKind,
    ///     PromptOutcome,
    /// };
    ///
    /// assert_eq!(PromptOutcome::Submit(3).or_cancel().unwrap(), 3);
    /// let error = PromptOutcome::<u8>::Leave.or_cancel().unwrap_err();
    /// assert_eq!(error.kind(), ErrorKind::Cancelled);
    /// ```
    pub fn or_cancel(self) -> Result<T> {
        self.into_option().ok_or_else(Error::cancelled)
    }
}

/// How a review interaction ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewExit<A> {
    /// The user accepted the review.
    Submit,
    /// The user left without accepting, discarding provisional edits.
    Leave,
    /// The user triggered an intrinsic action, carrying its value.
    Action(A),
}

/// An item returned from an accepted review.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reviewed<T> {
    value:   T,
    state:   ReviewState,
    changed: bool,
}

impl<T> Reviewed<T> {
    /// The value supplied when the item was added.
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The state the user left the item in.
    #[must_use]
    pub const fn state(&self) -> ReviewState {
        self.state
    }

    /// Whether the state differs from the one the item started in.
    #[must_use]
    pub const fn changed(&self) -> bool {
        self.changed
    }

    /// Split into the value, the final state and whether it changed.
    #[must_use]
    pub fn into_parts(self) -> (T, ReviewState, bool) {
        (self.value, self.state, self.changed)
    }
}

/// The exit and, when accepted, the resulting review items.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewOutcome<T, A> {
    exit:           ReviewExit<A>,
    accepted_items: Option<Vec<Reviewed<T>>>,
}

impl<T, A> ReviewOutcome<T, A> {
    /// How the review ended.
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

    /// Split into the exit and the accepted items.
    #[must_use]
    pub fn into_parts(self) -> (ReviewExit<A>, Option<Vec<Reviewed<T>>>) {
        (self.exit, self.accepted_items)
    }
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

#[derive(Clone, Debug)]
struct ReviewPromptCore<T> {
    id:          String,
    choices:     Vec<ReviewChoice<T>>,
    config:      ReviewConfig,
    interaction: Interaction,
}

impl<T> ReviewPromptCore<T> {
    #[must_use]
    fn new(header: impl Into<String>) -> Self {
        Self {
            id:          "review".to_owned(),
            choices:     Vec::new(),
            config:      ReviewConfig {
                list: ListConfig::with_header(header),
                ..ReviewConfig::default()
            },
            interaction: Interaction::default(),
        }
    }

    fn with_config(&mut self, config: ReviewConfig) {
        let header = self.config.list.header.take();
        self.config = config;
        self.config.list.inherit_header(header);
    }

    fn into_widget<A>(
        self,
        actions: &[ReviewPromptAction<A>],
    ) -> Result<(ReviewList, Vec<ReviewChoice<T>>)> {
        require_choices("review", self.choices.len())?;
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
        if let Some(selected) = self.config.list.selected {
            widget = widget.with_selected_index(selected);
        }
        Ok((widget, self.choices))
    }
}

macro_rules! review_prompt_methods {
    () => {
        /// Add an item shown as `label`, starting in `state`, that is returned as
        /// `value`.
        #[must_use]
        pub fn item(mut self, label: impl Into<String>, value: T, state: ReviewState) -> Self {
            self.core.choices.push(ReviewChoice {
                label: label.into(),
                value,
                state,
            });
            self
        }

        prompt_options!(core);
        list_options!(
            "Start with the item at `selected` highlighted, counting from zero.",
            core.config.list
        );

        /// Choose whether removed items stay in view. Defaults to `true`.
        #[must_use]
        pub const fn show_removed(mut self, show_removed: bool) -> Self {
            self.core.config.show_removed = show_removed;
            self
        }
    };
}

/// A review prompt with ordinary submit/leave outcomes.
///
/// The prompt needs at least one item. Create it with [`review`].
#[derive(Clone, Debug)]
pub struct ReviewPrompt<T> {
    core: ReviewPromptCore<T>,
}

impl<T> ReviewPrompt<T> {
    /// Create a review prompt with `header` above the items.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            core: ReviewPromptCore::new(header),
        }
    }

    review_prompt_methods!();

    /// Add the first intrinsic review action and transition to an
    /// action-bearing review prompt.
    ///
    /// `key` is matched case-insensitively and must not be one of the keys the
    /// review reserves for itself, which are `j`, `k`, `r`, `y`, `c`, `x`, `n`,
    /// `u` and space. Reserved or repeated keys make `interact` fail with
    /// `ErrorKind::InvalidConfiguration`.
    #[must_use]
    pub fn action<A>(
        self,
        key: char,
        help: impl Into<String>,
        value: A,
    ) -> ReviewPromptWithActions<T, A> {
        ReviewPromptWithActions {
            core:    self.core,
            actions: vec![ReviewPromptAction {
                key,
                help: help.into(),
                value,
            }],
        }
    }

    /// Run the prompt to completion.
    ///
    /// Returns `Ok(PromptOutcome::Leave)` when the user leaves or cancels
    /// without accepting the review. Fails with
    /// `ErrorKind::InvalidConfiguration` if no item was added, with
    /// `ErrorKind::InputEnded` if input ends first,
    /// and with `ErrorKind::InteractionUnavailable` if the driver's terminal
    /// isn't interactive.
    pub fn interact(self) -> Result<PromptOutcome<Vec<Reviewed<T>>>> {
        let interaction = self.core.interaction.clone();
        let header = self.core.config.list.header.clone();
        let summary = self.core.config.list.summary;
        let (widget, choices) = self
            .core
            .into_widget(&[] as &[ReviewPromptAction<std::convert::Infallible>])?;
        let value = match interaction.interact_named(
            header.as_deref(),
            widget,
            [],
            Summary::new(summary, &review_summary),
        ) {
            Ok(value) => value,
            Err(error) if error.kind() == crate::ErrorKind::Cancelled => {
                return Ok(PromptOutcome::Leave);
            },
            Err(error) => return Err(error),
        };
        let outcome: ReviewOutcome<T, std::convert::Infallible> =
            resolve_review(value, choices, Vec::new())?;
        let (exit, items) = outcome.into_parts();
        match exit {
            ReviewExit::Submit => {
                items
                    .map(PromptOutcome::Submit)
                    .ok_or_else(|| Error::unexpected("accepted review items"))
            },
            ReviewExit::Leave => Ok(PromptOutcome::Leave),
            ReviewExit::Action(never) => match never {},
        }
    }
}

impl<T> Configurable for ReviewPrompt<T> {
    type Config = ReviewConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.core.with_config(config);
        self
    }
}

/// A review prompt with one or more typed intrinsic actions.
///
/// It is reached through [`ReviewPrompt::action`]. The prompt needs at least
/// one item.
#[derive(Clone, Debug)]
pub struct ReviewPromptWithActions<T, A> {
    core:    ReviewPromptCore<T>,
    actions: Vec<ReviewPromptAction<A>>,
}

impl<T, A> ReviewPromptWithActions<T, A> {
    review_prompt_methods!();

    /// Add another intrinsic action triggered by `key` and listed in the help
    /// as `help`.
    #[must_use]
    pub fn action(mut self, key: char, help: impl Into<String>, value: A) -> Self {
        self.actions.push(ReviewPromptAction {
            key,
            help: help.into(),
            value,
        });
        self
    }

    /// Run the prompt to completion.
    ///
    /// The outcome's `exit()` is `ReviewExit::Leave` when the user leaves or
    /// cancels without accepting the review, and `ReviewExit::Action` when an
    /// intrinsic action fires. Fails with `ErrorKind::InvalidConfiguration` if
    /// no item was added or an action key is reserved or repeated, with
    /// `ErrorKind::InputEnded` if input ends first, and with
    /// `ErrorKind::InteractionUnavailable` if the driver's terminal isn't
    /// interactive.
    pub fn interact(self) -> Result<ReviewOutcome<T, A>> {
        validate_review_actions(&self.actions)?;
        let interaction = self.core.interaction.clone();
        let header = self.core.config.list.header.clone();
        let summary = self.core.config.list.summary;
        let (widget, choices) = self.core.into_widget(&self.actions)?;
        match interaction.interact_named(
            header.as_deref(),
            widget,
            [],
            Summary::new(summary, &review_summary),
        ) {
            Ok(value) => resolve_review(value, choices, self.actions),
            Err(error) if error.kind() == crate::ErrorKind::Cancelled => {
                Ok(ReviewOutcome {
                    exit:           ReviewExit::Leave,
                    accepted_items: None,
                })
            },
            Err(error) => Err(error),
        }
    }
}

impl<T, A> Configurable for ReviewPromptWithActions<T, A> {
    type Config = ReviewConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.core.with_config(config);
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
    key:   char,
    help:  String,
    value: A,
}

/// A prompt that picks one of several choices.
///
/// The prompt needs at least one choice. Create it with [`select`].
#[derive(Clone, Debug)]
pub struct SelectPrompt<T> {
    id:          String,
    choices:     Vec<Choice<T>>,
    config:      SelectConfig,
    interaction: Interaction,
}

impl<T> SelectPrompt<T> {
    /// Create a select prompt with `header` above the choices.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            id:          "select".to_owned(),
            choices:     Vec::new(),
            config:      SelectConfig {
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
        let header = self.config.list.header.clone();
        let summary = self.config.list.summary;
        let (widget, choices) = self.into_widget()?;
        let labels = choice_labels(&choices);
        resolve_prompt(
            interaction.interact_named(
                header.as_deref(),
                widget,
                [],
                Summary::new(summary, &|value| one_label(value, &labels)),
            ),
            |value| resolve_one(&value, choices),
        )
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
    id:          String,
    choices:     Vec<Choice<T>>,
    config:      MultiSelectConfig,
    interaction: Interaction,
}

impl<T> MultiSelectPrompt<T> {
    /// Create a multi-select prompt with `header` above the choices.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            id:          "multi_select".to_owned(),
            choices:     Vec::new(),
            config:      MultiSelectConfig {
                list:    ListConfig::with_header(header),
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
        let header = self.config.list.header.clone();
        let summary = self.config.list.summary;
        let (widget, choices) = self.into_widget()?;
        let labels = choice_labels(&choices);
        resolve_prompt(
            interaction.interact_named(
                header.as_deref(),
                widget,
                [],
                Summary::new(summary, &|value| many_labels(value, &labels)),
            ),
            |value| resolve_many(value, choices),
        )
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

/// A prompt that filters and picks one of several choices.
///
/// The prompt needs at least one choice. Create it with [`search`].
#[derive(Clone, Debug)]
pub struct SearchPrompt<T> {
    id:          String,
    choices:     Vec<Choice<T>>,
    config:      SearchConfig,
    interaction: Interaction,
}

impl<T> SearchPrompt<T> {
    /// Create a search prompt with `header` above the choices.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            id:          "search".to_owned(),
            choices:     Vec::new(),
            config:      SearchConfig {
                list:        ListConfig::with_header(header),
                prompt:      None,
                placeholder: None,
            },
            interaction: Interaction::default(),
        }
    }

    choice_option!();
    prompt_options!();
    list_options!(
        "Start with the choice at `selected` highlighted, counting from zero. The query starts \
         empty, so every choice is listed.",
        config.list
    );

    /// Set the text shown before the query.
    #[must_use]
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.config.prompt = Some(prompt.into());
        self
    }

    /// Set the hint shown while the query is empty.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.config.placeholder = Some(placeholder.into());
        self
    }

    /// Run the prompt to completion.
    ///
    /// Returns `Ok(PromptOutcome::Leave)` when the user leaves without
    /// submitting. Fails with `ErrorKind::InvalidConfiguration` if no choice
    /// was added, with `ErrorKind::InputEnded` if input ends first, and with
    /// `ErrorKind::InteractionUnavailable` if the driver's terminal isn't
    /// interactive.
    pub fn interact(self) -> Result<PromptOutcome<T>> {
        let interaction = self.interaction.clone();
        let header = self.config.list.header.clone();
        let summary = self.config.list.summary;
        let (widget, choices) = self.into_widget()?;
        let labels = choice_labels(&choices);
        resolve_prompt(
            interaction.interact_named(
                header.as_deref(),
                widget,
                [],
                Summary::new(summary, &|value| one_label(value, &labels)),
            ),
            |value| resolve_one(&value, choices),
        )
    }

    fn into_widget(self) -> Result<(SearchSelect, Vec<Choice<T>>)> {
        require_choices("search", self.choices.len())?;
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
        if let Some(selected) = self.config.list.selected {
            widget = widget.with_selected_match_index(selected);
        }
        Ok((widget, self.choices))
    }
}

impl<T> Configurable for SearchPrompt<T> {
    type Config = SearchConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        let header = self.config.list.header.take();
        self.config = config;
        self.config.list.inherit_header(header);
        self
    }
}

/// A prompt that reads a line of text.
///
/// Create it with [`text`].
#[derive(Clone, Debug)]
pub struct TextPrompt {
    prompt:      String,
    config:      TextConfig,
    interaction: Interaction,
}

impl TextPrompt {
    /// Create a text prompt shown as `prompt`.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt:      prompt.into(),
            config:      TextConfig::default(),
            interaction: Interaction::default(),
        }
    }

    /// Set the widget id. Defaults to `text`.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.config = self.config.id(id);
        self
    }

    /// Set the hint shown while nothing is typed.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.config = self.config.placeholder(placeholder);
        self
    }

    /// Start with `value` already typed.
    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.config = self.config.value(value);
        self
    }

    /// Reject submission with the returned message until `validator` accepts
    /// the text.
    #[must_use]
    pub fn validator(
        mut self,
        validator: impl Fn(&str) -> std::result::Result<(), String> + 'static,
    ) -> Self {
        self.config = self.config.validator(validator);
        self
    }

    /// Choose whether submitting leaves a one-line summary in the scrollback,
    /// overriding the interaction's setting.
    #[must_use]
    pub const fn summary(mut self, summary: bool) -> Self {
        self.config.summary = Some(summary);
        self
    }

    /// Run the prompt on `interaction` instead of the live terminal.
    #[must_use]
    pub fn interaction(mut self, interaction: Interaction) -> Self {
        self.interaction = interaction;
        self
    }

    /// Run the prompt to completion.
    ///
    /// Returns `Ok(PromptOutcome::Leave)` when the user leaves without
    /// submitting. Fails with `ErrorKind::InputEnded` if input ends first,
    /// and with `ErrorKind::InteractionUnavailable` if the driver's terminal
    /// isn't interactive.
    pub fn interact(self) -> Result<PromptOutcome<String>> {
        let prompt = self.prompt.clone();
        let summary = self.config.summary;
        let widget = text_widget(self.prompt, self.config, "text", None);
        resolve_prompt(
            self.interaction.interact_named(
                Some(&prompt),
                widget,
                [],
                Summary::new(summary, &|value| value.as_str().map(str::to_owned)),
            ),
            resolve_text,
        )
    }
}

pub(crate) fn text_widget(
    prompt: String,
    config: TextConfig,
    default_id: &str,
    mask: Option<char>,
) -> TextInput {
    let mut widget =
        TextInput::new(config.id.unwrap_or_else(|| default_id.to_owned())).with_prompt(prompt);
    if let Some(placeholder) = config.placeholder {
        widget = widget.with_placeholder(placeholder);
    }
    if let Some(value) = config.value {
        widget = widget.with_value(value);
    }
    if let Some(validator) = config.validator {
        widget = widget.with_validator(move |value| validator(value));
    }
    if let Some(mask) = mask {
        widget = widget.with_mask(mask);
    }
    widget
}

pub(crate) fn resolve_text(value: Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value),
        _ => Err(Error::unexpected("text")),
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

fn choice_labels<T>(choices: &[Choice<T>]) -> Vec<String> {
    choices.iter().map(|choice| choice.label.clone()).collect()
}

fn one_label(value: &Value, labels: &[String]) -> Option<String> {
    let index = value.as_str()?.parse::<usize>().ok()?;
    labels.get(index).cloned()
}

fn many_labels(value: &Value, labels: &[String]) -> Option<String> {
    let picked = value
        .as_list()?
        .iter()
        .map(|value| one_label(value, labels))
        .collect::<Option<Vec<_>>>()?;
    Some(if picked.is_empty() {
        "none".to_owned()
    } else {
        picked.join(", ")
    })
}

fn review_summary(value: &Value) -> Option<String> {
    let output = value.as_object()?;
    if output.get("exit")?.as_str()? != "submit" {
        return None;
    }
    let rows = output.get("rows")?.as_list()?;
    let confirmed = rows
        .iter()
        .filter(|row| {
            row.as_object()
                .and_then(|row| row.get("state"))
                .and_then(Value::as_str)
                == Some("confirmed")
        })
        .count();
    Some(format!("{confirmed} of {} confirmed", rows.len()))
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
            exit:           ReviewExit::Leave,
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
    use std::cell::RefCell;

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
        assert_eq!(search.config.list.selected, Some(3));

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
        assert_eq!(review.core.config.list.selected, Some(2));
        assert!(!review.core.config.show_removed);
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
                key:   'g',
                help:  "regenerate".to_owned(),
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
    fn cloned_text_prompt_accepts_local_validator() {
        let values = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&values);
        let prompt = text("name")
            .validator(move |value| {
                seen.borrow_mut().push(value.to_owned());
                (!value.is_empty())
                    .then_some(())
                    .ok_or_else(|| "empty".to_owned())
            })
            .interaction(crate::advanced::scripted_interaction([
                vec![
                    bang_core::Event::key(bang_core::Key::Enter),
                    bang_core::Event::char('A'),
                    bang_core::Event::key(bang_core::Key::Enter),
                ],
                vec![
                    bang_core::Event::char('B'),
                    bang_core::Event::key(bang_core::Key::Enter),
                ],
            ]));
        let cloned = prompt.clone();

        assert_eq!(
            prompt.interact().unwrap(),
            PromptOutcome::Submit("A".to_owned())
        );
        assert_eq!(
            cloned.interact().unwrap(),
            PromptOutcome::Submit("B".to_owned())
        );
        assert_eq!(*values.borrow(), ["", "A", "B"]);
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
    fn cancellation_leaves_the_review_prompts() {
        let control_c = bang_core::Event::Key(bang_core::KeyEvent::with_modifiers(
            bang_core::Key::Char('c'),
            bang_core::Modifiers::CONTROL,
        ));
        let interaction =
            crate::advanced::scripted_interaction([vec![control_c.clone()], vec![control_c]]);

        assert_eq!(
            review("files")
                .item("one", 1, ReviewState::Unconfirmed)
                .interaction(interaction.clone())
                .interact()
                .unwrap(),
            PromptOutcome::Leave
        );

        let outcome = review("files")
            .item("one", 1, ReviewState::Unconfirmed)
            .action('g', "regenerate", "regen")
            .interaction(interaction)
            .interact()
            .unwrap();
        assert_eq!(outcome.exit(), &ReviewExit::Leave);
        assert!(outcome.accepted_items().is_none());
    }

    #[test]
    fn input_ending_without_a_decision_remains_an_error() {
        let interaction = crate::advanced::scripted_interaction([Vec::new()]);
        let error = text("name")
            .interaction(interaction)
            .interact()
            .unwrap_err();
        assert_eq!(error.kind(), crate::ErrorKind::InputEnded);
        assert_eq!(
            error.to_string(),
            "input ended before \"name\" was answered, so supply the answer or run in a terminal"
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
        assert_eq!(
            error.to_string(),
            "cannot ask \"shell\" because there is no interactive terminal, so pass the answer \
             another way or run in a terminal"
        );
    }

    #[test]
    fn a_trailing_colon_is_dropped_from_the_prompt_name() {
        let error = text("name: ")
            .interaction(Interaction::disabled())
            .interact()
            .unwrap_err();
        assert!(error.to_string().starts_with("cannot ask \"name\" because"));
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

        let search = search::<()>("pick")
            .prompt("query: ")
            .placeholder("type")
            .page_size(5);
        assert_eq!(search.config.prompt.as_deref(), Some("query: "));
        assert_eq!(search.config.placeholder.as_deref(), Some("type"));
        assert_eq!(search.config.list.page_size, 5);

        let review = review::<()>("pick")
            .id("files")
            .show_removed(false)
            .selected(1);
        assert_eq!(review.core.id, "files");
        assert!(!review.core.config.show_removed);
        assert_eq!(review.core.config.list.selected, Some(1));
        let with_actions = review.action('g', "generate", ()).wrap(false).id("again");
        assert_eq!(with_actions.core.id, "again");
        assert!(!with_actions.core.config.list.wrap);
    }

    #[test]
    fn entry_point_text_is_the_header_and_ids_default() {
        let select = select::<()>("pick a shell");
        assert_eq!(select.config.list.header.as_deref(), Some("pick a shell"));
        assert_eq!(select.id, "select");
        assert_eq!(multi_select::<()>("x").id, "multi_select");
        assert_eq!(search::<()>("x").id, "search");
        assert_eq!(review::<()>("x").core.id, "review");
    }

    #[test]
    fn with_config_keeps_the_entry_point_header_unless_overridden() {
        let kept = select::<()>("original").with_config(SelectConfig::default().page_size(3));
        assert_eq!(kept.config.list.header.as_deref(), Some("original"));
        assert_eq!(kept.config.list.page_size, 3);

        let replaced =
            review::<()>("original").with_config(ReviewConfig::default().header("replacement"));
        assert_eq!(
            replaced.core.config.list.header.as_deref(),
            Some("replacement")
        );
    }

    #[test]
    fn every_list_prompt_rejects_zero_choices() {
        let kinds = [
            select::<i32>("x").interact().unwrap_err().kind(),
            multi_select::<i32>("x").interact().unwrap_err().kind(),
            search::<i32>("x").interact().unwrap_err().kind(),
            review::<i32>("x").interact().unwrap_err().kind(),
            review::<i32>("x")
                .action('g', "generate", 1)
                .interact()
                .unwrap_err()
                .kind(),
        ];
        assert!(
            kinds
                .iter()
                .all(|kind| *kind == crate::ErrorKind::InvalidConfiguration)
        );
    }
}
