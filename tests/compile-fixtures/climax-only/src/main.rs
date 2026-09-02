fn main() -> climax::Result<()> {
    climax::run_with((), |context, ()| {
        let _format = context.output_format();
        Ok(())
    })
}
