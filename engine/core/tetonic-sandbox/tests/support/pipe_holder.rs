//! Bounded native fixture for inherited pipes and service cleanup.
//! No network or filesystem mutation; an uncontained child exits after 4 seconds.
fn main() {
    if std::env::args().nth(1).as_deref() == Some("child") {
        std::thread::sleep(std::time::Duration::from_secs(4));
    } else {
        let _child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("child")
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap();
    }
}
