use aws_lc_rs::{
    aead::{Aad, Nonce, RandomizedNonceKey, AES_256_GCM, NONCE_LEN},
    digest::{digest, Digest, SHA256},
    rand,
};
use base64::prelude::*;
use std::{
    fs::OpenOptions,
    io::{ErrorKind, Read, Write},
    path::Path,
};

const KEY_FILENAME: &str = "collector-variables.key";
const VERSION: &str = "v1:";
const KEY_LEN: usize = 32;
const KEY_ERROR: &str = "无法读取采集器变量密钥：密钥缺失、损坏或不可访问。";
const DECRYPT_ERROR: &str = "无法解密采集器变量：密文损坏、版本不受支持或密钥不匹配。";

pub(super) struct KeyStore {
    key: RandomizedNonceKey,
    fingerprint: Digest,
}

impl KeyStore {
    fn read_key(config_dir: &Path) -> std::io::Result<[u8; KEY_LEN]> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(config_dir.join(KEY_FILENAME))?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != KEY_LEN as u64 {
            return Err(ErrorKind::InvalidData.into());
        }
        let mut key = Vec::with_capacity(KEY_LEN + 1);
        file.take((KEY_LEN + 1) as u64).read_to_end(&mut key)?;
        key.try_into().map_err(|_| ErrorKind::InvalidData.into())
    }

    fn from_bytes(key: &[u8; KEY_LEN]) -> Result<Self, String> {
        Ok(Self {
            key: RandomizedNonceKey::new(&AES_256_GCM, key).map_err(|_| KEY_ERROR)?,
            fingerprint: digest(&SHA256, key),
        })
    }

    pub(super) fn load(config_dir: &Path) -> Result<Self, String> {
        Self::from_bytes(&Self::read_key(config_dir).map_err(|_| KEY_ERROR)?)
    }

    pub(super) fn load_or_create(config_dir: &Path) -> Result<Self, String> {
        match Self::read_key(config_dir) {
            Ok(key) => return Self::from_bytes(&key),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(KEY_ERROR.into()),
        }
        let mut key = [0; KEY_LEN];
        rand::fill(&mut key).map_err(|_| "无法生成采集器变量密钥。")?;
        let write_error = "无法保存采集器变量密钥。";
        // The private temporary file is complete before we publish it, and an
        // existing key wins if another process creates one first.
        let mut temp = tempfile::NamedTempFile::new_in(config_dir).map_err(|_| write_error)?;
        temp.write_all(&key).map_err(|_| write_error)?;
        temp.as_file().sync_all().map_err(|_| write_error)?;
        match temp.persist_noclobber(config_dir.join(KEY_FILENAME)) {
            Ok(_) => Self::from_bytes(&key),
            Err(error) if error.error.kind() == ErrorKind::AlreadyExists => Self::load(config_dir),
            Err(_) => Err(write_error.into()),
        }
    }

    pub(super) fn verify_file(&self, config_dir: &Path) -> Result<(), String> {
        let key = Self::read_key(config_dir).map_err(|_| KEY_ERROR)?;
        if digest(&SHA256, &key).as_ref() != self.fingerprint.as_ref() {
            return Err("采集器变量密钥已变化，无法保存配置。".into());
        }
        Ok(())
    }

    pub(super) fn encrypt(&self, plain: &str) -> Result<String, String> {
        let mut ciphertext = plain.as_bytes().to_vec();
        let nonce = self
            .key
            .seal_in_place_append_tag(Aad::empty(), &mut ciphertext)
            .map_err(|_| "无法加密采集器变量。")?;
        let mut envelope = nonce.as_ref().to_vec();
        envelope.extend(ciphertext);
        Ok(format!("{VERSION}{}", BASE64_STANDARD.encode(envelope)))
    }

    pub(super) fn decrypt(&self, cipher: &str) -> Result<String, String> {
        let mut envelope = BASE64_STANDARD
            .decode(cipher.strip_prefix(VERSION).ok_or(DECRYPT_ERROR)?)
            .map_err(|_| DECRYPT_ERROR)?;
        let (nonce, ciphertext) = envelope
            .split_at_mut_checked(NONCE_LEN)
            .ok_or(DECRYPT_ERROR)?;
        let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| DECRYPT_ERROR)?;
        let plain = self
            .key
            .open_in_place(nonce, Aad::empty(), ciphertext)
            .map_err(|_| DECRYPT_ERROR)?;
        String::from_utf8(plain.to_vec()).map_err(|_| DECRYPT_ERROR.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn uses_random_nonces_and_reads_the_same_key_after_restart() {
        let config = tempfile::tempdir().unwrap();
        let vault = KeyStore::load_or_create(config.path()).unwrap();
        let plain = "本机 variable with spaces";
        let first = vault.encrypt(plain).unwrap();
        let second = vault.encrypt(plain).unwrap();
        assert_ne!(first, second);
        assert!(first.starts_with("v1:"));
        assert!(!first.contains(plain));
        let key = config.path().join(KEY_FILENAME);
        assert_eq!(fs::metadata(&key).unwrap().len(), 32);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(key).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let restarted = KeyStore::load_or_create(config.path()).unwrap();
        assert_eq!(restarted.decrypt(&first).unwrap(), plain);
        assert_eq!(restarted.decrypt(&second).unwrap(), plain);
    }

    #[test]
    fn rejects_tampered_ciphertext_and_wrong_key() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let vault = KeyStore::load_or_create(first.path()).unwrap();
        let wrong = KeyStore::load_or_create(second.path()).unwrap();
        let cipher = vault.encrypt("never in an error").unwrap();
        assert!(wrong.decrypt(&cipher).is_err());
        let mut tampered = cipher.into_bytes();
        tampered[3] = if tampered[3] == b'A' { b'B' } else { b'A' };
        assert!(vault
            .decrypt(&String::from_utf8(tampered).unwrap())
            .is_err());
        assert!(vault.decrypt("v2:unknown-version").is_err());
        assert!(vault.decrypt("v1:AAAA").is_err());
    }

    #[test]
    fn never_replaces_a_damaged_key() {
        let config = tempfile::tempdir().unwrap();
        let key = config.path().join(KEY_FILENAME);
        fs::write(&key, b"damaged").unwrap();
        assert!(KeyStore::load_or_create(config.path()).is_err());
        assert_eq!(fs::read(&key).unwrap(), b"damaged");
    }
}
