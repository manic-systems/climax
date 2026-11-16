// SPDX-License-Identifier: EUPL-1.2

fn main() -> climax::Result<()> {
    climax::run_with((), |cx, ()| {
        cx.status("working")
            .spinner()
            .final_message("done")
            .finish()
    })
}
