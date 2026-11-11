// SPDX-License-Identifier: EUPL-1.2

#[cfg(not(any(feature = "render", feature = "interactive")))]
use std::env;
use std::io::{
    self,
    IsTerminal as _,
};
#[cfg(all(feature = "interactive", feature = "render"))]
use std::io::{
    Read,
    Write,
};
#[cfg(all(feature = "interactive", feature = "render"))]
use std::os::fd::AsFd;
#[cfg(any(feature = "render", feature = "interactive"))]
use std::os::fd::OwnedFd;

#[cfg(all(feature = "interactive", not(feature = "render")))]
use bang::screw;

/// whether prompts use detected terminal capabilities
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum InteractionMode {
    /// interact only when stdin and the transient stream are terminals
    #[default]
    Auto,
    /// interact regardless of detection
    Force,
    /// reject every prompt without touching the terminal
    Disabled,
}

/// how transient status is presented
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum StatusMode {
    /// animate on a suitable terminal, print plain lines otherwise
    #[default]
    Auto,
    /// animate regardless of detection
    Live,
    /// one plain line when an operation finishes
    Plain,
    /// render nothing
    Silent,
}

/// whether colour is written
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg_attr(feature = "derive", derive(pound::ValueEnum))]
#[non_exhaustive]
pub enum ColorChoice {
    /// colour on a terminal when the environment allows it
    #[default]
    Auto,
    /// colour wherever the facade writes styled text
    Always,
    /// no colour
    Never,
}

impl ColorChoice {
    pub(crate) const fn resolve(self, detected: bool) -> bool {
        match self {
            Self::Auto => detected,
            Self::Always => true,
            Self::Never => false,
        }
    }
}

#[cfg(any(feature = "render", feature = "interactive"))]
fn ansi_available() -> bool {
    !screw::is_dumb_terminal()
}

#[cfg(any(feature = "render", feature = "interactive"))]
fn color_allowed() -> bool {
    screw::colors_enabled()
}

#[cfg(not(any(feature = "render", feature = "interactive")))]
fn ansi_available() -> bool {
    env::var_os("TERM").is_none_or(|term| term != "dumb")
}

#[cfg(not(any(feature = "render", feature = "interactive")))]
fn color_allowed() -> bool {
    let set = |name| env::var_os(name).filter(|value| !value.is_empty());
    if set("NO_COLOR").is_some() {
        return false;
    }
    if set("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
        return true;
    }
    ansi_available() && set("CLICOLOR").is_none_or(|value| value != "0")
}

/// terminal facts observed by the facade
///
/// build one with [`Self::detect`] or [`Self::none`] and the `with_` setters
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalCapabilities {
    terminals: Terminals,
    ansi:      bool,
    color:     bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Terminals {
    input:     bool,
    transient: bool,
    output:    bool,
}

/// readable terminal input accepted by a [`TerminalApplication`]
#[cfg(all(feature = "interactive", feature = "render"))]
pub trait TerminalInput: Read + AsFd {}

#[cfg(all(feature = "interactive", feature = "render"))]
impl<T> TerminalInput for T where T: Read + AsFd + ?Sized {}

/// input and transient output held under the context's exclusive terminal lease
#[cfg(all(feature = "interactive", feature = "render"))]
pub struct TerminalApplication<'a> {
    input:        Box<dyn TerminalInput + 'a>,
    output:       Box<dyn Write + 'a>,
    capabilities: TerminalCapabilities,
}

#[cfg(all(feature = "interactive", feature = "render"))]
impl<'a> TerminalApplication<'a> {
    pub(crate) const fn new(
        input: Box<dyn TerminalInput + 'a>,
        output: Box<dyn Write + 'a>,
        capabilities: TerminalCapabilities,
    ) -> Self {
        Self {
            input,
            output,
            capabilities,
        }
    }

    #[must_use]
    /// capabilities in force when the application started
    pub const fn capabilities(&self) -> TerminalCapabilities {
        self.capabilities
    }

