use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use std::path::Path;
use zamsync_core::{ZamError, ZamResult};

pub const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

pub struct EncryptionKey {
    cipher: ChaCha20Poly1305,
    raw: [u8; KEY_LEN],
}

impl Clone for EncryptionKey {
    fn clone(&self) -> Self {
        Self::from_bytes(self.raw)
    }
}

impl EncryptionKey {
    /// Generate a new random 32-byte key.
    pub fn generate() -> ZamResult<Self> {
        let key = ChaCha20Poly1305::generate_key(&mut OsRng);
        let raw: [u8; KEY_LEN] = key.into();
        Ok(Self {
            cipher: ChaCha20Poly1305::new(&key),
            raw,
        })
    }

    pub fn from_bytes(raw: [u8; KEY_LEN]) -> Self {
        let key = chacha20poly1305::Key::from(raw);
        Self {
            cipher: ChaCha20Poly1305::new(&key),
            raw,
        }
    }

    /// Load key from a 32-byte binary file.
    pub fn from_file(path: impl AsRef<Path>) -> ZamResult<Self> {
        let bytes = std::fs::read(path.as_ref()).map_err(|e| {
            ZamError::Io(std::io::Error::new(
                e.kind(),
                format!("key file {}: {}", path.as_ref().display(), e),
            ))
        })?;
        if bytes.len() != KEY_LEN {
            return Err(ZamError::Config(format!(
                "encryption key must be {KEY_LEN} bytes, got {}",
                bytes.len()
            )));
        }
        let mut raw = [0u8; KEY_LEN];
        raw.copy_from_slice(&bytes);
        Ok(Self::from_bytes(raw))
    }

    /// Return the raw 32-byte key material.
    pub fn raw_bytes(&self) -> [u8; KEY_LEN] {
        self.raw
    }

    /// Write the raw 32-byte key to a file (permissions should be 0600).
    pub fn to_file(&self, path: impl AsRef<Path>) -> ZamResult<()> {
        std::fs::write(path, self.raw).map_err(Into::into)
    }

    /// Encrypt `plaintext`. Returns `[nonce: 12][ciphertext][tag: 16]`.
    pub fn encrypt(&self, plaintext: &[u8]) -> ZamResult<Vec<u8>> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| ZamError::Config("WAL encryption failed".into()))?;
        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypt `data` (must be `[nonce: 12][ciphertext][tag: 16]`).
    pub fn decrypt(&self, data: &[u8]) -> ZamResult<Vec<u8>> {
        if data.len() < NONCE_LEN + 16 {
            return Err(ZamError::Corruption(
                "encrypted WAL payload too short".into(),
            ));
        }
        let nonce = Nonce::from_slice(&data[..NONCE_LEN]);
        self.cipher.decrypt(nonce, &data[NONCE_LEN..]).map_err(|_| {
            ZamError::Corruption("WAL decryption failed -- wrong key or tampered data".into())
        })
    }
}
