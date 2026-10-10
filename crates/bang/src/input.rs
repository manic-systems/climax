// SPDX-License-Identifier: EUPL-1.2

use std::{
    fmt,
    rc::Rc,
    str::FromStr,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use bang_core::{
    Context,
    Date,
    Event,
    Key,
    Modifiers,
    Reaction,
    Value,
    Widget,
    WidgetId,
    widgets::{
        DatePicker,
        Select,
        SelectItem,
    },
};
use screw::{
    LocalWidgetRef,
    RenderCtx,
    Role,
    Span,
    Spans,
    Stack,
    Surface,
    TickInterest,
    VerticalSize,
    local_widget,
};

use crate::{
    Configurable,
    Error,
    Interaction,
    PromptOutcome,
    Result,
    interaction::Summary,
    prompt::{
        TextConfig,
        resolve_prompt,
        resolve_text,
        text_widget,
    },
};

const DEFAULT_MASK: char = '*';
const SECRET_SUMMARY_WIDTH: usize = 8;

/// Ask the user for a secret, echoing a mask character per typed character.
#[must_use]
pub fn password(prompt: impl Into<String>) -> PasswordPrompt {
    PasswordPrompt::new(prompt)
}

/// Ask the user a yes or no question.
#[must_use]
pub fn confirm(prompt: impl Into<String>) -> ConfirmPrompt {
    ConfirmPrompt::new(prompt)
}

/// Ask the user to pick a calendar date.
#[must_use]
pub fn date(prompt: impl Into<String>) -> DatePrompt {
    DatePrompt::new(prompt)
}

/// Ask the user for a number of type `T`, such as `i64` or `f64`.
#[must_use]
pub fn number<T>(prompt: impl Into<String>) -> NumberPrompt<T>
where
    T: FromStr + 'static,
    T::Err: fmt::Display,
{
    NumberPrompt::new(prompt)
}

/// Builder methods shared by [`PasswordPrompt`] and [`PasswordConfig`],
/// generated once so the two cannot drift apart. `$base` is the path from
/// `self` to the `PasswordConfig`, empty when `self` is one.
macro_rules! password_options {
    ($($base:ident).*) => {
        /// Set the widget id. Defaults to `password`.
        #[must_use]
        pub fn id(mut self, id: impl Into<String>) -> Self {
            self$(.$base)*.text = self$(.$base)*.text.id(id);
            self
        }

        /// Choose whether submitting leaves a one-line summary in the
        /// scrollback, overriding the interaction's setting. The summary shows
        /// the prompt followed by a fixed run of mask characters.
        #[must_use]
        pub fn summary(mut self, summary: bool) -> Self {
            self$(.$base)*.text = self$(.$base)*.text.summary(summary);
            self
        }

        /// Set the character echoed for every typed character. Defaults to `*`.
        #[must_use]
        pub const fn mask(mut self, mask: char) -> Self {
            self$(.$base)*.mask = mask;
            self
        }

        /// Set the hint shown while nothing is typed.
        #[must_use]
        pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
            self$(.$base)*.text = self$(.$base)*.text.placeholder(placeholder);
            self
        }

        /// Reject submission with the returned message until `validator` accepts
        /// the secret.
        #[must_use]
        pub fn validator(
            mut self,
            validator: impl Fn(&str) -> std::result::Result<(), String> + 'static,
        ) -> Self {
            self$(.$base)*.text = self$(.$base)*.text.validator(validator);
            self
        }
    };
}

/// Presentation settings for [`PasswordPrompt`].
#[derive(Clone, Debug)]
pub struct PasswordConfig {
    text: TextConfig,
    mask: char,
}

impl Default for PasswordConfig {
    fn default() -> Self {
        Self {
            text: TextConfig::default(),
            mask: DEFAULT_MASK,
        }
    }
}

impl PasswordConfig {
    password_options!();
}

/// A prompt that reads a line of text without echoing it.
///
/// Create it with [`password`].
#[derive(Clone, Debug)]
pub struct PasswordPrompt {
    prompt:      String,
    config:      PasswordConfig,
    interaction: Interaction,
}

