use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Derive a 256-bit encryption key from a passphrase (the user's private key).
fn derive_key(passphrase: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"sheldrive-encryption-v1:");
    hasher.update(passphrase.as_bytes());
    hasher.finalize().into()
}

/// Encrypt data using AES-256-GCM.
/// Returns: 12-byte nonce + ciphertext (nonce prepended).
pub fn encrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Key error: {}", e))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| format!("Encrypt error: {}", e))?;

    // Prepend nonce to ciphertext
    let mut output = Vec::with_capacity(12 + ciphertext.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypt data encrypted with `encrypt`.
/// Input: 12-byte nonce + ciphertext.
pub fn decrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    if data.len() < 12 {
        return Err("Data too short to contain nonce".to_string());
    }

    let key = derive_key(passphrase);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Key error: {}", e))?;

    let nonce = Nonce::from_slice(&data[..12]);
    let ciphertext = &data[12..];

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decrypt error: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let data = b"Hello, Shelby!";
        let passphrase = "my-secret-key";

        let encrypted = encrypt(data, passphrase).unwrap();
        assert_ne!(encrypted, data);
        assert!(encrypted.len() > data.len()); // nonce + auth tag overhead

        let decrypted = decrypt(&encrypted, passphrase).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_wrong_passphrase_fails() {
        let data = b"secret data";
        let encrypted = encrypt(data, "correct-key").unwrap();
        let result = decrypt(&encrypted, "wrong-key");
        assert!(result.is_err());
    }

    #[test]
    fn test_different_encryptions_produce_different_output() {
        let data = b"same data";
        let e1 = encrypt(data, "key").unwrap();
        let e2 = encrypt(data, "key").unwrap();
        assert_ne!(e1, e2); // Different nonces
    }
}
