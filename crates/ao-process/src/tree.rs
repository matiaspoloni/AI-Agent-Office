//! Platform-specific ownership of a child's process tree.

use tokio::process::{Child, Command};

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    pub fn prepare(cmd: &mut Command) {
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    /// A Job Object that kills every process in it when terminated or closed.
    pub struct ProcessTree {
        job: HANDLE,
    }

    // The handle is only used through thread-safe Win32 calls.
    unsafe impl Send for ProcessTree {}
    unsafe impl Sync for ProcessTree {}

    impl ProcessTree {
        pub fn attach(child: &Child, pid: u32) -> Self {
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                tracing::warn!(pid, err = %std::io::Error::last_os_error(), "CreateJobObjectW failed; tree kill unavailable");
                return Self { job };
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            let assigned = match child.raw_handle() {
                Some(process) if configured != 0 => {
                    (unsafe { AssignProcessToJobObject(job, process as HANDLE) }) != 0
                }
                _ => false,
            };
            if !assigned {
                tracing::warn!(pid, err = %std::io::Error::last_os_error(), "could not assign process to job object");
            }
            Self { job }
        }

        pub fn terminate(&self, _running: bool) {
            if !self.job.is_null() {
                // Only processes inside *our* job are affected.
                unsafe { TerminateJobObject(self.job, 1) };
            }
        }
    }

    impl Drop for ProcessTree {
        fn drop(&mut self) {
            if !self.job.is_null() {
                // KILL_ON_JOB_CLOSE: closing the last handle ends the tree.
                unsafe { CloseHandle(self.job) };
            }
        }
    }

    pub fn is_process_alive(pid: u32) -> bool {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut code);
            CloseHandle(handle);
            ok != 0 && code == STILL_ACTIVE as u32
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::*;

    pub fn prepare(cmd: &mut Command) {
        // New process group whose id is the child's pid.
        cmd.process_group(0);
    }

    pub struct ProcessTree {
        pgid: i32,
    }

    impl ProcessTree {
        pub fn attach(_child: &Child, pid: u32) -> Self {
            Self { pgid: pid as i32 }
        }

        /// Kills the process group. Only done while our child has not been
        /// reaped: afterwards the id could in theory be reused by someone else.
        pub fn terminate(&self, running: bool) {
            if self.pgid > 0 && running {
                unsafe {
                    libc::killpg(self.pgid, libc::SIGKILL);
                }
            }
        }
    }

    pub fn is_process_alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        let result = unsafe { libc::kill(pid as i32, 0) };
        result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

pub use imp::{is_process_alive, prepare, ProcessTree};
