//! Windows job cancellation: enumerate, terminate, and observe process exit.
use super::{terminate_job, OwnedNativeHandle};
use crate::types::SandboxError;
use std::{
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::Foundation::{GetLastError, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::JobObjects::TerminateJobObject;
use windows_sys::Win32::System::Threading::WaitForSingleObject;

// Confirm all job members have exited before acknowledging cooperative cancellation.
pub(super) fn terminate_canceled_job(job: HANDLE) -> Result<(), SandboxError> {
    use windows_sys::Win32::System::JobObjects::{
        JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList,
        QueryInformationJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
    // Snapshot owned members before termination: ActiveProcesses may reach zero
    // slightly before the corresponding process objects become signaled.
    #[repr(C)]
    struct ProcessIds {
        assigned: u32,
        count: u32,
        ids: [usize; 1024],
    }
    let mut ids = Box::new(ProcessIds {
        assigned: 0,
        count: 0,
        ids: [0; 1024],
    });
    let listed = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicProcessIdList,
            (&mut *ids as *mut ProcessIds).cast(),
            std::mem::size_of::<ProcessIds>() as u32,
            ptr::null_mut(),
        )
    };
    if listed == 0 || ids.count as usize > ids.ids.len() {
        terminate_job(job);
        return Err(SandboxError::Internal(
            "cannot enumerate canceled job members".into(),
        ));
    }
    let mut handles = Vec::new();
    for &pid in &ids.ids[..ids.count as usize] {
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid as u32) };
        if !handle.is_null() {
            handles.push(OwnedNativeHandle(handle));
        } else if unsafe { GetLastError() } != 87 {
            // ERROR_INVALID_PARAMETER: exited before open
            terminate_job(job);
            return Err(SandboxError::Internal(
                "cannot observe canceled job member".into(),
            ));
        }
    }
    if unsafe { TerminateJobObject(job, 1) } == 0 {
        return Err(SandboxError::Internal("job termination failed".into()));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(SandboxError::Internal(
                "cannot confirm job termination".into(),
            ));
        }
        let signaled = handles
            .iter()
            .all(|handle| unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0);
        if info.ActiveProcesses == 0 && signaled {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(SandboxError::Internal(
                "job termination did not quiesce".into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
