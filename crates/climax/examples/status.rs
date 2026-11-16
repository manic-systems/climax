// SPDX-License-Identifier: EUPL-1.2

use core::time::Duration;
use std::{
    process::ExitCode,
    thread,
};

fn main() -> ExitCode {
    climax::main_with!(|cx| {
        let progress = cx
            .status("downloading")
            .spinner()
            .final_message("done")
            .progress(20);
        for _ in 0..20 {
            thread::sleep(Duration::from_millis(100));
            progress.inc(1)?;
        }
        progress.finish()
    })
}
