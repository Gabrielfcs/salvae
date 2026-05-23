//! (Windows) system process lister via Win32 toolhelp + QueryFullProcessImageNameW.

use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};

use crate::process::{ProcessInfo, ProcessLister};
use crate::WatchError;

/// Lists running processes (with full exe paths) using the Windows toolhelp API.
pub struct SystemProcessLister;

impl ProcessLister for SystemProcessLister {
    fn list(&self) -> Result<Vec<ProcessInfo>, WatchError> {
        // SAFETY: standard toolhelp snapshot iteration; handles are closed below.
        unsafe { list_processes() }
    }
}

/// Resolve the full image path of a process by id, or `None` if it cannot be
/// opened (e.g. protected/system processes) or queried.
///
/// # Safety
/// Calls Win32 process APIs; the opened handle is always closed before return.
unsafe fn full_path(pid: u32) -> Option<PathBuf> {
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut size = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
    CloseHandle(handle);
    if ok == 0 {
        return None;
    }
    let os = std::ffi::OsString::from_wide(&buf[..size as usize]);
    Some(PathBuf::from(os))
}

/// Enumerate processes via a toolhelp snapshot.
///
/// # Safety
/// Creates and closes a toolhelp snapshot handle and iterates it per the Win32
/// contract.
unsafe fn list_processes() -> Result<Vec<ProcessInfo>, WatchError> {
    let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(WatchError::Lister("CreateToolhelp32Snapshot failed".into()));
    }

    let mut entry: PROCESSENTRY32W = std::mem::zeroed();
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut out = Vec::new();
    if Process32FirstW(snapshot, &mut entry) != 0 {
        loop {
            let pid = entry.th32ProcessID;
            if let Some(path) = full_path(pid) {
                out.push(ProcessInfo { pid, exe_path: path });
            }
            if Process32NextW(snapshot, &mut entry) == 0 {
                break;
            }
        }
    }
    CloseHandle(snapshot);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_running_processes() {
        let lister = SystemProcessLister;
        let procs = lister.list().unwrap();
        // There are always running processes; at least one should resolve a path
        // (the test runner itself is openable by the current user).
        assert!(!procs.is_empty());
        assert!(procs.iter().any(|p| !p.exe_path.as_os_str().is_empty()));
    }
}
