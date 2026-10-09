use climax::status::message;

fn main() -> climax::Result<()> {
    message("working").spinner().final_message("done").finish()
}
