use std::{path::PathBuf, process::Command};

#[test]
fn documented_dependency_stories_compile_in_isolation() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("climax crate lives under the workspace root")
        .to_owned();
    let fixtures = root.join("tests/compile-fixtures/Cargo.toml");
    let target = root.join("target/compile-fixtures");
    let output = Command::new(env!("CARGO"))
        .args([
            "check",
            "--workspace",
            "--all-targets",
            "--locked",
            "--offline",
            "--manifest-path",
        ])
        .arg(&fixtures)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .expect("run Cargo for dependency fixtures");

    assert!(
        output.status.success(),
        "dependency fixtures failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
