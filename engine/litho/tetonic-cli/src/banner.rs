//! Startup banner for the interactive agent path.

pub fn print_startup_banner() {
    println!("Tetonic\n  Agent runtime client");
    crate::help::print_startup_hint();
}
