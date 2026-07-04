use bang_core::{
    ActionBinding,
    Reaction,
    Session,
    SessionStatus,
    Value,
    Widget,
    widgets::{
        MultiSelect,
        SearchSelect,
        Select,
        SelectItem,
        TextInput,
    },
};

use crate::{
    Error,
    Result,
};

#[must_use]
pub fn select(id: impl Into<String>) -> SelectPrompt {
    SelectPrompt::new(id)
}

#[must_use]
pub fn multi_select(id: impl Into<String>) -> MultiSelectPrompt {
    MultiSelectPrompt::new(id)
}

#[must_use]
pub fn search(id: impl Into<String>) -> SearchPrompt {
    SearchPrompt::new(id)
}

#[must_use]
pub fn text(prompt: impl Into<String>) -> TextPrompt {
    TextPrompt::new(prompt)
}

#[derive(Clone, Debug)]
pub struct SelectPrompt {
    id:        String,
    items:     Vec<SelectItem>,
    page_size: usize,
    selected:  Option<usize>,
    actions:   Vec<ActionBinding>,
}

impl SelectPrompt {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id:        id.into(),
            items:     Vec::new(),
            page_size: 9,
            selected:  None,
            actions:   Vec::new(),
        }
    }

    #[must_use]
    pub fn option(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.items.push(SelectItem::new(label.clone(), label));
        self
    }

    #[must_use]
    pub fn item(mut self, label: impl Into<String>, value: impl Into<Value>) -> Self {
        self.items.push(SelectItem::new(label, value));
        self
    }

    #[must_use]
    pub const fn page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size;
        self
    }

    #[must_use]
    pub const fn selected(mut self, selected: usize) -> Self {
        self.selected = Some(selected);
        self
    }

    #[must_use]
    pub fn action(mut self, action: ActionBinding) -> Self {
        self.actions.push(action);
        self
    }

    pub fn run(self) -> Result<Value> {
        let mut widget = Select::new(self.id, self.items).with_page_size(self.page_size);
        if let Some(selected) = self.selected {
            widget = widget.with_selected_index(selected);
        }
        run_widget(widget, self.actions)
    }

    pub fn run_string(self) -> Result<String> {
        into_string(self.run()?)
    }
}

#[derive(Clone, Debug)]
pub struct MultiSelectPrompt {
    id:        String,
    items:     Vec<SelectItem>,
    page_size: usize,
    checked:   Vec<usize>,
    actions:   Vec<ActionBinding>,
}

impl MultiSelectPrompt {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id:        id.into(),
            items:     Vec::new(),
            page_size: 9,
            checked:   Vec::new(),
            actions:   Vec::new(),
        }
    }

    #[must_use]
    pub fn option(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.items.push(SelectItem::new(label.clone(), label));
        self
    }

    #[must_use]
    pub fn item(mut self, label: impl Into<String>, value: impl Into<Value>) -> Self {
        self.items.push(SelectItem::new(label, value));
        self
    }

    #[must_use]
    pub const fn page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size;
        self
    }

    #[must_use]
    pub fn checked(mut self, index: usize) -> Self {
        self.checked.push(index);
        self
    }

    #[must_use]
    pub fn action(mut self, action: ActionBinding) -> Self {
        self.actions.push(action);
        self
    }

    pub fn run(self) -> Result<Value> {
        let widget = MultiSelect::new(self.id, self.items)
            .with_page_size(self.page_size)
            .with_checked_indices(self.checked);
        run_widget(widget, self.actions)
    }
}

#[derive(Clone, Debug)]
pub struct SearchPrompt {
    id:          String,
    items:       Vec<SelectItem>,
    page_size:   usize,
    prompt:      Option<String>,
    placeholder: Option<String>,
    actions:     Vec<ActionBinding>,
}

impl SearchPrompt {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id:          id.into(),
            items:       Vec::new(),
            page_size:   9,
            prompt:      None,
            placeholder: None,
            actions:     Vec::new(),
        }
    }

    #[must_use]
    pub fn option(mut self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.items.push(SelectItem::new(label.clone(), label));
        self
    }

    #[must_use]
    pub fn item(mut self, label: impl Into<String>, value: impl Into<Value>) -> Self {
        self.items.push(SelectItem::new(label, value));
        self
    }

    #[must_use]
    pub const fn page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size;
        self
    }

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
    pub fn action(mut self, action: ActionBinding) -> Self {
        self.actions.push(action);
        self
    }

    pub fn run(self) -> Result<Value> {
        let mut widget = SearchSelect::new(self.id, self.items).with_page_size(self.page_size);
        if let Some(prompt) = self.prompt {
            widget = widget.with_prompt(prompt);
        }
        if let Some(placeholder) = self.placeholder {
            widget = widget.with_placeholder(placeholder);
        }
        run_widget(widget, self.actions)
    }
}

#[derive(Clone, Debug)]
pub struct TextPrompt {
    id:          String,
    prompt:      String,
    placeholder: Option<String>,
    value:       Option<String>,
    actions:     Vec<ActionBinding>,
}

impl TextPrompt {
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            id:          "text".to_owned(),
            prompt:      prompt.into(),
            placeholder: None,
            value:       None,
            actions:     Vec::new(),
        }
    }

    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    #[must_use]
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    #[must_use]
    pub fn action(mut self, action: ActionBinding) -> Self {
        self.actions.push(action);
        self
    }

    pub fn run(self) -> Result<Value> {
        let mut widget = TextInput::new(self.id).with_prompt(self.prompt);
        if let Some(placeholder) = self.placeholder {
            widget = widget.with_placeholder(placeholder);
        }
        if let Some(value) = self.value {
            widget = widget.with_value(value);
        }
        run_widget(widget, self.actions)
    }

    pub fn run_string(self) -> Result<String> {
        into_string(self.run()?)
    }
}

pub fn run_widget(widget: impl Widget + 'static, actions: Vec<ActionBinding>) -> Result<Value> {
    let widget = bang_core::ActionLayer::new(widget).with_actions(actions);
    bang_screw::run_live_session(widget).map_err(Error::from)
}

/// Drive a widget with already-decoded events. Useful for tests and examples.
pub fn replay_events(
    widget: impl Widget + 'static,
    events: impl IntoIterator<Item = bang_core::Event>,
) -> Result<Value> {
    let mut session = Session::new(widget);
    for event in events {
        match session.handle(event) {
            Reaction::Submit(value) => return Ok(value),
            Reaction::Cancel => return Err(Error::Cancelled),
            Reaction::Ignored | Reaction::Changed | Reaction::Focus(_) => {},
        }
        if !matches!(session.status(), SessionStatus::Running) {
            break;
        }
    }

    match session.status() {
        SessionStatus::Submitted(value) => Ok(value.clone()),
        SessionStatus::Cancelled => Err(Error::Cancelled),
        SessionStatus::Running => Err(Error::InputEnded),
    }
}

fn into_string(value: Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value),
        other => {
            Err(Error::UnexpectedValue {
                expected: "string",
                actual:   value_name(&other),
            })
        },
    }
}

const fn value_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Date(_) => "date",
        Value::List(_) => "list",
        Value::Object(_) => "object",
        _ => "unknown",
    }
}
