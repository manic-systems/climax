use pound::Parse;

#[derive(Parse)]
struct Bad {
    #[pound(long)]
    a: bool,
    #[pound(long, conflicts_with = "nope")]
    b: bool,
}

fn main() {}
