use serde::Deserialize;
#[cfg(windows)]
use std::time::Duration;
use std::{future::Future, io, path::PathBuf};
#[cfg(windows)]
pub type Stream = tokio::net::windows::named_pipe::NamedPipeClient;
#[cfg(unix)]
pub type Stream = tokio::net::UnixStream;
#[cfg(windows)]
pub type Accepted = tokio::net::windows::named_pipe::NamedPipeServer;
#[cfg(unix)]
pub type Accepted = tokio::net::UnixStream;
#[cfg(windows)]
const ENDPOINT: &str = r"\\.\pipe\frp-sh-network-v1";
#[cfg(unix)]
const ENDPOINT: &str = "/var/run/frp-sh/network.sock";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[cfg(windows)]
    pub allowed_sid: String,
    #[cfg(unix)]
    pub allowed_uid: u32,
}
impl Policy {
    pub fn load() -> anyhow::Result<Self> {
        #[cfg(windows)]
        let path = std::env::current_exe()?
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Installation path missing"))?
            .join("helper.toml");
        #[cfg(unix)]
        let path = PathBuf::from("/etc/frp-sh/helper.toml");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let m = std::fs::symlink_metadata(&path)?;
            anyhow::ensure!(
                m.is_file() && m.uid() == 0 && m.mode() & 0o022 == 0,
                "Helper policy must be root-owned and not writable by other accounts"
            );
            anyhow::ensure!(
                unsafe { libc::geteuid() } == 0,
                "Start the network helper through the installed system service"
            );
        }
        Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
    }
}
pub async fn connect() -> io::Result<Stream> {
    #[cfg(unix)]
    {
        let s = tokio::net::UnixStream::connect(ENDPOINT).await?;
        if s.peer_cred()?.uid() != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Untrusted helper identity",
            ));
        }
        Ok(s)
    }
    #[cfg(windows)]
    {
        let until = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            match open_client() {
                Ok(s) => {
                    verify_server(&s).map_err(|e| {
                        io::Error::new(e.kind(), format!("Verify helper process identity: {e}"))
                    })?;
                    return Ok(s);
                }
                Err(e) if e.raw_os_error() == Some(231) && tokio::time::Instant::now() < until => {
                    tokio::time::sleep(Duration::from_millis(25)).await
                }
                Err(e) => return Err(io::Error::new(e.kind(), format!("Open helper pipe: {e}"))),
            }
        }
    }
}
#[cfg(windows)]
fn open_client() -> io::Result<Stream> {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, SECURITY_IDENTIFICATION,
            SECURITY_SQOS_PRESENT,
        },
    };
    let name: Vec<_> = ENDPOINT.encode_utf16().chain(Some(0)).collect();
    unsafe {
        // Request data/attribute rights explicitly; never request FILE_CREATE_PIPE_INSTANCE.
        let h = CreateFileW(
            name.as_ptr(),
            0x00120183,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT,
            std::ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Stream::from_raw_handle(h)
    }
}
#[cfg(windows)]
fn verify_server(pipe: &Stream) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            Pipes::GetNamedPipeServerProcessId,
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };
    unsafe {
        let mut pid = 0;
        if GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut path = [0u16; 32768];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len);
        CloseHandle(process);
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let actual = PathBuf::from(String::from_utf16_lossy(&path[..len as usize]));
        let expected = std::env::current_exe()?
            .parent()
            .ok_or_else(|| io::Error::other("Installation path missing"))?
            .join("frp-sh-net.exe");
        if !actual
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.to_string_lossy())
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Network helper is outside this installation",
            ));
        }
    }
    Ok(())
}
#[cfg(windows)]
fn pipe(policy: &Policy, first: bool) -> io::Result<Accepted> {
    use tokio::net::windows::named_pipe::ServerOptions;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SECURITY_ATTRIBUTES,
        },
    };
    if !policy.allowed_sid.starts_with("S-1-")
        || !policy
            .allowed_sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
    {
        return Err(io::Error::other("Invalid allowed SID"));
    }
    // Individual rights exclude FILE_CREATE_PIPE_INSTANCE; reject network clients.
    let descriptor = format!("D:P(A;;GA;;;SY)(A;;0x0012019b;;;{})", policy.allowed_sid);
    let wide: Vec<_> = descriptor.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut sd = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut sd,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        let result = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(32)
            .create_with_security_attributes_raw(ENDPOINT, &attrs as *const _ as *mut _);
        LocalFree(sd);
        result
    }
}
pub async fn listen<F, Fut>(policy: Policy, handler: F) -> anyhow::Result<()>
where
    F: Fn(Accepted) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut tasks = tokio::task::JoinSet::new();
    #[cfg(windows)]
    grant_identity_query(&policy)?;
    #[cfg(windows)]
    let mut listener = pipe(&policy, true)?;
    #[cfg(unix)]
    let listener = {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let directory = PathBuf::from(ENDPOINT).parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&directory)?;
        let m = std::fs::symlink_metadata(&directory)?;
        anyhow::ensure!(
            m.is_dir() && m.uid() == 0 && m.mode() & 0o022 == 0,
            "Unsafe socket directory"
        );
        // Never unlink an endpoint belonging to an already running service.
        if tokio::net::UnixStream::connect(ENDPOINT).await.is_ok() {
            anyhow::bail!("Network helper already running");
        }
        if std::fs::symlink_metadata(ENDPOINT).is_ok() {
            std::fs::remove_file(ENDPOINT)?;
        }
        let socket = tokio::net::UnixListener::bind(ENDPOINT)?;
        std::fs::set_permissions(ENDPOINT, std::fs::Permissions::from_mode(0o600))?;
        let path = std::ffi::CString::new(ENDPOINT)?;
        anyhow::ensure!(
            unsafe { libc::chown(path.as_ptr(), policy.allowed_uid, u32::MAX) } == 0,
            "Cannot grant IPC access to the installed user"
        );
        socket
    };
    let shutdown = super::service::shutdown();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            _=&mut shutdown=>break,
            _=tasks.join_next(),if !tasks.is_empty()=>{},
            incoming=async{
                #[cfg(windows)]{
                    listener.connect().await?;
                    let next=pipe(&policy,false)?;
                    Ok::<_,io::Error>(std::mem::replace(&mut listener,next))
                }
                #[cfg(unix)]{
                    let (s,_)=listener.accept().await?;
                    if s.peer_cred()?.uid()!=policy.allowed_uid{return Err(io::Error::new(io::ErrorKind::PermissionDenied,"Unauthorized helper caller"));}
                    Ok::<_,io::Error>(s)
                }
            }=>{
                if let Ok(s)=incoming{if tasks.len()<30{tasks.spawn(handler(s));}}
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    #[cfg(unix)]
    let _ = std::fs::remove_file(ENDPOINT);
    Ok(())
}

/// Permit only executable-identity queries; do not grant memory, token duplication,
/// termination, handle duplication, or pipe-instance creation to the client.
#[cfg(windows)]
fn grant_identity_query(policy: &Policy) -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SetSecurityInfo,
                SE_KERNEL_OBJECT,
            },
            GetSecurityDescriptorDacl, DACL_SECURITY_INFORMATION,
        },
        System::Threading::GetCurrentProcess,
    };
    if !policy.allowed_sid.starts_with("S-1-")
        || !policy
            .allowed_sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
    {
        return Err(io::Error::other("Invalid allowed SID"));
    }
    let descriptor = format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x1000;;;{})",
        policy.allowed_sid
    );
    let wide: Vec<_> = descriptor.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut sd = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut sd,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = std::ptr::null_mut();
        if GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted) == 0
            || present == 0
            || acl.is_null()
        {
            let error = io::Error::last_os_error();
            LocalFree(sd);
            return Err(error);
        }
        let code = SetSecurityInfo(
            GetCurrentProcess(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null(),
        );
        LocalFree(sd);
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
    }
    Ok(())
}
