fn main() {
    if let Err(error) = worktree::run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
