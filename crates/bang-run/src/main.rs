// SPDX-License-Identifier: EUPL-1.2

use std::{
    fmt,
    io::{
        self,
        Write as _,
    },
};

use bang_run::{
    Cli,
    CliError,
};
use pound::Parse as _;

fn main() {
    match bang_run::run(Cli::parse()) {
        Ok(output) => {
            let mut stdout = io::stdout().lock();
            if let Err(error) = stdout
                .write_all(output.as_bytes())
                .and_then(|()| stdout.flush())
            {
                if error.kind() == io::ErrorKind::BrokenPipe {
                    std::process::exit(141);
                }
                print_error(error);
                std::process::exit(1);
            }
        },
        Err(error @ (CliError::Cancelled | CliError::Interrupted(..))) => {
            for failure in error.cleanup_failures() {
                print_error(failure);
            }
            std::process::exit(error.exit_code())
        },
        Err(error) => {
            print_error(&error);
            std::process::exit(error.exit_code());
        },
    }
}

fn print_error(error: impl fmt::Display) {
    write_error(io::stderr().lock(), error);
}

/// A pty hangup can turn `eprintln!` into a panic (`EIO` on the write, then a
/// broken write on the panic message itself). Write through the caller's
/// stream and drop the result instead.
fn write_error(mut writer: impl io::Write, error: impl fmt::Display) {
    let _ = writeln!(writer, "error: {error}");
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::write_error;

    struct AlwaysFails;

    impl io::Write for AlwaysFails {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }
    }

    #[test]
    fn write_error_does_not_panic_when_the_stream_is_gone() {
        write_error(AlwaysFails, "boom");
    }
}