impl PasswordPrompt {
    /// Create a password prompt shown as `prompt`.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt:      prompt.into(),
            config:      PasswordConfig::default(),
            interaction: Interaction::default(),
        }
    }

    password_options!(config);

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
        let summary = self.config.text.summary;
        let hidden = self.config.mask.to_string().repeat(SECRET_SUMMARY_WIDTH);
        let widget = text_widget(
            self.prompt,
            self.config.text,
            "password",
            Some(self.config.mask),
        );
        resolve_prompt(
            self.interaction.interact_named(
                Some(&prompt),
                widget,
                [],
                Summary::new(summary, &|_| Some(hidden.clone())),
            ),
            resolve_text,
        )
    }
}

impl Configurable for PasswordPrompt {
    type Config = PasswordConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.config = config;
        self
    }
}

/// Builder methods shared by [`ConfirmPrompt`] and [`ConfirmConfig`].
/// `$base` is the path from `self` to the `ConfirmConfig`, empty when `self`
/// is one.
macro_rules! confirm_options {
    ($($base:ident).*) => {
        /// Set the widget id. Defaults to `confirm`.
        #[must_use]
        pub fn id(mut self, id: impl Into<String>) -> Self {
            self$(.$base)*.id = id.into();
            self
        }

        /// Choose the answer that is highlighted first. Defaults to `false`,
        /// so that pressing Enter without reading declines.
        #[must_use]
        pub const fn default(mut self, default: bool) -> Self {
            self$(.$base)*.default = default;
            self
        }

        /// Choose whether submitting leaves a one-line summary in the
        /// scrollback, overriding the interaction's setting.
        #[must_use]
        pub const fn summary(mut self, summary: bool) -> Self {
            self$(.$base)*.summary = Some(summary);
            self
        }
    };
}

/// Presentation settings for [`ConfirmPrompt`].
///
/// Its `default` builder method shadows [`Default::default`], so start from
/// [`ConfirmConfig::new`].
#[derive(Clone, Debug)]
pub struct ConfirmConfig {
    id:      String,
    default: bool,
    summary: Option<bool>,
}

impl Default for ConfirmConfig {
    fn default() -> Self {
        Self {
            id:      "confirm".to_owned(),
            default: false,
            summary: None,
        }
    }
}

impl ConfirmConfig {
    /// A config with every setting at its default.
    #[must_use]
    pub fn new() -> Self {
        <Self as Default>::default()
    }

    confirm_options!();
}

/// A yes or no prompt.
///
/// Create it with [`confirm`].
#[derive(Clone, Debug)]
pub struct ConfirmPrompt {
    prompt:      String,
    config:      ConfirmConfig,
    interaction: Interaction,
}

impl ConfirmPrompt {
    /// Create a confirm prompt asking `prompt`.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt:      prompt.into(),
            config:      ConfirmConfig::new(),
            interaction: Interaction::default(),
        }
    }

    confirm_options!(config);

    /// Run the prompt on `interaction` instead of the live terminal.
    #[must_use]
    pub fn interaction(mut self, interaction: Interaction) -> Self {
        self.interaction = interaction;
        self
    }

    /// Run the prompt to completion.
    ///
    /// Pressing `y` or `n` (either case) answers at once. Enter takes the
    /// highlighted answer, which is No unless [`default`](Self::default) says
    /// otherwise. Returns `Ok(PromptOutcome::Leave)` when the user leaves
    /// without answering. Fails with `ErrorKind::InputEnded` if input ends
    /// first, and with `ErrorKind::InteractionUnavailable` if the driver's
    /// terminal isn't interactive.
    pub fn interact(self) -> Result<PromptOutcome<bool>> {
        let select = Select::new(self.config.id, [
            SelectItem::new("Yes", "yes"),
            SelectItem::new("No", "no"),
        ])
        .with_header(self.prompt.clone())
        .with_selected_index(usize::from(!self.config.default));
        let widget = YesNoKeys(select);
        let summary = Summary::new(self.config.summary, &|value| {
            match value.as_str() {
                Some("yes") => Some("yes".to_owned()),
                Some("no") => Some("no".to_owned()),
                _ => None,
            }
        });
        resolve_prompt(
            self.interaction
                .interact_named(Some(&self.prompt), widget, [], summary),
            |value| {
                match value.as_str() {
                    Some("yes") => Ok(true),
                    Some("no") => Ok(false),
                    _ => Err(Error::unexpected("yes or no")),
                }
            },
        )
    }
}

