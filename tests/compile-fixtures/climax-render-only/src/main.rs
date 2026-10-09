// SPDX-License-Identifier: EUPL-1.2

fn main() -> climax::Result<()> {
    climax::status::message("rendering only").spinner().finish()
}