    /// terminal input, a handle to `/dev/tty` or process stdin
    pub fn input(&mut self) -> &mut (dyn TerminalInput + 'a) {
        &mut *self.input
    }

    /// transient output writer
    pub fn output(&mut self) -> &mut dyn Write {
        &mut *self.output
    }

    /// input and output borrowed together, which separate `input` and `output`
    /// calls cannot
    pub fn split(&mut self) -> (&mut (dyn TerminalInput + 'a), &mut (dyn Write + 'a)) {
        (&mut *self.input, &mut *self.output)
    }
}

#[cfg(all(feature = "interactive", feature = "render"))]
impl Write for TerminalApplication<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

impl TerminalCapabilities {
    #[must_use]
    /// no terminal anywhere and no colour, the start for tests and hosts that
    /// detect terminals themselves
    pub const fn none() -> Self {
        Self {
            terminals: Terminals {
                input:     false,
                transient: false,
                output:    false,
            },
            ansi:      false,
            color:     false,
        }
    }

    #[must_use]
    /// detect from process stdin, stdout and stderr, `TERM` and the colour
    /// variables `NO_COLOR`, `CLICOLOR` and `CLICOLOR_FORCE`
    pub fn detect() -> Self {
        Self {
            terminals: Terminals {
                input:     io::stdin().is_terminal(),
                transient: io::stderr().is_terminal(),
                output:    io::stdout().is_terminal(),
            },
            ansi:      ansi_available(),
            color:     color_allowed(),
        }
    }

    #[cfg(any(feature = "render", feature = "interactive"))]
    pub(crate) fn detect_on(handle: &OwnedFd) -> Self {
        let is_terminal = handle.is_terminal();
        let detected = Self::detect();
        Self {
            terminals: Terminals {
                input: is_terminal,
                transient: is_terminal,
                ..detected.terminals
            },
            ..detected
        }
    }

    #[must_use]
    /// sets whether input comes from a terminal
    pub const fn with_input_terminal(mut self, terminal: bool) -> Self {
        self.terminals.input = terminal;
        self
    }

    #[must_use]
    /// sets whether the transient stream is a terminal
    pub const fn with_transient_terminal(mut self, terminal: bool) -> Self {
        self.terminals.transient = terminal;
        self
    }

    #[must_use]
    /// sets whether stdout is a terminal
    pub const fn with_output_terminal(mut self, terminal: bool) -> Self {
        self.terminals.output = terminal;
        self
    }

    #[must_use]
    /// sets whether ANSI escapes should work
    pub const fn with_ansi(mut self, ansi: bool) -> Self {
        self.ansi = ansi;
        self
    }

    #[must_use]
    /// sets whether the environment allows colour
    pub const fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    #[must_use]
    /// whether input comes from a terminal
    pub const fn input_terminal(self) -> bool {
        self.terminals.input
    }

    #[must_use]
    /// whether the transient stream, stderr by default, is a terminal
    pub const fn transient_terminal(self) -> bool {
        self.terminals.transient
    }

    #[must_use]
    /// whether stdout, where results go by default, is a terminal
    pub const fn output_terminal(self) -> bool {
        self.terminals.output
    }

    #[must_use]
    /// whether ANSI escapes should work, true unless `TERM` is `dumb`
    pub const fn ansi(self) -> bool {
        self.ansi
    }

    #[must_use]
    /// whether the environment allows colour, which `NO_COLOR` and a dumb
    /// terminal refuse and `CLICOLOR_FORCE` grants
    pub const fn color(self) -> bool {
        self.color
    }

    #[must_use]
    /// whether prompts can run, needing terminal input, a terminal transient
    /// stream and ANSI
    pub const fn interaction_available(self) -> bool {
        self.terminals.input && self.terminals.transient && self.ansi
    }

    #[must_use]
    /// whether a live status can animate, needing a terminal transient stream
    /// and ANSI
    pub const fn live_status_available(self) -> bool {
        self.terminals.transient && self.ansi
    }
}

