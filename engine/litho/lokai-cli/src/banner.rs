//! Startup banner for the interactive agent path.

pub fn print_startup_banner() {
    println!(
        "\
 ██╗       ██████╗ ██╗  ██╗ █████╗ ██╗
 ██║      ██╔═══██╗██║ ██╔╝██╔══██╗██║
 ██║      ██║   ██║█████╔╝ ███████║██║
 ██║      ██║   ██║██╔═██╗ ██╔══██║██║
 ███████╗ ╚██████╔╝██║  ██╗██║  ██║██║
 ╚══════╝  ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝
  local-only coding agent"
    );
    crate::help::print_startup_hint();
}
