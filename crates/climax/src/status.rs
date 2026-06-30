use screw::{
    Looping,
    Runtime,
    Text,
    WidgetRef,
    layout,
    widget,
};

use crate::Result;

#[must_use]
pub fn message(message: impl Into<String>) -> Status {
    Status::new(message)
}

pub struct Status {
    message:       String,
    spinner:       bool,
    fps:           u16,
    final_message: Option<String>,
}

impl Status {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message:       message.into(),
            spinner:       false,
            fps:           15,
            final_message: None,
        }
    }

    #[must_use]
    pub const fn spinner(mut self) -> Self {
        self.spinner = true;
        self
    }

    #[must_use]
    pub const fn fps(mut self, fps: u16) -> Self {
        self.fps = fps;
        self
    }

    #[must_use]
    pub fn final_message(mut self, message: impl Into<String>) -> Self {
        self.final_message = Some(message.into());
        self
    }

    pub fn start(self) -> StatusRuntime {
        let root = self.root_widget();
        let mut builder = Runtime::stderr_auto(root).fps(self.fps);
        if let Some(final_message) = self.final_message {
            builder = builder.final_widget(widget(Text::new(final_message)));
        }
        StatusRuntime {
            runtime: builder.start(),
        }
    }

    pub fn finish(self) -> Result<()> {
        self.start().finish()
    }

    fn root_widget(&self) -> WidgetRef {
        if self.spinner {
            layout()
                .line(vec![
                    widget(Looping::new(["/", "-", "\\", "|"])),
                    widget(Text::new(format!(" {}", self.message))),
                ])
                .into_widget()
        } else {
            widget(Text::new(self.message.clone()))
        }
    }
}

pub struct StatusRuntime {
    runtime: screw::AutoRuntime<std::io::Stderr>,
}

impl StatusRuntime {
    pub fn mark_dirty(&self) -> Result<()> {
        self.runtime.mark_dirty()?;
        Ok(())
    }

    pub fn finish(self) -> Result<()> {
        let _stderr = self.runtime.finish()?;
        Ok(())
    }
}
