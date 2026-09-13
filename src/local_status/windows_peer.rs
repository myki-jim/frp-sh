//! A pipe name or ACL alone does not authenticate its server process.
use std::{io, os::windows::io::AsRawHandle};
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{
        Pipes::GetNamedPipeServerProcessId,
        Services::*,
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};
pub fn verify(pipe: &super::transport::Client, role: &str) -> io::Result<()> {
    unsafe {
        let mut pid = 0;
        if GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !process.is_null() {
            let owner = super::transport::identity_of(process);
            CloseHandle(process);
            if owner.is_ok_and(|sid| super::identity().is_ok_and(|mine| mine == sid)) {
                return Ok(());
            }
        }
        let name = match role {
            "agent" => "FrpShClient",
            "server_agent" => "FrpShServer",
            _ => return Err(denied()),
        };
        if service_pid(name) == Some(pid) {
            return Ok(());
        }
        Err(denied())
    }
}
fn denied() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "untrusted status process")
}
unsafe fn service_pid(name: &str) -> Option<u32> {
    let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
    if manager.is_null() {
        return None;
    }
    let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
    let service = OpenServiceW(manager, wide.as_ptr(), SERVICE_QUERY_STATUS);
    CloseServiceHandle(manager);
    if service.is_null() {
        return None;
    }
    let mut status: SERVICE_STATUS_PROCESS = std::mem::zeroed();
    let mut needed = 0;
    let ok = QueryServiceStatusEx(
        service,
        SC_STATUS_PROCESS_INFO,
        (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
        std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
        &mut needed,
    );
    CloseServiceHandle(service);
    (ok != 0 && status.dwCurrentState == SERVICE_RUNNING && status.dwProcessId != 0)
        .then_some(status.dwProcessId)
}
