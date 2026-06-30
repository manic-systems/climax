fn main() -> climax::Result<()> {
    climax::status::message("working")
        .spinner()
        .final_message("done")
        .finish()
}
