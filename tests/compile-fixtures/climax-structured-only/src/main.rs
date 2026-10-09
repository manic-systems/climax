// SPDX-License-Identifier: EUPL-1.2

fn main() -> climax::Result<()> {
    climax::run_with((), |context, ()| {
        context.output().result(&42).text(|value| *value).emit()
    })
}
