// SPDX-License-Identifier: EUPL-1.2

fn main() -> climax::Result<()> {
    climax::status::message("working")
        .spinner()
        .final_message("done")
        .finish()
}
