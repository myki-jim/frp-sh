use std::io;
#[cfg(unix)]
pub type Client = tokio::net::UnixStream;
#[cfg(unix)]
pub type Connection = tokio::net::UnixStream;
#[cfg(windows)]
pub type Client = tokio::net::windows::named_pipe::NamedPipeClient;
#[cfg(windows)]
pub type Connection = tokio::net::windows::named_pipe::NamedPipeServer;

#[cfg(windows)]
pub(crate) fn identity() -> io::Result<String> {
    identity_of(unsafe { windows_sys::Win32::System::Threading::GetCurrentProcess() })
}
#[cfg(windows)]
pub(super) fn identity_of(process: windows_sys::Win32::Foundation::HANDLE) -> io::Result<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
            TOKEN_USER,
        },
        System::Threading::OpenProcessToken,
    };
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut len = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buffer = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
        let ok = GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), len, &mut len);
        let error = io::Error::last_os_error();
        CloseHandle(token);
        if ok == 0 {
            return Err(error);
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut n = 0;
        while *text.add(n) != 0 {
            n += 1;
        }
        let result = String::from_utf16_lossy(std::slice::from_raw_parts(text, n));
        LocalFree(text.cast());
        Ok(result)
    }
}
fn endpoint(role: &str, owner: Option<&str>) -> io::Result<String> {
    if !matches!(role, "host" | "guest" | "serve" | "agent" | "server_agent") {
        return Err(io::Error::other("invalid status role"));
    }
    #[cfg(test)]
    let role = format!("{role}-test-{}", std::process::id());
    #[cfg(windows)]
    {
        let sid = owner.map(str::to_owned).unwrap_or(identity()?);
        if !sid.starts_with("S-1-")
            || sid.len() > 184
            || !sid
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
        {
            return Err(io::Error::other("invalid status owner"));
        }
        Ok(format!(r"\\.\pipe\frp-sh-status-{sid}-{role}"))
    }
    #[cfg(unix)]
    {
        let _ = owner;
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        let uid = unsafe { libc::geteuid() };
        let directory = std::path::PathBuf::from(format!("/tmp/frp-sh-status-{uid}"));
        match std::fs::DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let m = std::fs::symlink_metadata(&directory)?;
        if !m.is_dir() || m.uid() != uid || m.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unsafe status directory",
            ));
        }
        Ok(directory
            .join(format!("{role}.sock"))
            .to_string_lossy()
            .into_owned())
    }
}
#[cfg(windows)]
fn pipe(path: &str, first: bool, owner: Option<&str>) -> io::Result<Connection> {
    use tokio::net::windows::named_pipe::ServerOptions;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SECURITY_ATTRIBUTES,
        },
    };
    let current = identity()?;
    let mut descriptor = format!("D:P(A;;GA;;;SY)(A;;GA;;;{current})");
    if let Some(sid) = owner.filter(|s| *s != current) {
        descriptor.push_str(&format!("(A;;0x0012019b;;;{sid})"));
    }
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
            .max_instances(8)
            .create_with_security_attributes_raw(path, &attrs as *const _ as *mut _);
        LocalFree(sd);
        result
    }
}
pub struct Listener {
    #[cfg(unix)]
    socket: tokio::net::UnixListener,
    #[cfg(windows)]
    socket: Connection,
    path: String,
    #[cfg(windows)]
    owner: Option<String>,
}
impl Listener {
    pub async fn bind(role: &str) -> io::Result<Self> {
        Self::bind_for(role, None).await
    }
    pub async fn bind_for(role: &str, owner: Option<&str>) -> io::Result<Self> {
        let path = endpoint(role, owner)?;
        #[cfg(windows)]
        let socket = pipe(&path, true, owner)?;
        #[cfg(unix)]
        let socket = {
            use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
            if let Ok(m) = std::fs::symlink_metadata(&path) {
                if !m.file_type().is_socket() || m.uid() != unsafe { libc::geteuid() } {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "unsafe status endpoint",
                    ));
                }
                match tokio::net::UnixStream::connect(&path).await {
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::AddrInUse,
                            "status already running",
                        ))
                    }
                    Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {
                        std::fs::remove_file(&path)?
                    }
                    Err(e) => return Err(e),
                }
            }
            let socket = tokio::net::UnixListener::bind(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            socket
        };
        Ok(Self {
            socket,
            path,
            #[cfg(windows)]
            owner: owner.map(str::to_owned),
        })
    }
    pub async fn accept(&mut self) -> io::Result<Connection> {
        #[cfg(windows)]
        {
            self.socket.connect().await?;
            let next = pipe(&self.path, false, self.owner.as_deref())?;
            Ok(std::mem::replace(&mut self.socket, next))
        }
        #[cfg(unix)]
        {
            loop {
                let (stream, _) = self.socket.accept().await?;
                // A bind probe can disconnect before accept. On macOS getpeereid
                // then returns ENOTCONN; discard that client, not the listener.
                if stream
                    .peer_cred()
                    .is_ok_and(|cred| cred.uid() == unsafe { libc::geteuid() })
                {
                    return Ok(stream);
                }
            }
        }
    }
}
#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
pub async fn connect(role: &str) -> io::Result<Client> {
    let path = endpoint(role, None)?;
    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(path).await?;
        if stream.peer_cred()?.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "untrusted status server",
            ));
        }
        Ok(stream)
    }
    #[cfg(windows)]
    {
        for _ in 0..10 {
            match open_client(&path) {
                Err(e) if e.raw_os_error() == Some(231) => {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await
                }
                result => {
                    let client = result?;
                    super::windows_peer::verify(&client, role)?;
                    return Ok(client);
                }
            }
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "status pipe busy"))
    }
}

#[cfg(windows)]
fn open_client(path: &str) -> io::Result<Client> {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_OVERLAPPED, OPEN_EXISTING, SECURITY_IDENTIFICATION,
            SECURITY_SQOS_PRESENT,
        },
    };
    let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let handle = CreateFileW(
            path.as_ptr(),
            0x00120183,
            0,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Client::from_raw_handle(handle)
    }
}
