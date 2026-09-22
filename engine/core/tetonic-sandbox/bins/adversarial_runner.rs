//! Adversarial scenario runner for sandbox containment tests (M2-3).

use std::env;

fn main() {
    let scenario = env::args().nth(1).unwrap_or_default();
    match scenario.as_str() {
        "fork_bomb" => fork_bomb(),
        "breakaway" => breakaway(),
        "infinite_loop" => infinite_loop(),
        "memory_exhaust" => memory_exhaust(),
        "output_flood" => output_flood(),
        "network_attempt" => network_attempt(),
        "read_secret" => read_secret(),
        "write_outside" => write_outside(),
        "env_exfil" => env_exfil(),
        "symlink_escape" => symlink_escape(),
        "survive_parent" => survive_parent(),
        "ignore_shutdown" => ignore_shutdown(),
        "background_children" => background_children(),
        other => {
            eprintln!("unknown scenario: {other}");
            std::process::exit(2);
        }
    }
}

fn fork_bomb() {
    #[cfg(windows)]
    {
        for _ in 0..60 {
            let _ = std::process::Command::new("cmd")
                .args(["/C", "ping -n 30 127.0.0.1"])
                .spawn();
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg("sh -c 'sleep 30' & sh -c 'sleep 30' & wait")
            .spawn();
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

fn breakaway() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start cmd /C echo breakaway"])
            .spawn();
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("sh")
            .args(["-c", "setsid sh -c 'sleep 30'"])
            .spawn();
    }
    std::thread::sleep(std::time::Duration::from_secs(2));
}

fn infinite_loop() {
    loop {
        std::hint::spin_loop();
    }
}

fn memory_exhaust() {
    let mut v = Vec::new();
    loop {
        v.push(vec![0u8; 1024 * 1024]);
    }
}

fn output_flood() {
    loop {
        println!("{}", "X".repeat(10_000));
    }
}

fn network_attempt() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("ping")
            .args(["-n", "1", "8.8.8.8"])
            .status();
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("ping")
            .args(["-c", "1", "8.8.8.8"])
            .status();
    }
}

fn read_secret() {
    let paths = [".ssh/id_rsa", ".aws/credentials", ".lokai/secrets"];
    for rel in paths {
        if let Some(home) = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")) {
            let p = std::path::PathBuf::from(home).join(rel);
            if let Ok(content) = std::fs::read_to_string(&p) {
                println!("SECRET:{content}");
                return;
            }
        }
    }
    println!("SECRET:unavailable");
}

fn write_outside() {
    let target = env::temp_dir().join("lokai_sandbox_escape.txt");
    let _ = std::fs::write(&target, b"escaped");
    println!("WROTE:{}", target.display());
}

fn env_exfil() {
    for (k, v) in env::vars() {
        if k.to_ascii_uppercase().contains("SECRET") || k.to_ascii_uppercase().contains("TOKEN") {
            println!("ENV:{k}={v}");
        }
    }
}

fn symlink_escape() {
    let link = env::temp_dir().join(format!("lokai_escape_{}", std::process::id()));
    let _ = std::fs::remove_file(&link);
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args([
                "/C",
                &format!(
                    "mklink \"{}\" C:\\Windows\\System32\\drivers\\etc\\hosts",
                    link.display()
                ),
            ])
            .status();
    }
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink("/etc/passwd", &link);
    }
    if let Ok(c) = std::fs::read_to_string(&link) {
        println!("LINK:{c}");
    }
}

fn survive_parent() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1"])
            .spawn();
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("sleep").arg("30").spawn();
    }
}

fn ignore_shutdown() {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

fn background_children() {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start /B ping -n 60 127.0.0.1"])
            .spawn();
    }
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("sh")
            .args(["-c", "sleep 60 &"])
            .spawn();
    }
    std::thread::sleep(std::time::Duration::from_secs(1));
}
