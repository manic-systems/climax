// SPDX-License-Identifier: EUPL-1.2

use pound::Parse;

struct HexByte(u8);

fn hex_byte(_: &str) -> Result<HexByte, &'static str> {
    Ok(HexByte(0))
}

#[derive(Parse)]
struct Cli {
    #[pound(long, parse = "hex_byte", min = "0x01")]
    byte: HexByte,
}

fn main() {}
