// SPDX-License-Identifier: EUPL-1.2

use climax::status;

fn main() -> climax::Result<()> {
    status::message("working")
        .spinner()
        .final_message("done")
        .finish()
}
