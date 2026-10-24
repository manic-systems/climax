// SPDX-License-Identifier: EUPL-1.2

use std::io;

use crate::Viewport;

/// columns and rows of standard error, width falls back to
/// [`Viewport::FALLBACK`] off a terminal
pub(crate) fn stderr_size() -> (usize, Option<usize>) {
    Viewport::of(&io::stderr()).map_or((Viewport::FALLBACK.columns, None), |size| {
        (size.columns, Some(size.rows))
    })
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    fn measuring_a_non_terminal_fails() {
        let null = File::open("/dev/null").unwrap();
        Viewport::of(&null).unwrap_err();
    }
}
