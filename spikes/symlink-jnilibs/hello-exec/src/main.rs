use std::io::Write;

fn main() {
    std::io::stdout().write_all(b"SYMLINK_EXEC_TOKEN_OK\n").unwrap();
    std::process::exit(42);
}