impl Configurable for ConfirmPrompt {
    type Config = ConfirmConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.config = config;
        self
    }
}

/// Builder methods shared by [`DatePrompt`] and [`DateConfig`]. `$base` is the
/// path from `self` to the `DateConfig`, empty when `self` is one.
macro_rules! date_options {
    ($($base:ident).*) => {
        /// Set the widget id. Defaults to `date`.
        #[must_use]
        pub fn id(mut self, id: impl Into<String>) -> Self {
            self$(.$base)*.id = id.into();
            self
        }

        /// Start with `default` selected. Defaults to today in the local time zone, or UTC when the zone is unknown.
        #[must_use]
        pub const fn default(mut self, default: Date) -> Self {
            self$(.$base)*.default = Some(default);
            self
        }

        /// Choose whether submitting leaves a one-line summary in the
        /// scrollback, overriding the interaction's setting.
        #[must_use]
        pub const fn summary(mut self, summary: bool) -> Self {
            self$(.$base)*.summary = Some(summary);
            self
        }
    };
}

/// Presentation settings for [`DatePrompt`].
///
/// Its `default` builder method shadows [`Default::default`], so start from
/// [`DateConfig::new`].
#[derive(Clone, Debug)]
pub struct DateConfig {
    id:      String,
    default: Option<Date>,
    summary: Option<bool>,
}

impl Default for DateConfig {
    fn default() -> Self {
        Self {
            id:      "date".to_owned(),
            default: None,
            summary: None,
        }
    }
}

impl DateConfig {
    /// A config with every setting at its default.
    #[must_use]
    pub fn new() -> Self {
        <Self as Default>::default()
    }

    date_options!();
}

/// A calendar date prompt.
///
/// Create it with [`date`].
#[derive(Clone, Debug)]
pub struct DatePrompt {
    prompt:      String,
    config:      DateConfig,
    interaction: Interaction,
}

impl DatePrompt {
    /// Create a date prompt shown as `prompt`.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt:      prompt.into(),
            config:      DateConfig::new(),
            interaction: Interaction::default(),
        }
    }

    date_options!(config);

    /// Run the prompt on `interaction` instead of the live terminal.
    #[must_use]
    pub fn interaction(mut self, interaction: Interaction) -> Self {
        self.interaction = interaction;
        self
    }

    /// Run the prompt to completion.
    ///
    /// Today and the default date come from the local time zone. Returns
    /// `Ok(PromptOutcome::Leave)` when the user leaves without submitting.
    /// Fails with `ErrorKind::InputEnded` if input ends first, and with
    /// `ErrorKind::InteractionUnavailable` if the driver's terminal isn't
    /// interactive.
    pub fn interact(self) -> Result<PromptOutcome<Date>> {
        let today = today_local();
        let picker =
            DatePicker::new(self.config.id, self.config.default.unwrap_or(today)).with_today(today);
        let widget = Headed::new(self.prompt.clone(), picker);
        let summary = Summary::new(self.config.summary, &|value| {
            value.as_date().map(|date| date.to_string())
        });
        resolve_prompt(
            self.interaction
                .interact_named(Some(&self.prompt), widget, [], summary),
            |value| value.as_date().ok_or_else(|| Error::unexpected("a date")),
        )
    }
}

impl Configurable for DatePrompt {
    type Config = DateConfig;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.config = config;
        self
    }
}

type NumberCheck<T> = dyn Fn(&T) -> std::result::Result<(), String> + 'static;

