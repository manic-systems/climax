// SPDX-License-Identifier: EUPL-1.2

use std::io::{
    self,
    Write,
};

pub struct InlineScreenGuard<'a, W>
where
    W: Write,
{
    output: &'a mut W,
    active: bool,
}

impl<'a, W> InlineScreenGuard<'a, W>
where
    W: Write,
{
    pub fn enter(output: &'a mut W) -> io::Result<Self> {
        enter_inline_screen(output)?;
        Ok(Self {
            output,
            active: true,
        })
    }

    pub const fn writer(&mut self) -> &mut W {
        self.output
    }

    pub fn leave(mut self) -> io::Result<()> {
        self.leave_active()
    }

    fn leave_active(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        leave_inline_screen(self.output)
    }
}

impl<W> Drop for InlineScreenGuard<'_, W>
where
    W: Write,
{
    fn drop(&mut self) {
        let _result = self.leave_active();
    }
}

pub fn enter_inline_screen(output: &mut impl Write) -> io::Result<()> {
    output.write_all(b"\x1b[?25l")?;
    output.flush()
}

pub fn leave_inline_screen(output: &mut impl Write) -> io::Result<()> {
    output.write_all(b"\r\x1b[2K\x1b[?25h")?;
    output.flush()
}
