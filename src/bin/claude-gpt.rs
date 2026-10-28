#[cfg(unix)]
fn main() {
    if let Err(error) = claude_code_proxy::bridge::run() {
        eprintln!("Claude Bridge: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("Claude Bridge desktop currently supports macOS Apple Silicon.");
    std::process::exit(1);
}