/// terminal capabilities and overrides carried by [`crate::Context`]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalPolicy {
    capabilities: TerminalCapabilities,
    interaction:  InteractionMode,
    status:       StatusMode,
}

impl TerminalPolicy {
    #[must_use]
    pub(crate) fn process() -> Self {
        Self {
            capabilities: TerminalCapabilities::detect(),
            interaction:  InteractionMode::Auto,
            status:       StatusMode::Auto,
        }
    }

    #[must_use]
    /// detected or injected capabilities
    pub const fn capabilities(self) -> TerminalCapabilities {
        self.capabilities
    }

    #[must_use]
    /// interaction override, [`InteractionMode::Auto`] unless changed
    pub const fn interaction_mode(self) -> InteractionMode {
        self.interaction
    }

    #[must_use]
    /// status override, [`StatusMode::Auto`] unless changed
    pub const fn status_mode(self) -> StatusMode {
        self.status
    }

    #[must_use]
    /// whether prompts can run once the interaction override is applied
    pub const fn interaction_available(self) -> bool {
        match self.interaction {
            InteractionMode::Auto => self.capabilities.interaction_available(),
            InteractionMode::Force => true,
            InteractionMode::Disabled => false,
        }
    }

    #[must_use]
    /// status mode after resolving `Auto` to `Live` on a capable terminal and
    /// `Plain` otherwise
    pub const fn effective_status_mode(self) -> StatusMode {
        match self.status {
            StatusMode::Auto if self.capabilities.live_status_available() => StatusMode::Live,
            StatusMode::Auto => StatusMode::Plain,
            status @ (StatusMode::Live | StatusMode::Plain | StatusMode::Silent) => status,
        }
    }

    pub(crate) const fn set_capabilities(&mut self, capabilities: TerminalCapabilities) {
        self.capabilities = capabilities;
    }

    pub(crate) const fn set_interaction_mode(&mut self, interaction: InteractionMode) {
        self.interaction = interaction;
    }

    pub(crate) const fn set_status_mode(&mut self, status: StatusMode) {
        self.status = status;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(input: bool, transient: bool, ansi: bool) -> TerminalCapabilities {
        TerminalCapabilities::none()
            .with_input_terminal(input)
            .with_transient_terminal(transient)
            .with_ansi(ansi)
    }

    #[test]
    fn auto_interaction_requires_input_transient_and_ansi_capabilities() {
        for capabilities in [
            caps(false, true, true),
            caps(true, false, true),
            caps(true, true, false),
        ] {
            let mut policy = TerminalPolicy::process();
            policy.set_capabilities(capabilities);
            assert!(!policy.interaction_available());
        }
        let mut policy = TerminalPolicy::process();
        policy.set_capabilities(caps(true, true, true));
        assert!(policy.interaction_available());
    }

    #[test]
    fn auto_status_is_plain_off_a_terminal_and_live_on_one() {
        let mut policy = TerminalPolicy::process();
        policy.set_capabilities(caps(false, false, false));
        assert_eq!(policy.effective_status_mode(), StatusMode::Plain);
        policy.set_capabilities(caps(true, true, true));
        assert_eq!(policy.effective_status_mode(), StatusMode::Live);
    }

    #[test]
    fn explicit_modes_override_detection() {
        let mut policy = TerminalPolicy::process();
        policy.set_capabilities(caps(false, false, false));
        policy.set_interaction_mode(InteractionMode::Force);
        policy.set_status_mode(StatusMode::Live);
        assert!(policy.interaction_available());
        assert_eq!(policy.effective_status_mode(), StatusMode::Live);

        policy.set_capabilities(caps(true, true, true));
        policy.set_interaction_mode(InteractionMode::Disabled);
        policy.set_status_mode(StatusMode::Silent);
        assert!(!policy.interaction_available());
        assert_eq!(policy.effective_status_mode(), StatusMode::Silent);
    }
}
