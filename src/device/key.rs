//! Local device signing key. No Debug/Serialize implementation exposes its seed.
use anyhow::ensure;
use ed25519_dalek::{Signer, SigningKey};
use std::{
    io::{Read, Write},
    path::Path,
};
pub struct DeviceKey(SigningKey);
impl DeviceKey {
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if path.try_exists()? {
            return Self::load(path);
        }
        let mut bytes = [0u8; 32];
        use rand::RngCore;
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        let encoded = protect(&bytes, true)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        match options.open(path) {
            Ok(mut file) => {
                file.write_all(&encoded)?;
                file.sync_all()?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return Self::load(path),
            Err(e) => return Err(e.into()),
        }
        Ok(Self(SigningKey::from_bytes(&bytes)))
    }
    fn load(path: &Path) -> anyhow::Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.len() <= 16384,
            "invalid device key file"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(
                metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
                "device key must be private to its account"
            );
        }
        let mut encoded = Vec::new();
        file.take(16385).read_to_end(&mut encoded)?;
        ensure!(encoded.len() <= 16384, "invalid device key file");
        let decoded = protect(&encoded, false)?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid device key"))?;
        Ok(Self(SigningKey::from_bytes(&bytes)))
    }
    pub fn public_key(&self) -> String {
        hex::encode(self.0.verifying_key().to_bytes())
    }
    pub fn sign(
        &self,
        challenge: &super::Challenge,
        server: &str,
        action: &str,
    ) -> anyhow::Result<String> {
        ensure!(
            challenge.device == self.public_key()
                && challenge.server == server
                && challenge.action_hash == action,
            "untrusted device challenge"
        );
        let now = crate::utils::now_unix();
        ensure!(
            challenge.expires_at > now && challenge.expires_at <= now.saturating_add(120),
            "expired or invalid device challenge"
        );
        Ok(hex::encode(self.0.sign(&challenge.message()?).to_bytes()))
    }
}
#[cfg(unix)]
fn protect(bytes: &[u8], _: bool) -> anyhow::Result<Vec<u8>> {
    Ok(bytes.to_vec())
}
#[cfg(windows)]
fn protect(bytes: &[u8], encrypt: bool) -> anyhow::Result<Vec<u8>> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::*};
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    unsafe {
        let ok = if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        ensure!(ok != 0, "device key protection failed");
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        // Wipe plaintext allocated by DPAPI before releasing its buffer.
        if !encrypt {
            for n in 0..output.cbData as usize {
                std::ptr::write_volatile(output.pbData.add(n), 0);
            }
        }
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persisted_key_signs_only_the_expected_server_and_operation() {
        let path = std::env::temp_dir().join(format!("frpsh-device-key-{}", uuid::Uuid::new_v4()));
        let key = DeviceKey::load_or_create(&path).unwrap();
        let again = DeviceKey::load_or_create(&path).unwrap();
        assert_eq!(key.public_key(), again.public_key());
        let mut challenges = crate::device::Challenges::default();
        let action = "ab".repeat(32);
        let now = crate::utils::now_unix();
        let challenge = challenges
            .issue("https://example.test", &key.public_key(), &action, now)
            .unwrap();
        assert!(key.sign(&challenge, "https://other.test", &action).is_err());
        assert!(key
            .sign(&challenge, "https://example.test", &"cd".repeat(32))
            .is_err());
        let signature = again
            .sign(&challenge, "https://example.test", &action)
            .unwrap();
        assert_eq!(
            challenges
                .verify(&challenge.nonce, &signature, &action, now)
                .unwrap()
                .public_key(),
            key.public_key()
        );
        std::fs::write(&path, b"damaged").unwrap();
        assert!(DeviceKey::load_or_create(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