/// Builder methods shared by [`NumberPrompt`] and [`NumberConfig`]. `$base` is
/// the path from `self` to the `NumberConfig`, empty when `self` is one.
macro_rules! number_options {
    ($($base:ident).*) => {
        /// Set the widget id. Defaults to `number`.
        #[must_use]
        pub fn id(mut self, id: impl Into<String>) -> Self {
            self$(.$base)*.text = self$(.$base)*.text.id(id);
            self
        }

        /// Set the hint shown while nothing is typed.
        #[must_use]
        pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
            self$(.$base)*.text = self$(.$base)*.text.placeholder(placeholder);
            self
        }

        /// Choose whether submitting leaves a one-line summary in the
        /// scrollback, overriding the interaction's setting.
        #[must_use]
        pub fn summary(mut self, summary: bool) -> Self {
            self$(.$base)*.text = self$(.$base)*.text.summary(summary);
            self
        }

        /// Reject submission with the returned message until `validator` accepts
        /// the parsed number.
        #[must_use]
        pub fn validator(
            mut self,
            validator: impl Fn(&T) -> std::result::Result<(), String> + 'static,
        ) -> Self {
            self$(.$base)*.check = Some(Rc::new(validator));
            self
        }
    };
}

/// The `value` builder method of [`NumberPrompt`] and [`NumberConfig`].
macro_rules! number_value_option {
    ($($base:ident).*) => {
        /// Start with `value` already typed.
        #[must_use]
        pub fn value(mut self, value: T) -> Self {
            self$(.$base)*.text = self$(.$base)*.text.value(value.to_string());
            self
        }
    };
}

/// Presentation settings for [`NumberPrompt`].
pub struct NumberConfig<T> {
    text:  TextConfig,
    check: Option<Rc<NumberCheck<T>>>,
}

impl<T> NumberConfig<T> {
    number_options!();
}

impl<T: fmt::Display> NumberConfig<T> {
    number_value_option!();
}

impl<T> Default for NumberConfig<T> {
    fn default() -> Self {
        Self {
            text:  TextConfig::default(),
            check: None,
        }
    }
}

impl<T> Clone for NumberConfig<T> {
    fn clone(&self) -> Self {
        Self {
            text:  self.text.clone(),
            check: self.check.clone(),
        }
    }
}

impl<T> fmt::Debug for NumberConfig<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NumberConfig")
            .field("text", &self.text)
            .field("check", &self.check.as_ref().map(|_| ".."))
            .finish()
    }
}

/// A prompt that reads a number.
///
/// Create it with [`number`]. The typed text is parsed with `T::from_str`
/// after trimming surrounding whitespace, and submission is refused with the
/// parse error until it succeeds. Floating point types accept `nan` and `inf`
/// because they parse them.
pub struct NumberPrompt<T> {
    prompt:      String,
    config:      NumberConfig<T>,
    interaction: Interaction,
}

impl<T> NumberPrompt<T>
where
    T: FromStr + 'static,
    T::Err: fmt::Display,
{
    /// Create a number prompt shown as `prompt`.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt:      prompt.into(),
            config:      NumberConfig::default(),
            interaction: Interaction::default(),
        }
    }

    number_options!(config);

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
    pub fn interact(self) -> Result<PromptOutcome<T>> {
        let check = self.config.check;
        let text = self.config.text.validator(move |text| {
            let parsed = parse_number::<T>(text)?;
            check.as_ref().map_or(Ok(()), |check| check(&parsed))
        });
        let prompt = self.prompt.clone();
        let summary = Summary::new(text.summary, &|value| {
            value.as_str().map(|text| text.trim().to_owned())
        });
        let widget = text_widget(self.prompt, text, "number", None);
        resolve_prompt(
            self.interaction
                .interact_named(Some(&prompt), widget, [], summary),
            |value| {
                let text = resolve_text(value)?;
                parse_number::<T>(&text).map_err(|_message| Error::unexpected("a number"))
            },
        )
    }
}

impl<T: fmt::Display> NumberPrompt<T> {
    number_value_option!(config);
}

