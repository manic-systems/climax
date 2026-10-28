// SPDX-License-Identifier: EUPL-1.2

fn main() -> std::process::ExitCode {
    climax::main_with(|_context| Err(anyhow::anyhow!("fixture failure").into()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_anyhow_error_converts_into_a_climax_error() {
        let error: climax::Error = anyhow::anyhow!("fixture failure").into();
        assert!(error.to_string().contains("fixture failure"));
    }
}
