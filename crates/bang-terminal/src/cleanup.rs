// SPDX-License-Identifier: EUPL-1.2

use core::{
    error,
    fmt,
};
use std::io;

/// the part of terminal teardown that failed
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CleanupStage {
    /// clearing the drawn frame
    Renderer,
    /// leaving the screen modes
    Screen,
    /// restoring signal handlers
    Signals,
    /// restoring the terminal settings
    RawMode,
}

/// one failed teardown step and the I/O error behind it
#[derive(Debug)]
pub struct CleanupFailure {
    stage:  CleanupStage,
    source: io::Error,
}

impl CleanupFailure {
    /// records that `stage` failed with `source`
    #[must_use]
    pub const fn new(stage: CleanupStage, source: io::Error) -> Self {
        Self { stage, source }
    }

    /// which teardown step failed
    #[must_use]
    pub const fn stage(&self) -> CleanupStage {
        self.stage
    }

    /// the underlying I/O error
    #[must_use]
    pub const fn source_error(&self) -> &io::Error {
        &self.source
    }
}

impl fmt::Display for CleanupFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage = match self.stage {
            CleanupStage::Renderer => "renderer",
            CleanupStage::Screen => "screen",
            CleanupStage::Signals => "signals",
            CleanupStage::RawMode => "raw mode",
        };
        write!(f, "{stage} failed, {}", self.source)
    }
}

impl error::Error for CleanupFailure {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.source)
    }
}

/// every teardown step that failed during one restore, all steps run even after
/// one fails
#[derive(Debug, Default)]
pub struct CleanupFailures(Vec<CleanupFailure>);

impl CleanupFailures {
    /// collects `failures` in the order they happened
    #[must_use]
    pub const fn new(failures: Vec<CleanupFailure>) -> Self {
        Self(failures)
    }

    /// every failure, in the order it happened
    #[must_use]
    pub fn failures(&self) -> &[CleanupFailure] {
        &self.0
    }

    /// takes the failures out of the wrapper
    #[must_use]
    pub fn into_failures(self) -> Vec<CleanupFailure> {
        self.0
    }

    /// whether nothing failed
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for CleanupFailures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, failure) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{failure}")?;
        }
        Ok(())
    }
}

impl error::Error for CleanupFailures {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.0
            .first()
            .map(|failure| -> &(dyn error::Error + 'static) { failure })
    }
}

pub(crate) fn staged(stage: CleanupStage, errors: Vec<io::Error>) -> Result<(), CleanupFailures> {
    if errors.is_empty() {
        return Ok(());
    }
    Err(CleanupFailures::new(
        errors
            .into_iter()
            .map(|source| CleanupFailure::new(stage, source))
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_failures_keep_their_stage_and_order() {
        let failures = staged(CleanupStage::Signals, vec![
            io::Error::other("first"),
            io::Error::other("second"),
        ])
        .unwrap_err();

        assert_eq!(failures.failures().len(), 2);
        assert_eq!(failures.failures()[0].stage(), CleanupStage::Signals);
        assert_eq!(
            failures.to_string(),
            "signals failed, first, signals failed, second"
        );
        staged(CleanupStage::Screen, Vec::new()).unwrap();
    }
}
