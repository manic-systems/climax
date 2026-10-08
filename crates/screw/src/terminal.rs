use std::io;

use crate::Viewport;

/// Columns and rows of standard error, with the width falling back to
/// [`Viewport::FALLBACK`] when it is not a terminal.
pub(crate) fn stderr_size() -> (usize, Option<usize>) {
    Viewport::of(&io::stderr()).map_or((Viewport::FALLBACK.columns, None), |size| {
        (size.columns, Some(size.rows))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measuring_a_non_terminal_fails() {
        let null = std::fs::File::open("/dev/null").unwrap();
        assert!(Viewport::of(&null).is_err());
    }
}
