// SPDX-License-Identifier: EUPL-1.2

fn main() -> climax::Result<()> {
    climax::run_with((), |context, ()| {
        context.status("rendering only").spinner().finish()
    })
}
