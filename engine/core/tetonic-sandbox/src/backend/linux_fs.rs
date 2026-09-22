//! Linux OS filesystem confinement via Landlock LSM (R20 / M2-3).
//!
//! Unprivileged Landlock (Linux 5.13+) enforces workspace-scoped filesystem bounds
//! on the process and all descendants. Capability is advertised only when Landlock
//! is available on the running kernel.

use std::path::Path;

use crate::types::FilesystemPolicy;

#[cfg(target_os = "linux")]
const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
#[cfg(target_os = "linux")]
const SYS_LANDLOCK_ADD_RULE: libc::c_long = 445;
#[cfg(target_os = "linux")]
const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;

#[cfg(target_os = "linux")]
const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;
#[cfg(target_os = "linux")]
const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;

#[cfg(target_os = "linux")]
const ACCESS_FS_EXECUTE: u64 = 1 << 0;
#[cfg(target_os = "linux")]
const ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
#[cfg(target_os = "linux")]
const ACCESS_FS_READ_FILE: u64 = 1 << 2;
#[cfg(target_os = "linux")]
const ACCESS_FS_READ_DIR: u64 = 1 << 3;
#[cfg(target_os = "linux")]
const ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
#[cfg(target_os = "linux")]
const ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
#[cfg(target_os = "linux")]
const ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
#[cfg(target_os = "linux")]
const ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
#[cfg(target_os = "linux")]
const ACCESS_FS_MAKE_REG: u64 = 1 << 8;
#[cfg(target_os = "linux")]
const ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
#[cfg(target_os = "linux")]
const ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
#[cfg(target_os = "linux")]
const ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
#[cfg(target_os = "linux")]
const ACCESS_FS_MAKE_SYM: u64 = 1 << 12;

#[cfg(target_os = "linux")]
const ACCESS_FS_RO: u64 = ACCESS_FS_EXECUTE | ACCESS_FS_READ_FILE | ACCESS_FS_READ_DIR;
#[cfg(target_os = "linux")]
const ACCESS_FS_RW: u64 = ACCESS_FS_RO
    | ACCESS_FS_WRITE_FILE
    | ACCESS_FS_REMOVE_DIR
    | ACCESS_FS_REMOVE_FILE
    | ACCESS_FS_MAKE_CHAR
    | ACCESS_FS_MAKE_DIR
    | ACCESS_FS_MAKE_REG
    | ACCESS_FS_MAKE_SOCK
    | ACCESS_FS_MAKE_FIFO
    | ACCESS_FS_MAKE_BLOCK
    | ACCESS_FS_MAKE_SYM;

#[cfg(target_os = "linux")]
#[repr(C)]
struct LandlockRulesetAttr {
    handled_access_fs: u64,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct LandlockPathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

/// True when Landlock is supported by the running Linux kernel.
pub fn filesystem_confinement_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        let res = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                std::ptr::null::<LandlockRulesetAttr>(),
                0,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        };
        res >= 1
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Apply Landlock workspace filesystem confinement in the child after fork, before exec.
#[cfg(target_os = "linux")]
pub fn apply_in_child(policy: &FilesystemPolicy, wd: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    if !filesystem_confinement_available() {
        return Ok(());
    }

    let attr = LandlockRulesetAttr {
        handled_access_fs: ACCESS_FS_RW,
    };
    let ruleset_fd = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr as *const _,
            std::mem::size_of::<LandlockRulesetAttr>(),
            0,
        )
    };
    if ruleset_fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let ruleset_fd = ruleset_fd as i32;

    let add_path = |path: &Path, access: u64| -> std::io::Result<()> {
        let cpath = match CString::new(path.as_os_str().as_bytes()) {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };
        let fd = unsafe {
            libc::open(
                cpath.as_ptr(),
                libc::O_PATH | libc::O_CLOEXEC | libc::O_DIRECTORY,
            )
        };
        if fd < 0 {
            let file_fd = unsafe { libc::open(cpath.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
            if file_fd < 0 {
                return Ok(());
            }
            let path_attr = LandlockPathBeneathAttr {
                allowed_access: access,
                parent_fd: file_fd,
            };
            let ret = unsafe {
                libc::syscall(
                    SYS_LANDLOCK_ADD_RULE,
                    ruleset_fd,
                    LANDLOCK_RULE_PATH_BENEATH,
                    &path_attr as *const _,
                    0,
                )
            };
            unsafe {
                libc::close(file_fd);
            }
            if ret < 0 {
                return Err(std::io::Error::last_os_error());
            }
            return Ok(());
        }

        let path_attr = LandlockPathBeneathAttr {
            allowed_access: access,
            parent_fd: fd,
        };
        let ret = unsafe {
            libc::syscall(
                SYS_LANDLOCK_ADD_RULE,
                ruleset_fd,
                LANDLOCK_RULE_PATH_BENEATH,
                &path_attr as *const _,
                0,
            )
        };
        unsafe {
            libc::close(fd);
        }
        if ret < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    };

    // System standard read-only roots required for binary execution and libraries
    let ro_system_paths = [
        "/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc", "/dev", "/proc", "/sys",
    ];
    for p in &ro_system_paths {
        let _ = add_path(Path::new(p), ACCESS_FS_RO);
    }

    // Temporary root (read-write)
    if let Some(tmp) = &policy.temporary_root {
        let _ = add_path(tmp, ACCESS_FS_RW);
    }
    let _ = add_path(Path::new("/tmp"), ACCESS_FS_RW);

    // Working directory (read-write)
    let _ = add_path(wd, ACCESS_FS_RW);

    // Policy read roots (read-only)
    for root in &policy.read_roots {
        let _ = add_path(&root.path, ACCESS_FS_RO);
    }

    // Policy write roots (read-write)
    for root in &policy.write_roots {
        let _ = add_path(&root.path, ACCESS_FS_RW);
    }

    // Restrict self
    let prctl_ret = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if prctl_ret < 0 {
        let err = std::io::Error::last_os_error();
        unsafe {
            libc::close(ruleset_fd);
        }
        return Err(err);
    }

    let restrict_ret = unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset_fd, 0) };
    unsafe {
        libc::close(ruleset_fd);
    }
    if restrict_ret < 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}
