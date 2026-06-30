use crate::Result;

pub fn run<C, F>(command: C, f: F) -> Result<()>
where
    F: FnOnce(Context, C) -> Result<()>,
{
    f(Context::new(), command)
}

#[derive(Clone, Debug, Default)]
pub struct Context;

impl Context {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}
