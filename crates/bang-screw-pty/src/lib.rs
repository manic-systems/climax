// SPDX-License-Identifier: EUPL-1.2

//! PTY overlay runtime for bang widgets rendered with screw

use bang_core::Widget as BangWidget;
use screw::Theme;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessSpec {
    program: String,
    args: Vec<String>,
}

impl ProcessSpec {
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.args
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputPolicy {
    ChildFirst,
    OverlayFirst,
    OverlayWhenOpen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayPlacement {
    Inline,
    Bottom { height: u16 },
    Floating { row: u16, col: u16, width: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CtrlCPolicy {
    Child,
    CancelOverlay,
    AbortRuntime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlayConfig {
    process: ProcessSpec,
    input_policy: InputPolicy,
    placement: OverlayPlacement,
    ctrl_c: CtrlCPolicy,
    theme: Theme,
}

impl OverlayConfig {
    #[must_use]
    pub const fn process(&self) -> &ProcessSpec {
        &self.process
    }

    #[must_use]
    pub const fn input_policy(&self) -> InputPolicy {
        self.input_policy
    }

    #[must_use]
    pub const fn placement(&self) -> OverlayPlacement {
        self.placement
    }

    #[must_use]
    pub const fn ctrl_c_policy(&self) -> CtrlCPolicy {
        self.ctrl_c
    }

    #[must_use]
    pub const fn theme(&self) -> Theme {
        self.theme
    }
}

#[derive(Clone, Debug)]
pub struct OverlayBuilder {
    config: OverlayConfig,
}

impl OverlayBuilder {
    #[must_use]
    pub fn new(process: ProcessSpec) -> Self {
        Self {
            config: OverlayConfig {
                process,
                input_policy: InputPolicy::OverlayWhenOpen,
                placement: OverlayPlacement::Bottom { height: 8 },
                ctrl_c: CtrlCPolicy::Child,
                theme: Theme::DEFAULT,
            },
        }
    }

    #[must_use]
    pub const fn input_policy(mut self, policy: InputPolicy) -> Self {
        self.config.input_policy = policy;
        self
    }

    #[must_use]
    pub const fn placement(mut self, placement: OverlayPlacement) -> Self {
        self.config.placement = placement;
        self
    }

    #[must_use]
    pub const fn ctrl_c_policy(mut self, policy: CtrlCPolicy) -> Self {
        self.config.ctrl_c = policy;
        self
    }

    #[must_use]
    pub const fn theme(mut self, theme: Theme) -> Self {
        self.config.theme = theme;
        self
    }

    #[must_use]
    pub fn widget<W>(self, widget: W) -> Overlay<W>
    where
        W: BangWidget,
    {
        Overlay {
            config: self.config,
            widget,
        }
    }

    #[must_use]
    pub fn config(self) -> OverlayConfig {
        self.config
    }
}

#[derive(Clone, Debug)]
pub struct Overlay<W> {
    config: OverlayConfig,
    widget: W,
}

impl<W> Overlay<W>
where
    W: BangWidget,
{
    #[must_use]
    pub const fn config(&self) -> &OverlayConfig {
        &self.config
    }

    #[must_use]
    pub const fn widget(&self) -> &W {
        &self.widget
    }

    pub fn into_parts(self) -> (OverlayConfig, W) {
        (self.config, self.widget)
    }
}

#[must_use]
pub fn command(program: impl Into<String>) -> OverlayBuilder {
    OverlayBuilder::new(ProcessSpec::new(program))
}
