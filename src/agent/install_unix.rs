//! Root installs the unit; its process runs as the existing non-root installation user.
use anyhow::{ensure, Context};
use sha2::{Digest, Sha256};
use std::{
    ffi::CStr,
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    process::Command,
};
const EXECUTABLE: &str = "/usr/local/lib/frp-sh/frp-sh";
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    uid: u32,
    server: bool,
    config: String,
    job: String,
}
struct Account {
    name: String,
    home: String,
    gid: u32,
}
fn account(uid: u32) -> anyhow::Result<Account> {
    ensure!(
        uid != 0,
        "background services must run as a non-root installation user"
    );
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut found = std::ptr::null_mut();
        let mut buffer = vec![0u8; 65536];
        ensure!(
            libc::getpwuid_r(
                uid,
                &mut pwd,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut found
            ) == 0
                && !found.is_null(),
            "installation account unavailable"
        );
        Ok(Account {
            name: CStr::from_ptr(pwd.pw_name).to_str()?.into(),
            home: CStr::from_ptr(pwd.pw_dir).to_str()?.into(),
            gid: pwd.pw_gid,
        })
    }
}
pub fn install(path: &Path) -> anyhow::Result<()> {
    let uid = unsafe { libc::geteuid() };
    let _ = account(uid)?;
    let mut job = super::job::Job::load(path)?;
    job.check_profile()?;
    let mut config = crate::config::Config::load(Some(&job.config))?;
    config.uuid = Some(
        config
            .uuid
            .unwrap_or_else(crate::config::Config::ensure_identity),
    );
    if job.server {
        config.profiles.clear();
    } else {
        config.profiles.retain(|name, _| name == &job.profile);
        config.server = None;
    }
    job.config = data_directory(uid, job.server).join("config.toml");
    job.owner_sid = None;
    let snapshot = Snapshot {
        uid,
        server: job.server,
        config: toml::to_string(&config)?,
        job: toml::to_string(&job)?,
    };
    let bytes = serde_json::to_vec(&snapshot)?;
    ensure!(
        bytes.len() <= 262144,
        "installation configuration too large"
    );
    let temporary =
        std::env::temp_dir().join(format!("frpsh-service-{}.json", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    let digest = hex::encode(Sha256::digest(&bytes));
    let result = Command::new("/usr/bin/sudo")
        .arg("--")
        .arg(std::env::current_exe()?)
        .args(["--plain", "agent", "install-elevated", "--snapshot"])
        .arg(&temporary)
        .arg("--digest")
        .arg(digest)
        .status();
    let _ = std::fs::remove_file(temporary);
    ensure!(result?.success(), "system service installation failed");
    Ok(())
}
fn data_directory(uid: u32, server: bool) -> PathBuf {
    PathBuf::from(format!(
        "/var/lib/frp-sh/{uid}/{}",
        if server { "server" } else { "client" }
    ))
}
fn directory(path: &Path, uid: u32, gid: u32, mode: u32) -> anyhow::Result<()> {
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && (metadata.uid() == 0 || metadata.uid() == uid)
            && metadata.mode() & 0o022 == 0,
        "unsafe service directory"
    );
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    ensure!(
        unsafe { libc::chown(name.as_ptr(), uid, gid) } == 0,
        "cannot assign service directory"
    );
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}
fn atomic_file(
    path: &Path,
    bytes: &[u8],
    uid: u32,
    gid: u32,
    mode: u32,
    staging: &Path,
) -> anyhow::Result<()> {
    let tmp = staging.join(format!(".install-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        ensure!(
            unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } == 0,
            "cannot assign service file"
        );
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
fn trusted_file(path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
        "untrusted system service file"
    );
    let mut bytes = Vec::new();
    file.take(262145).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 262144, "system service file too large");
    Ok(Some(bytes))
}
fn is_loaded(server: bool) -> anyhow::Result<bool> {
    let role = if server { "server" } else { "client" };
    #[cfg(target_os = "linux")]
    let result = Command::new("/bin/systemctl")
        .args(["is-active", "--quiet", &format!("frp-sh-{role}.service")])
        .status()?;
    #[cfg(target_os = "macos")]
    let result = Command::new("/bin/launchctl")
        .args(["print", &format!("system/com.frpsh.{role}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    Ok(result.success())
}
#[cfg(target_os = "linux")]
fn systemd_quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}
#[cfg(any(test, target_os = "macos"))]
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn unit(snapshot: &Snapshot, owner: &Account, data: &Path) -> anyhow::Result<(PathBuf, String)> {
    ensure!(
        !owner.home.chars().any(char::is_control) && owner.home.starts_with('/'),
        "invalid installation home"
    );
    let role = if snapshot.server { "server" } else { "client" };
    let job = data.join("job.toml");
    #[cfg(target_os = "linux")]
    {
        let _ = &owner.name;
        let dependencies = if snapshot.server {
            "After=network-online.target\n"
        } else {
            "After=network-online.target frp-sh-network.service\nRequires=frp-sh-network.service\n"
        };
        Ok((PathBuf::from(format!("/etc/systemd/system/frp-sh-{role}.service")),format!("[Unit]\nDescription=frp-sh {role}\nWants=network-online.target\n{dependencies}\n[Service]\nType=simple\nUser={}\nGroup={}\nEnvironment={}\nEnvironment={}\nExecStart={} --plain agent service --job {}\nWorkingDirectory={}\nRestart=on-failure\nRestartSec=3\nTimeoutStopSec=15\nKillMode=control-group\nNoNewPrivileges=yes\nUMask=0077\n\n[Install]\nWantedBy=multi-user.target\n",snapshot.uid,owner.gid,systemd_quote(&format!("HOME={}",owner.home)),systemd_quote(&format!("FRPSH_LOG_DIR={}/logs",data.display())),systemd_quote(EXECUTABLE),systemd_quote(&job.to_string_lossy()),data.display())))
    }
    #[cfg(target_os = "macos")]
    {
        Ok((PathBuf::from(format!("/Library/LaunchDaemons/com.frpsh.{role}.plist")),format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>com.frpsh.{role}</string><key>UserName</key><string>{}</string><key>ProgramArguments</key><array><string>{EXECUTABLE}</string><string>--plain</string><string>agent</string><string>service</string><string>--job</string><string>{}</string></array><key>EnvironmentVariables</key><dict><key>HOME</key><string>{}</string><key>FRPSH_LOG_DIR</key><string>{}/logs</string></dict><key>WorkingDirectory</key><string>{}</string><key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>ThrottleInterval</key><integer>3</integer><key>ExitTimeOut</key><integer>15</integer></dict></plist>\n",xml(&owner.name),xml(&job.to_string_lossy()),xml(&owner.home),xml(&data.to_string_lossy()),xml(&data.to_string_lossy()))))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    anyhow::bail!("native service manager not supported")
}
fn manager(server: bool, start: bool, path: &Path) -> anyhow::Result<()> {
    let role = if server { "server" } else { "client" };
    #[cfg(target_os = "linux")]
    {
        let _ = path;
        ensure!(
            Path::new("/run/systemd/system").is_dir(),
            "systemd is not running"
        );
        ensure!(
            Command::new("/bin/systemctl")
                .arg("daemon-reload")
                .status()?
                .success(),
            "systemd reload failed"
        );
        let status = if start {
            ensure!(
                Command::new("/bin/systemctl")
                    .args(["enable", &format!("frp-sh-{role}.service")])
                    .status()?
                    .success(),
                "systemd enable failed"
            );
            Command::new("/bin/systemctl")
                .args(["start", &format!("frp-sh-{role}.service")])
                .status()?
        } else {
            Command::new("/bin/systemctl")
                .args(["stop", &format!("frp-sh-{role}.service")])
                .status()?
        };
        ensure!(status.success(), "systemd service operation failed");
    }
    #[cfg(target_os = "macos")]
    {
        let status = if start {
            Command::new("/bin/launchctl")
                .arg("bootstrap")
                .arg("system")
                .arg(path)
                .status()?
        } else {
            Command::new("/bin/launchctl")
                .arg("bootout")
                .arg(format!("system/com.frpsh.{role}"))
                .status()?
        };
        ensure!(status.success(), "launchd service operation failed");
    }
    Ok(())
}
pub fn elevated_install(path: &Path, digest: &str) -> anyhow::Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "service installation requires root"
    );
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    ensure!(file.metadata()?.is_file(), "invalid installation snapshot");
    let mut bytes = Vec::new();
    file.take(262145).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 262144 && hex::encode(Sha256::digest(&bytes)) == digest,
        "installation snapshot changed"
    );
    let snapshot: Snapshot = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("invalid installation snapshot"))?;
    ensure!(
        !snapshot.server || cfg!(feature = "server"),
        "server jobs require the full binary"
    );
    let owner = account(snapshot.uid)?;
    ensure!(
        std::env::current_exe()?.canonicalize()? == Path::new(EXECUTABLE).canonicalize()?,
        "run the verified installed executable"
    );
    let binary = std::fs::metadata(EXECUTABLE)?;
    ensure!(
        binary.uid() == 0 && binary.mode() & 0o022 == 0,
        "installation executable must be root-owned"
    );
    if !snapshot.server {
        let bytes = trusted_file(Path::new("/etc/frp-sh/helper.toml"))?
            .context("network helper is not installed")?;
        let policy: toml::Value = toml::from_str(std::str::from_utf8(&bytes)?)?;
        ensure!(
            policy.get("allowed_uid").and_then(|v| v.as_integer()) == Some(snapshot.uid as i64),
            "network helper belongs to a different user"
        );
    }
    let data = data_directory(snapshot.uid, snapshot.server);
    let job: super::job::Job =
        toml::from_str(&snapshot.job).map_err(|_| anyhow::anyhow!("invalid service job"))?;
    job.validate()?;
    ensure!(
        job.config == data.join("config.toml")
            && job.server == snapshot.server
            && job.owner_sid.is_none(),
        "service job destination mismatch"
    );
    if !Path::new("/var/lib").exists() {
        directory(Path::new("/var/lib"), 0, 0, 0o755)?;
    }
    directory(Path::new("/var/lib/frp-sh"), 0, 0, 0o755)?;
    let staging = data.parent().context("service parent missing")?;
    directory(staging, 0, 0, 0o711)?;
    directory(&data, snapshot.uid, owner.gid, 0o700)?;
    directory(Path::new("/etc/frp-sh"), 0, 0, 0o755)?;
    let owner_path = PathBuf::from(format!(
        "/etc/frp-sh/{}-owner",
        if snapshot.server { "server" } else { "client" }
    ));
    let old_owner = trusted_file(&owner_path)?;
    if let Some(value) = &old_owner {
        ensure!(
            std::str::from_utf8(value)?.trim() == snapshot.uid.to_string(),
            "service belongs to another installation user"
        );
    }
    let (unit_path, text) = unit(&snapshot, &owner, &data)?;
    let old_unit = trusted_file(&unit_path)?;
    let was_loaded = is_loaded(snapshot.server)?;
    ensure!(
        !was_loaded || old_unit.is_some(),
        "service is managed outside this installer"
    );
    #[cfg(target_os = "linux")]
    let was_enabled = Command::new("/bin/systemctl")
        .args([
            "is-enabled",
            "--quiet",
            unit_path.file_name().unwrap().to_str().unwrap(),
        ])
        .status()?
        .success();
    if was_loaded {
        manager(snapshot.server, false, &unit_path)?;
    }
    let mut backups = Vec::new();
    let mut written = Vec::new();
    let result = (|| {
        for (name, content) in [
            ("config.toml", snapshot.config.as_bytes()),
            ("job.toml", snapshot.job.as_bytes()),
        ] {
            let dest = data.join(name);
            if dest.try_exists()? {
                let backup = staging.join(format!(".previous-{}", uuid::Uuid::new_v4()));
                std::fs::rename(&dest, &backup)?;
                backups.push((dest.clone(), backup));
            }
            atomic_file(&dest, content, snapshot.uid, owner.gid, 0o600, staging)?;
            written.push(dest);
        }
        atomic_file(
            &unit_path,
            text.as_bytes(),
            0,
            0,
            0o644,
            unit_path.parent().context("unit directory missing")?,
        )?;
        atomic_file(
            &owner_path,
            snapshot.uid.to_string().as_bytes(),
            0,
            0,
            0o644,
            owner_path.parent().context("owner directory missing")?,
        )?;
        manager(snapshot.server, true, &unit_path)?;
        Ok::<(), anyhow::Error>(())
    })();
    if result.is_err() {
        let _ = manager(snapshot.server, false, &unit_path);
        for path in written {
            let _ = std::fs::remove_file(path);
        }
        for (path, backup) in &backups {
            std::fs::rename(backup, path)?;
        }
        if let Some(old) = old_unit {
            atomic_file(&unit_path, &old, 0, 0, 0o644, unit_path.parent().unwrap())?;
            if was_loaded {
                let _ = manager(snapshot.server, true, &unit_path);
            }
        } else {
            let _ = std::fs::remove_file(&unit_path);
        }
        if let Some(old) = old_owner {
            atomic_file(&owner_path, &old, 0, 0, 0o644, owner_path.parent().unwrap())?;
        } else {
            let _ = std::fs::remove_file(&owner_path);
        }
        #[cfg(target_os = "linux")]
        {
            if !was_enabled {
                let _ = Command::new("/bin/systemctl")
                    .arg("disable")
                    .arg(unit_path.file_name().unwrap())
                    .status();
            }
            let _ = Command::new("/bin/systemctl").arg("daemon-reload").status();
        }
    } else {
        for (_, backup) in backups {
            let _ = std::fs::remove_file(backup);
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_units_run_as_owner_and_contain_no_credentials() {
        let spec = Snapshot {
            uid: 1001,
            server: false,
            config: "password = 'test-password'".into(),
            job: String::new(),
        };
        let account = Account {
            name: "test-user".into(),
            home: "/home/test-user".into(),
            gid: 1001,
        };
        let (_, value) = unit(&spec, &account, Path::new("/var/lib/frp-sh/1001/client")).unwrap();
        assert!(value.contains("agent"));
        assert!(!value.contains("test-password"));
        assert!(!value.contains("User=0"));
        #[cfg(target_os = "linux")]
        assert!(value.contains("\nWorkingDirectory=/var/lib/frp-sh/1001/client\n"));
        assert_eq!(xml("<&\""), "&lt;&amp;&quot;");
    }
}
