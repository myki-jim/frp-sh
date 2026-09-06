//! OS service lifecycle; service stop cancels sessions and releases owned routes.
#[cfg(windows)]
static STOP: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
pub async fn shutdown() {
    #[cfg(unix)]
    {
        let mut sig = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("signal");
        tokio::select! {_=sig.recv()=>{},_=tokio::signal::ctrl_c()=>{}}
    }
    #[cfg(windows)]
    STOP.get_or_init(Default::default).notified().await;
}
#[cfg(windows)]
pub fn dispatch() -> anyhow::Result<()> {
    use windows_sys::Win32::System::Services::*;
    let mut name: Vec<_> = "FrpShNetwork".encode_utf16().chain(Some(0)).collect();
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(entry),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: std::ptr::null_mut(),
            lpServiceProc: None,
        },
    ];
    anyhow::ensure!(
        unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } != 0,
        "Network helper must be started by Windows Service Control Manager: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}
#[cfg(windows)]
unsafe extern "system" fn control(
    code: u32,
    _: u32,
    _: *mut core::ffi::c_void,
    _: *mut core::ffi::c_void,
) -> u32 {
    use windows_sys::Win32::System::Services::*;
    if code == SERVICE_CONTROL_STOP || code == SERVICE_CONTROL_SHUTDOWN {
        STOP.get_or_init(Default::default).notify_one();
    }
    0
}
#[cfg(windows)]
unsafe extern "system" fn entry(_: u32, _: *mut windows_sys::core::PWSTR) {
    use windows_sys::Win32::System::Services::*;
    let name: Vec<_> = "FrpShNetwork".encode_utf16().chain(Some(0)).collect();
    let handle = RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control), std::ptr::null());
    if handle.is_null() {
        return;
    }
    let mut state = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: SERVICE_RUNNING,
        dwControlsAccepted: SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN,
        dwWin32ExitCode: 0,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: 0,
    };
    SetServiceStatus(handle, &state);
    let result = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|rt| rt.block_on(super::run()));
    state.dwCurrentState = SERVICE_STOPPED;
    state.dwControlsAccepted = 0;
    state.dwWin32ExitCode = if result.is_ok() { 0 } else { 1 };
    SetServiceStatus(handle, &state);
}
#[cfg(not(windows))]
pub fn dispatch() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(super::run())
}