impl<T> Configurable for NumberPrompt<T>
where
    T: FromStr + 'static,
    T::Err: fmt::Display,
{
    type Config = NumberConfig<T>;

    fn with_config(mut self, config: Self::Config) -> Self {
        self.config = config;
        self
    }
}

impl<T> Clone for NumberPrompt<T> {
    fn clone(&self) -> Self {
        Self {
            prompt:      self.prompt.clone(),
            config:      self.config.clone(),
            interaction: self.interaction.clone(),
        }
    }
}

impl<T> fmt::Debug for NumberPrompt<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NumberPrompt")
            .field("prompt", &self.prompt)
            .field("config", &self.config)
            .field("interaction", &self.interaction)
            .finish()
    }
}

fn parse_number<T>(text: &str) -> std::result::Result<T, String>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    text.trim()
        .parse()
        .map_err(|error| format!("not a valid number ({error})"))
}

/// Answers a yes or no `Select` immediately on `y` or `n`.
struct YesNoKeys<W>(W);

impl<W: Widget> screw::Widget for YesNoKeys<W> {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        self.0.render(ctx, out);
    }

    fn tick_interest(&self) -> TickInterest {
        self.0.tick_interest()
    }

    fn vertical_size(&self) -> VerticalSize {
        self.0.vertical_size()
    }
}

impl<W: Widget> Widget for YesNoKeys<W> {
    fn id(&self) -> WidgetId {
        self.0.id()
    }

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        if let Event::Key(key) = &event {
            let held = key.modifiers;
            let chorded = held.contains(Modifiers::CONTROL)
                || held.contains(Modifiers::ALT)
                || held.contains(Modifiers::SUPER);
            if !chorded {
                match key.key {
                    Key::Char('y' | 'Y') => return Reaction::Submit(Value::from("yes")),
                    Key::Char('n' | 'N') => return Reaction::Submit(Value::from("no")),
                    _ => {},
                }
            }
        }
        self.0.handle(event, cx)
    }

    fn current_value(&self) -> Option<Value> {
        self.0.current_value()
    }
}

/// Shows a prompt line above a widget that has no header of its own.
struct Headed<W> {
    prompt: Span,
    inner:  W,
}

impl<W> Headed<W> {
    fn new(prompt: impl Into<String>, inner: W) -> Self {
        Self {
            prompt: Span::new(prompt).role(Role::Prompt),
            inner,
        }
    }
}

impl<W: Widget> screw::Widget for Headed<W> {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let children: Vec<LocalWidgetRef<'_>> = vec![
            local_widget(Spans::new([self.prompt.clone()])),
            local_widget(&self.inner),
        ];
        Stack::new(children).render(ctx, out);
    }

    fn tick_interest(&self) -> TickInterest {
        self.inner.tick_interest()
    }

    fn vertical_size(&self) -> VerticalSize {
        self.inner.vertical_size()
    }
}

impl<W: Widget> Widget for Headed<W> {
    fn id(&self) -> WidgetId {
        self.inner.id()
    }

    fn handle(&mut self, event: Event, cx: &mut Context) -> Reaction {
        self.inner.handle(event, cx)
    }

    fn current_value(&self) -> Option<Value> {
        self.inner.current_value()
    }
}

/// Today in the local time zone, falling back to UTC when the zone is unknown.
fn today_local() -> Date {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0));
    date_at(seconds, local_offset_seconds(seconds))
}

fn date_at(unix_seconds: i64, offset_seconds: i64) -> Date {
    Date::from_unix_days(
        unix_seconds
            .saturating_add(offset_seconds)
            .div_euclid(86_400),
    )
}

#[allow(
    clippy::useless_conversion,
    reason = "c_long and time_t are 32 bit on some targets"
)]
fn local_offset_seconds(unix_seconds: i64) -> i64 {
    let time = libc::time_t::try_from(unix_seconds).unwrap_or_default();
    // SAFETY: a zeroed tm is plain data that localtime_r overwrites on success.
    let mut local = unsafe { std::mem::zeroed::<libc::tm>() };
    // SAFETY: both pointers are valid for the call, and localtime_r keeps no
    // state of its own beyond the time zone data libc guards internally.
    if unsafe { libc::localtime_r(&raw const time, &raw mut local) }.is_null() {
        return 0;
    }
    i64::from(local.tm_gmtoff)
}

