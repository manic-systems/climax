use std::io::{self, IsTerminal as _};
#[cfg(all(feature = "interactive", feature = "render"))]
use std::io::{Read, Write};
#[cfg(all(feature = "interactive", feature = "render"))]
use std::os::fd::AsFd;

/// How prompt interaction should use detected terminal capabilities.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InteractionMode {
    /// Interact only when stdin and the transient stream are suitable
    /// terminals.
    #[default]
    Auto,
    /// Attempt interaction regardless of capability detection.
    Force,
    /// Reject all interactive prompts without touching the terminal.
    Disabled,
}

/// How transient status presentation should behave.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusMode {
    /// Animate on a suitable terminal and otherwise remain silent.
    #[default]
    Auto,
    /// Animate regardless of terminal capability detection.
    Live,
    /// Emit one plain status line when an operation finishes.
    Plain,
    /// Do not render status output.
    Silent,
}

/// Terminal facts observed by the application facade.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalCapabilities {
    input_terminal: bool,
    transient_terminal: bool,
    ansi: bool,
}

/// Readable terminal input accepted by a [`TerminalApplication`].
#[cfg(all(feature = "interactive", feature = "render"))]
pub trait TerminalInput: Read + AsFd {}

#[cfg(all(feature = "interactive", feature = "render"))]
impl<T> TerminalInput for T where T: Read + AsFd + ?Sized {}

/// Configured input and transient output held under Context's exclusive
/// terminal-presentation lease.
#[cfg(all(feature = "interactive", feature = "render"))]
pub struct TerminalApplication<'a> {
    input: Box<dyn TerminalInput + 'a>,
    output: Box<dyn Write + 'a>,
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
    pub const fn capabilities(&self) -> TerminalCapabilities {
        self.capabilities
    }

    pub fn input(&mut self) -> &mut (dyn TerminalInput + 'a) {
        &mut *self.input
    }

    pub fn output(&mut self) -> &mut dyn Write {
        &mut *self.output
    }

    pub fn split(&mut self) -> (&mut (dyn TerminalInput + 'a), &mut (dyn Write + 'a)) {
        (&mut *self.input, &mut *self.output)
    }
}

#[cfg(all(feature = "interactive", feature = "render"))]
impl Write for TerminalApplication<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.output.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

impl TerminalCapabilities {
    #[must_use]
    pub const fn new(input_terminal: bool, transient_terminal: bool, ansi: bool) -> Self {
        Self {
            input_terminal,
            transient_terminal,
            ansi,
        }
    }

    #[must_use]
    pub fn detect() -> Self {
        Self::detect_process()
    }

    pub(crate) fn detect_process() -> Self {
        let ansi = std::env::var_os("TERM").is_none_or(|term| term != "dumb");
        Self::new(io::stdin().is_terminal(), io::stderr().is_terminal(), ansi)
    }

    #[must_use]
    pub const fn input_terminal(self) -> bool {
        self.input_terminal
    }

    #[must_use]
    pub const fn transient_terminal(self) -> bool {
        self.transient_terminal
    }

    #[must_use]
    pub const fn ansi(self) -> bool {
        self.ansi
    }

    #[must_use]
    pub const fn interaction_available(self) -> bool {
        self.input_terminal && self.transient_terminal && self.ansi
    }

    #[must_use]
    pub const fn live_status_available(self) -> bool {
        self.transient_terminal && self.ansi
    }
}

/// Terminal capability and override policy carried by [`crate::Context`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalPolicy {
    capabilities: TerminalCapabilities,
    interaction: InteractionMode,
    status: StatusMode,
}

impl TerminalPolicy {
    #[must_use]
    pub(crate) fn process() -> Self {
        Self {
            capabilities: TerminalCapabilities::detect_process(),
            interaction: InteractionMode::Auto,
            status: StatusMode::Auto,
        }
    }

    #[must_use]
    pub const fn capabilities(self) -> TerminalCapabilities {
        self.capabilities
    }

    #[must_use]
    pub const fn interaction_mode(self) -> InteractionMode {
        self.interaction
    }

    #[must_use]
    pub const fn status_mode(self) -> StatusMode {
        self.status
    }

    #[must_use]
    pub const fn interaction_available(self) -> bool {
        match self.interaction {
            InteractionMode::Auto => self.capabilities.interaction_available(),
            InteractionMode::Force => true,
            InteractionMode::Disabled => false,
        }
    }

    #[must_use]
    pub const fn effective_status_mode(self) -> StatusMode {
        match self.status {
            StatusMode::Auto if self.capabilities.live_status_available() => StatusMode::Live,
            StatusMode::Auto => StatusMode::Silent,
            status => status,
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

    #[test]
    fn auto_interaction_requires_input_transient_and_ansi_capabilities() {
        for capabilities in [
            TerminalCapabilities::new(false, true, true),
            TerminalCapabilities::new(true, false, true),
            TerminalCapabilities::new(true, true, false),
        ] {
            let mut policy = TerminalPolicy::process();
            policy.set_capabilities(capabilities);
            assert!(!policy.interaction_available());
        }
        let mut policy = TerminalPolicy::process();
        policy.set_capabilities(TerminalCapabilities::new(true, true, true));
        assert!(policy.interaction_available());
    }

    #[test]
    fn explicit_modes_override_detection() {
        let mut policy = TerminalPolicy::process();
        policy.set_capabilities(TerminalCapabilities::new(false, false, false));
        policy.set_interaction_mode(InteractionMode::Force);
        policy.set_status_mode(StatusMode::Live);
        assert!(policy.interaction_available());
        assert_eq!(policy.effective_status_mode(), StatusMode::Live);

        policy.set_capabilities(TerminalCapabilities::new(true, true, true));
        policy.set_interaction_mode(InteractionMode::Disabled);
        policy.set_status_mode(StatusMode::Silent);
        assert!(!policy.interaction_available());
        assert_eq!(policy.effective_status_mode(), StatusMode::Silent);
    }
}
