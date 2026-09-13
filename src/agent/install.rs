//! UAC is confined to installation; secrets are transferred in a private snapshot.
#[cfg(windows)]
mod windows {
    use anyhow::{ensure, Context};
    use sha2::{Digest, Sha256};
    use std::{
        io::{Read, Write},
        path::{Path, PathBuf},
        process::Stdio,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree, INVALID_HANDLE_VALUE},
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetTokenInformation, TokenElevation, SECURITY_ATTRIBUTES, TOKEN_ELEVATION, TOKEN_QUERY,
        },
        Storage::FileSystem::{CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_TEMPORARY},
        System::Threading::{
            GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
        },
        UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW},
    };
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Snapshot {
        executable: PathBuf,
        owner_sid: String,
        server: bool,
        config: String,
        job: String,
    }
    fn elevated() -> anyhow::Result<bool> {
        unsafe {
            let mut token = std::ptr::null_mut();
            ensure!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0,
                "cannot inspect installation identity"
            );
            let mut value: TOKEN_ELEVATION = std::mem::zeroed();
            let mut size = 0;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                (&mut value as *mut TOKEN_ELEVATION).cast(),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            );
            CloseHandle(token);
            ensure!(ok != 0, "cannot inspect installation identity");
            Ok(value.TokenIsElevated != 0)
        }
    }
    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().chain(Some(0)).collect()
    }
    fn private_file(path: &Path, bytes: &[u8], owner: &str) -> anyhow::Result<()> {
        use std::os::windows::io::FromRawHandle;
        let descriptor = wide(std::ffi::OsStr::new(&format!(
            "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{owner})"
        )));
        unsafe {
            let mut sd = std::ptr::null_mut();
            ensure!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    descriptor.as_ptr(),
                    1,
                    &mut sd,
                    std::ptr::null_mut()
                ) != 0,
                "cannot protect installation snapshot"
            );
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd,
                bInheritHandle: 0,
            };
            let handle = CreateFileW(
                wide(path.as_os_str()).as_ptr(),
                0x40000000,
                0,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_TEMPORARY,
                std::ptr::null_mut(),
            );
            LocalFree(sd);
            ensure!(
                handle != INVALID_HANDLE_VALUE,
                "cannot create private installation snapshot"
            );
            let mut file = std::fs::File::from_raw_handle(handle);
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Ok(())
    }
    pub fn install(path: &Path) -> anyhow::Result<()> {
        let mut job = super::super::job::Job::load(path)?;
        job.check_profile()?;
        let mut config = crate::config::Config::load(Some(&job.config))?;
        config.uuid = Some(
            config
                .uuid
                .unwrap_or_else(crate::config::Config::ensure_identity),
        );
        if !job.server {
            config.profiles.retain(|name, _| name == &job.profile);
            config.server = None;
        } else {
            config.profiles.clear();
        }
        let owner = crate::local_status::identity()?;
        let role = if job.server { "server" } else { "client" };
        let directory = common_data()?.join("frp-sh").join(role);
        // The elevated script independently checks its OS-derived target path.
        job.config = directory.join("config.toml");
        job.owner_sid = Some(owner.clone());
        let snapshot = Snapshot {
            executable: std::env::current_exe()?,
            owner_sid: owner.clone(),
            server: job.server,
            config: toml::to_string(&config)?,
            job: toml::to_string(&job)?,
        };
        let bytes = serde_json::to_vec(&snapshot)?;
        ensure!(
            bytes.len() <= 262144,
            "installation configuration too large"
        );
        if elevated()? {
            return apply(&bytes);
        }
        let temporary =
            std::env::temp_dir().join(format!("frpsh-service-{}.json", uuid::Uuid::new_v4()));
        private_file(&temporary, &bytes, &owner)?;
        let result = (|| {
            let digest = hex::encode(Sha256::digest(&bytes));
            let params = wide(std::ffi::OsStr::new(&format!(
                "agent install-elevated --snapshot \"{}\" --digest {digest}",
                temporary.display()
            )));
            let executable = wide(snapshot.executable.as_os_str());
            let verb = wide(std::ffi::OsStr::new("runas"));
            unsafe {
                let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
                info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
                info.fMask = SEE_MASK_NOCLOSEPROCESS;
                info.lpVerb = verb.as_ptr();
                info.lpFile = executable.as_ptr();
                info.lpParameters = params.as_ptr();
                info.nShow = 0;
                ensure!(
                    ShellExecuteExW(&mut info) != 0,
                    "service installation approval was cancelled or unavailable"
                );
                WaitForSingleObject(info.hProcess, INFINITE);
                let mut code = 1;
                GetExitCodeProcess(info.hProcess, &mut code);
                CloseHandle(info.hProcess);
                ensure!(code==0,"service installation failed; run from an administrator terminal for diagnostics");
            }
            Ok(())
        })();
        let _ = std::fs::remove_file(temporary);
        result
    }
    pub fn elevated_install(path: &Path, digest: &str) -> anyhow::Result<()> {
        ensure!(
            elevated()?,
            "service installation requires administrator approval"
        );
        let file = std::fs::File::open(path)?;
        ensure!(file.metadata()?.is_file(), "invalid installation snapshot");
        let mut bytes = Vec::new();
        file.take(262145).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 262144 && hex::encode(Sha256::digest(&bytes)) == digest,
            "installation snapshot changed"
        );
        apply(&bytes)
    }
    fn apply(bytes: &[u8]) -> anyhow::Result<()> {
        let snapshot: Snapshot = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid installation snapshot"))?;
        let job: super::super::job::Job = toml::from_str(&snapshot.job)
            .map_err(|_| anyhow::anyhow!("invalid installation job"))?;
        job.validate()?;
        ensure!(
            job.owner_sid.as_deref() == Some(snapshot.owner_sid.as_str())
                && job.server == snapshot.server,
            "installation identity mismatch"
        );
        let mut process = std::process::Command::new(system_powershell()?)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                include_str!("install_windows.ps1"),
            ])
            .stdin(Stdio::piped())
            .spawn()?;
        process
            .stdin
            .take()
            .context("installation input unavailable")?
            .write_all(bytes)?;
        ensure!(process.wait()?.success(), "service installation failed");
        Ok(())
    }
    fn common_data() -> anyhow::Result<PathBuf> {
        use windows_sys::Win32::{
            System::Com::CoTaskMemFree,
            UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
        };
        unsafe {
            let mut path = std::ptr::null_mut();
            ensure!(
                SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, std::ptr::null_mut(), &mut path)
                    >= 0,
                "ProgramData unavailable"
            );
            let mut n = 0;
            while *path.add(n) != 0 {
                n += 1;
            }
            let result = PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(
                path, n,
            )));
            CoTaskMemFree(path.cast());
            Ok(result)
        }
    }
    fn system_powershell() -> anyhow::Result<PathBuf> {
        let mut path = vec![0u16; 32768];
        let length = unsafe {
            windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
                path.as_mut_ptr(),
                path.len() as u32,
            )
        };
        ensure!(
            length > 0 && length < path.len() as u32,
            "system directory unavailable"
        );
        Ok(
            PathBuf::from(String::from_utf16_lossy(&path[..length as usize]))
                .join("WindowsPowerShell/v1.0/powershell.exe"),
        )
    }
}
#[cfg(windows)]
pub use windows::{elevated_install, install};
