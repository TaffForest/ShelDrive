use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Symmetric encryption (AES-256-GCM) — for file content
// ---------------------------------------------------------------------------

/// Generate a random 256-bit symmetric key for a folder.
pub fn generate_folder_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    key
}

/// Encrypt data using a raw 256-bit AES key.
/// Returns: 12-byte nonce + ciphertext.
pub fn encrypt_with_key(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Key error: {}", e))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| format!("Encrypt error: {}", e))?;

    let mut output = Vec::with_capacity(12 + ciphertext.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypt data encrypted with `encrypt_with_key`.
pub fn decrypt_with_key(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, String> {
    if data.len() < 12 {
        return Err("Data too short to contain nonce".to_string());
    }

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("Key error: {}", e))?;
    let nonce = Nonce::from_slice(&data[..12]);
    let ciphertext = &data[12..];

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| format!("Decrypt error: {}", e))
}

// ---------------------------------------------------------------------------
// Key wrapping — encrypt a folder key for a user (using their passphrase)
// In production this would use Ed25519→X25519 key exchange.
// For now we derive a wrapping key from the user's private key.
// ---------------------------------------------------------------------------

/// Derive a 256-bit wrapping key from a private key string.
fn derive_wrapping_key(private_key: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"sheldrive-keywrap-v1:");
    hasher.update(private_key.as_bytes());
    hasher.finalize().into()
}

/// Wrap (encrypt) a folder key for storage. Uses the owner's private key as passphrase.
/// Returns base64-encoded encrypted key.
pub fn wrap_folder_key(folder_key: &[u8; 32], private_key: &str) -> Result<String, String> {
    let wrapping_key = derive_wrapping_key(private_key);
    let encrypted = encrypt_with_key(folder_key, &wrapping_key)?;
    Ok(BASE64.encode(&encrypted))
}

/// Unwrap (decrypt) a folder key. Uses the owner's private key as passphrase.
/// Input: base64-encoded encrypted key.
pub fn unwrap_folder_key(encrypted_key_b64: &str, private_key: &str) -> Result<[u8; 32], String> {
    let encrypted = BASE64
        .decode(encrypted_key_b64)
        .map_err(|e| format!("Base64 decode error: {}", e))?;
    let wrapping_key = derive_wrapping_key(private_key);
    let decrypted = decrypt_with_key(&encrypted, &wrapping_key)?;
    if decrypted.len() != 32 {
        return Err(format!("Invalid key length: {} (expected 32)", decrypted.len()));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&decrypted);
    Ok(key)
}

/// Wrap a folder key for a specific recipient using their address.
/// For sharing: derives a wrapping key from a shared secret.
/// In production, this would use X25519 ECDH with the recipient's public key.
/// For now, uses a deterministic derivation from both addresses.
pub fn wrap_folder_key_for_recipient(
    folder_key: &[u8; 32],
    owner_private_key: &str,
    recipient_address: &str,
) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(b"sheldrive-share-v1:");
    hasher.update(owner_private_key.as_bytes());
    hasher.update(b":");
    hasher.update(recipient_address.as_bytes());
    let shared_secret: [u8; 32] = hasher.finalize().into();
    let encrypted = encrypt_with_key(folder_key, &shared_secret)?;
    Ok(BASE64.encode(&encrypted))
}

/// Unwrap a folder key that was shared with you.
pub fn unwrap_shared_folder_key(
    encrypted_key_b64: &str,
    sharer_address: &str,
    my_private_key: &str,
) -> Result<[u8; 32], String> {
    let encrypted = BASE64
        .decode(encrypted_key_b64)
        .map_err(|e| format!("Base64 decode error: {}", e))?;
    let mut hasher = Sha256::new();
    hasher.update(b"sheldrive-share-v1:");
    hasher.update(sharer_address.as_bytes());
    hasher.update(b":");
    // Derive the recipient's address from their private key
    let mut addr_hasher = Sha256::new();
    addr_hasher.update(my_private_key.as_bytes());
    let my_address = hex::encode(&addr_hasher.finalize()[..20]);
    hasher.update(my_address.as_bytes());
    let shared_secret: [u8; 32] = hasher.finalize().into();
    decrypt_with_key(&encrypted, &shared_secret).and_then(|d| {
        if d.len() != 32 {
            Err(format!("Invalid key length: {}", d.len()))
        } else {
            let mut key = [0u8; 32];
            key.copy_from_slice(&d);
            Ok(key)
        }
    })
}

// ---------------------------------------------------------------------------
// Legacy API — keep for backward compatibility
// ---------------------------------------------------------------------------

/// Encrypt data using a passphrase (derives key via SHA-256).
pub fn encrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    let key = derive_wrapping_key(passphrase);
    encrypt_with_key(data, &key)
}

/// Decrypt data using a passphrase.
pub fn decrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    let key = derive_wrapping_key(passphrase);
    decrypt_with_key(data, &key)
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
        let decrypted = decrypt(&encrypted, passphrase).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_wrong_passphrase_fails() {
        let data = b"secret data";
        let encrypted = encrypt(data, "correct-key").unwrap();
        assert!(decrypt(&encrypted, "wrong-key").is_err());
    }

    #[test]
    fn test_different_encryptions_produce_different_output() {
        let data = b"same data";
        let e1 = encrypt(data, "key").unwrap();
        let e2 = encrypt(data, "key").unwrap();
        assert_ne!(e1, e2);
    }

    #[test]
    fn test_folder_key_generation() {
        let k1 = generate_folder_key();
        let k2 = generate_folder_key();
        assert_ne!(k1, k2); // Random keys should differ
        assert_eq!(k1.len(), 32);
    }

    #[test]
    fn test_folder_key_wrap_unwrap() {
        let folder_key = generate_folder_key();
        let private_key = "0xabc123";
        let wrapped = wrap_folder_key(&folder_key, private_key).unwrap();
        let unwrapped = unwrap_folder_key(&wrapped, private_key).unwrap();
        assert_eq!(folder_key, unwrapped);
    }

    #[test]
    fn test_folder_key_wrong_key_fails() {
        let folder_key = generate_folder_key();
        let wrapped = wrap_folder_key(&folder_key, "owner-key").unwrap();
        assert!(unwrap_folder_key(&wrapped, "wrong-key").is_err());
    }

    #[test]
    fn test_encrypt_decrypt_with_folder_key() {
        let folder_key = generate_folder_key();
        let data = b"file content here";
        let encrypted = encrypt_with_key(data, &folder_key).unwrap();
        let decrypted = decrypt_with_key(&encrypted, &folder_key).unwrap();
        assert_eq!(decrypted, data);
    }
}