#[cfg(test)]
mod tests {
    use bang_core::{
        Event,
        Key,
        KeyEvent,
    };

    use super::*;
    use crate::advanced::scripted_interaction;

    fn keys(keys: impl IntoIterator<Item = Key>) -> Vec<Event> {
        keys.into_iter()
            .map(|key| Event::Key(KeyEvent::new(key)))
            .collect()
    }

    fn typed(text: &str) -> Vec<Event> {
        text.chars().map(Event::char).collect()
    }

    fn enter() -> Event {
        Event::key(Key::Enter)
    }

    #[test]
    fn a_zone_offset_moves_today_across_midnight() {
        // 2026-10-10T02:00:00Z
        let instant = 1_791_597_600;
        let utc = Date::new(2026, 10, 10).unwrap();
        assert_eq!(date_at(instant, 0), utc);
        assert_eq!(
            date_at(instant, -8 * 3_600),
            Date::new(2026, 10, 9).unwrap()
        );
        assert_eq!(
            date_at(instant, 23 * 3_600),
            Date::new(2026, 10, 11).unwrap()
        );
    }

    #[test]
    fn password_returns_the_real_text() {
        let mut script = typed("hunter2");
        script.push(enter());

        let outcome = password("secret: ")
            .interaction(scripted_interaction([script]))
            .interact()
            .unwrap();

        assert_eq!(outcome, PromptOutcome::Submit("hunter2".to_owned()));
    }

    #[test]
    fn password_validator_sees_the_real_text() {
        let mut script = typed("ab");
        script.push(enter());
        script.extend(typed("c"));
        script.push(enter());

        let outcome = password("secret: ")
            .mask('•')
            .validator(|value| {
                (value.len() >= 3)
                    .then_some(())
                    .ok_or_else(|| "too short".to_owned())
            })
            .interaction(scripted_interaction([script]))
            .interact()
            .unwrap();

        assert_eq!(outcome, PromptOutcome::Submit("abc".to_owned()));
    }

    #[test]
    fn confirm_declines_by_default_and_accepts_when_defaulted_or_moved() {
        let interaction =
            scripted_interaction([vec![enter()], vec![enter()], keys([Key::Down, Key::Enter])]);

        let first = confirm("sure?").interaction(interaction.clone()).interact();
        let second = confirm("sure?")
            .default(true)
            .interaction(interaction.clone())
            .interact();
        let third = confirm("sure?").interaction(interaction).interact();

        assert_eq!(first.unwrap(), PromptOutcome::Submit(false));
        assert_eq!(second.unwrap(), PromptOutcome::Submit(true));
        assert_eq!(third.unwrap(), PromptOutcome::Submit(true));
    }

