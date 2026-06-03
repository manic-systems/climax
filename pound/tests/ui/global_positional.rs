use pound::Parse;

// `global` requires a named flag/option, not a positional
#[derive(Parse)]
struct Bad {
    #[pound(global)]
    name: String,
}

fn main() {}
