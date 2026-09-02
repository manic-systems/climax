fn main() {
    let _prompt = bang::select("shell")
        .choice("bash", "bash")
        .choice("zsh", "zsh");
}
