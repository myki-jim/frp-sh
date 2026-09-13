//! Native Windows SCM entry point. The service account is selected at installation.
#[cfg(windows)]
mod windows {
    use std::{
        path::PathBuf,
        sync::{
            atomic::{AtomicPtr, Ordering},
            OnceLock,
        },
    };
    use tokio_util::sync::CancellationToken;
    use windows_sys::Win32::System::Services::*;
    static JOB: OnceLock<PathBuf> = OnceLock::new();
    static NAME: OnceLock<String> = OnceLock::new();
    static STOP: OnceLock<CancellationToken> = OnceLock::new();
    static HANDLE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
    fn report(state: u32, code: u32) {
        let handle = HANDLE.load(Ordering::Acquire);
        if handle.is_null() {
            return;
        }
        let value = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: if state == SERVICE_RUNNING {
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
            } else {
                0
            },
            dwWin32ExitCode: code,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: if matches!(state, SERVICE_START_PENDING | SERVICE_STOP_PENDING) {
                1
            } else {
                0
            },
            dwWaitHint: if matches!(state, SERVICE_START_PENDING | SERVICE_STOP_PENDING) {
                10000
            } else {
                0
            },
        };
        unsafe {
            SetServiceStatus(handle, &value);
        }
    }
    unsafe extern "system" fn control(
        code: u32,
        _: u32,
        _: *mut core::ffi::c_void,
        _: *mut core::ffi::c_void,
    ) -> u32 {
        if code == SERVICE_CONTROL_STOP || code == SERVICE_CONTROL_SHUTDOWN {
            report(SERVICE_STOP_PENDING, 0);
            STOP.get_or_init(CancellationToken::new).cancel();
        }
        0
    }
    unsafe extern "system" fn entry(_: u32, _: *mut windows_sys::core::PWSTR) {
        let name: Vec<_> = NAME
            .get()
            .expect("service name")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let handle = RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control), std::ptr::null());
        if handle.is_null() {
            return;
        }
        HANDLE.store(handle, Ordering::Release);
        let result = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_stack_size(8 * 1024 * 1024)
            .enable_all()
            .build()
            .map_err(anyhow::Error::from)
            .and_then(|runtime| {
                report(SERVICE_START_PENDING, 0);
                runtime.block_on(async {
                    let path = JOB.get().expect("service job");
                    if !super::super::job::Job::load(path)?.server {
                        crate::helper::status().await?;
                    }
                    report(SERVICE_RUNNING, 0);
                    super::super::supervise_with_startup(
                        JOB.get().expect("service job"),
                        STOP.get_or_init(CancellationToken::new).clone(),
                        crate::runtime::SessionManager::default(),
                        true,
                    )
                    .await
                })
            });
        report(SERVICE_STOPPED, if result.is_ok() { 0 } else { 1 });
        HANDLE.store(std::ptr::null_mut(), Ordering::Release);
    }
    pub fn dispatch(path: PathBuf) -> anyhow::Result<()> {
        let job = super::super::job::Job::load(&path)?;
        let name = if job.server {
            "FrpShServer"
        } else {
            "FrpShClient"
        };
        NAME.set(name.into())
            .map_err(|_| anyhow::anyhow!("service already initialized"))?;
        JOB.set(path)
            .map_err(|_| anyhow::anyhow!("service already initialized"))?;
        STOP.get_or_init(CancellationToken::new);
        let mut wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: wide.as_mut_ptr(),
                lpServiceProc: Some(entry),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: std::ptr::null_mut(),
                lpServiceProc: None,
            },
        ];
        anyhow::ensure!(
            unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } != 0,
            "start this entry through Windows Service Control Manager: {}",
            std::io::Error::last_os_error()
        );
        Ok(())
    }
}
#[cfg(windows)]
pub use windows::dispatch;