    #[test]
    fn confirm_answers_at_once_on_y_and_n_in_either_case() {
        let interaction = scripted_interaction([
            keys([Key::Char('y')]),
            keys([Key::Char('Y')]),
            keys([Key::Char('n')]),
            keys([Key::Char('N')]),
        ]);

        let answers = (0..4)
            .map(|_| {
                confirm("sure?")
                    .default(true)
                    .interaction(interaction.clone())
                    .interact()
                    .unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(answers, [
            PromptOutcome::Submit(true),
            PromptOutcome::Submit(true),
            PromptOutcome::Submit(false),
            PromptOutcome::Submit(false),
        ]);
    }

    #[test]
    fn confirm_ignores_y_and_n_held_with_a_control_chord() {
        let chord = Event::Key(KeyEvent::with_modifiers(Key::Char('y'), Modifiers::CONTROL));
        let interaction = scripted_interaction([vec![chord, enter()]]);

        let outcome = confirm("sure?")
            .interaction(interaction)
            .interact()
            .unwrap();

        assert_eq!(outcome, PromptOutcome::Submit(false));
    }

    #[test]
    fn confirm_leaves_on_escape() {
        let outcome = confirm("sure?")
            .interaction(scripted_interaction([keys([Key::Esc])]))
            .interact()
            .unwrap();

        assert_eq!(outcome, PromptOutcome::Leave);
    }

    #[test]
    fn date_submits_the_moved_selection() {
        let start = Date::new(2026, 2, 27).unwrap();

        let outcome = date("when: ")
            .default(start)
            .interaction(scripted_interaction([keys([
                Key::Right,
                Key::Right,
                Key::Enter,
            ])]))
            .interact()
            .unwrap();

        assert_eq!(
            outcome,
            PromptOutcome::Submit(Date::new(2026, 3, 1).unwrap())
        );
    }

    #[test]
    fn date_without_a_default_starts_on_a_valid_day() {
        let PromptOutcome::Submit(picked) = date("when: ")
            .interaction(scripted_interaction([vec![enter()]]))
            .interact()
            .unwrap()
        else {
            panic!("date should submit");
        };

        assert_eq!(
            Date::new(picked.year, picked.month, picked.day),
            Some(picked)
        );
    }

    #[test]
    fn every_input_prompt_takes_a_config() {
        let mut script = typed("pw");
        script.push(enter());
        let password = password("secret: ")
            .with_config(PasswordConfig::default().id("pw").mask('#'))
            .interaction(scripted_interaction([script]))
            .interact()
            .unwrap();
        assert_eq!(password, PromptOutcome::Submit("pw".to_owned()));

        let confirmed = confirm("sure?")
            .with_config(ConfirmConfig::new().default(false))
            .interaction(scripted_interaction([vec![enter()]]))
            .interact()
            .unwrap();
        assert_eq!(confirmed, PromptOutcome::Submit(false));

        let picked = Date::new(2026, 7, 4).unwrap();
        let dated = date("when?")
            .with_config(DateConfig::new().default(picked))
            .interaction(scripted_interaction([vec![enter()]]))
            .interact()
            .unwrap();
        assert_eq!(dated, PromptOutcome::Submit(picked));

        let numbered = number::<i64>("count: ")
            .with_config(NumberConfig::default().value(7_i64).validator(|count| {
                if *count > 0 {
                    Ok(())
                } else {
                    Err("positive".to_owned())
                }
            }))
            .interaction(scripted_interaction([vec![enter()]]))
            .interact()
            .unwrap();
        assert_eq!(numbered, PromptOutcome::Submit(7));
    }

    #[test]
    fn number_parses_integers_and_floats() {
        let mut integer = typed(" 42 ");
        integer.push(enter());
        let mut float = typed("-1.5");
        float.push(enter());
        let interaction = scripted_interaction([integer, float]);

        let integer = number::<i64>("n: ")
            .interaction(interaction.clone())
            .interact()
            .unwrap();
        let float = number::<f64>("x: ")
            .interaction(interaction)
            .interact()
            .unwrap();

        assert_eq!(integer, PromptOutcome::Submit(42));
        assert_eq!(float, PromptOutcome::Submit(-1.5));
    }

    #[test]
    fn number_refuses_text_and_values_the_validator_rejects() {
        let mut script = typed("abc");
        script.push(enter());
        script.extend(std::iter::repeat_n(Event::key(Key::Backspace), 3));
        script.extend(typed("-4"));
        script.push(enter());
        script.extend(std::iter::repeat_n(Event::key(Key::Backspace), 2));
        script.extend(typed("7"));
        script.push(enter());

        let outcome = number::<i64>("n: ")
            .validator(|value| {
                (*value > 0)
                    .then_some(())
                    .ok_or_else(|| "must be positive".to_owned())
            })
            .interaction(scripted_interaction([script]))
            .interact()
            .unwrap();

        assert_eq!(outcome, PromptOutcome::Submit(7));
    }

    #[test]
    fn number_starts_from_a_given_value() {
        let outcome = number::<u16>("port: ")
            .value(8080)
            .interaction(scripted_interaction([vec![enter()]]))
            .interact()
            .unwrap();

        assert_eq!(outcome, PromptOutcome::Submit(8080));
    }
}
