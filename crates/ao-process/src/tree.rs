//! Platform-specific ownership of a child's process tree.

use tokio::process::{Child, Command};

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
        TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, OpenThread, ResumeThread, TerminateProcess,
        CREATE_SUSPENDED, PROCESS_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    /// The child starts suspended: it runs its first instruction only after
    /// it is inside its Job Object (see `ProcessTree::attach`), so nothing it
    /// starts can escape the job.
    pub fn prepare(cmd: &mut Command) {
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);
    }

    /// A Job Object that kills every process in it when terminated or closed.
    pub struct ProcessTree {
        job: HANDLE,
    }

    // The handle is only used through thread-safe Win32 calls.
    unsafe impl Send for ProcessTree {}
    unsafe impl Sync for ProcessTree {}

    /// Resumes every thread of a suspended process (a new process has one).
    fn resume_threads(pid: u32) -> std::io::Result<()> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut resumed = 0;
        let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !thread.is_null() {
                    if unsafe { ResumeThread(thread) } != u32::MAX {
                        resumed += 1;
                    }
                    unsafe { CloseHandle(thread) };
                }
            }
            more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
        }
        unsafe { CloseHandle(snapshot) };
        if resumed == 0 {
            return Err(std::io::Error::other("could not resume the new process"));
        }
        Ok(())
    }

    impl ProcessTree {
        /// Puts the (suspended) child in a new job, then lets it run. If the
        /// child cannot be resumed it is killed and an error is returned.
        pub fn attach(child: &Child, pid: u32) -> std::io::Result<Self> {
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                tracing::warn!(pid, err = %std::io::Error::last_os_error(), "CreateJobObjectW failed; tree kill unavailable");
            } else {
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
            }
            let tree = Self { job };
            if let Err(err) = resume_threads(pid) {
                // Never leave a suspended process behind.
                if tree.job.is_null() {
                    if let Some(process) = child.raw_handle() {
                        unsafe { TerminateProcess(process as HANDLE, 1) };
                    }
                } else {
                    tree.terminate(true);
                }
                return Err(err);
            }
            Ok(tree)
        }

        pub fn terminate(&self, _running: bool) {
            if !self.job.is_null() {
                // Only processes inside *our* job are affected.
                unsafe { TerminateJobObject(self.job, 1) };
            }
        }

        /// Processes currently alive in the tree (the job's active count).
        pub fn process_count(&self) -> Option<u32> {
            if self.job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            let ok = unsafe {
                QueryInformationJobObject(
                    self.job,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION as *mut c_void,
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                )
            };
            (ok != 0).then_some(info.ActiveProcesses)
        }

        /// After the main process exited: stops whatever it left running in
        /// its job. Returns how many processes were still alive.
        pub fn stop_leftovers(&self) -> u32 {
            let left = self.process_count().unwrap_or(0);
            if left > 0 {
                self.terminate(false);
            }
            left
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
        // New process group whose id is the child's pid (set before exec, so
        // everything the child starts is in the group from the beginning).
        cmd.process_group(0);
    }

    pub struct ProcessTree {
        pgid: i32,
    }

    /// Members of a process group, from /proc (Linux only).
    #[cfg(target_os = "linux")]
    fn group_members(pgid: i32) -> Option<u32> {
        let mut count = 0;
        for entry in std::fs::read_dir("/proc").ok()?.flatten() {
            let name = entry.file_name();
            if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            // "pid (comm) state ppid pgrp ...": comm may contain spaces.
            let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
                continue;
            };
            let fields: Vec<&str> = rest.split_whitespace().collect();
            let state = fields.first().copied().unwrap_or("");
            if state != "Z" && fields.get(2).and_then(|g| g.parse::<i32>().ok()) == Some(pgid) {
                count += 1;
            }
        }
        Some(count)
    }

    #[cfg(not(target_os = "linux"))]
    fn group_members(pgid: i32) -> Option<u32> {
        // Without /proc: only "some" or "none".
        let alive = unsafe { libc::killpg(pgid, 0) } == 0;
        Some(u32::from(alive))
    }

    impl ProcessTree {
        pub fn attach(_child: &Child, pid: u32) -> std::io::Result<Self> {
            Ok(Self { pgid: pid as i32 })
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

        pub fn process_count(&self) -> Option<u32> {
            (self.pgid > 0).then(|| group_members(self.pgid)).flatten()
        }

        /// After the group leader exited: stops the members it left running.
        /// A group id stays reserved while the group has members, so it cannot
        /// name someone else's processes at this point.
        pub fn stop_leftovers(&self) -> u32 {
            let left = self.process_count().unwrap_or(0);
            if left > 0 {
                unsafe {
                    libc::killpg(self.pgid, libc::SIGKILL);
                }
            }
            left
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
