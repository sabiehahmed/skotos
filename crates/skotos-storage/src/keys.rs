use rand::RngCore;
use sha2::{Digest, Sha256};

/// Generated secret credential returned once to the caller alongside its stored hash and prefix.
#[derive(Debug, Clone)]
pub struct GeneratedSecretKey {
    pub plaintext: String,
    pub prefix: String,
    pub hash: String,
}

/// Generates a cryptographically random prefixed secret (`sk_sdk_`, `sk_mob_`, `sk_api_`).
pub fn generate_prefixed_secret(kind_prefix: &str) -> GeneratedSecretKey {
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    let random_hex = hex::encode(bytes);
    let plaintext = format!("{kind_prefix}{random_hex}");
    let prefix = format!("{kind_prefix}{}", &random_hex[..8]);
    let hash = sha256_hex(&plaintext);
    GeneratedSecretKey {
        plaintext,
        prefix,
        hash,
    }
}

/// Generates a public client-side environment identifier (`sk_cid_...`).
pub fn generate_client_side_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("sk_cid_{}", hex::encode(bytes))
}

/// Generates a random salt string for flag/segment percentage rollout hashing.
pub fn generate_salt() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Computes lowercase SHA-256 hex digest of a secret token or key.
pub fn sha256_hex(secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hex::encode(hasher.finalize())
}
