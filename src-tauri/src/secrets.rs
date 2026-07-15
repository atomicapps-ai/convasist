//! Portable encrypted secrets (owner request).
//!
//! API keys still live in the OS keyring at runtime (design §4.6). This
//! module adds an *opt-in* way to carry them (and arbitrary values) to
//! another machine via git: a single passphrase-encrypted file that is safe
//! to commit. The passphrase is read from the `CONVASIST_SECRETS_PASSPHRASE`
//! environment variable — so no plaintext secret ever lands in the repo, and
//! nothing is typed on each launch. On startup, if the passphrase is set and
//! the file is present, keys seed the keyring only where one is missing (so
//! in-app edits win; the file bootstraps a fresh machine).
//!
//! Crypto is cocoon (PBKDF2 key derivation + ChaCha20-Poly1305 AEAD, random
//! salt+nonce embedded per file). Wrong passphrase or a tampered file fails
//! closed on decrypt.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use convasist_core::llm::{provider_registry, ProviderId};

use crate::llm::{load_api_key, store_api_key};

pub const PASSPHRASE_ENV: &str = "CONVASIST_SECRETS_PASSPHRASE";
pub const SECRETS_FILE_ENV: &str = "CONVASIST_SECRETS_FILE";
pub const DEFAULT_SECRETS_FILE: &str = "convasist.secrets.enc";

/// The decrypted payload. `values` is a free-form escape hatch for non-key
/// settings the user wants to carry; `keys` maps a provider id string
/// (snake_case, matching `ProviderId`) to its API key.
#[derive(Default, Serialize, Deserialize)]
struct SecretBundle {
    #[serde(default)]
    keys: BTreeMap<String, String>,
    #[serde(default)]
    values: BTreeMap<String, String>,
}

fn pid_to_str(provider: ProviderId) -> String {
    serde_json::to_string(&provider)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

fn str_to_pid(s: &str) -> Option<ProviderId> {
    serde_json::from_str(&format!("\"{s}\"")).ok()
}

/// The configured secrets-file path: `CONVASIST_SECRETS_FILE` if set, else
/// `convasist.secrets.enc` in the current directory (the repo root in dev).
pub fn default_path() -> PathBuf {
    match std::env::var(SECRETS_FILE_ENV) {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => PathBuf::from(DEFAULT_SECRETS_FILE),
    }
}

pub fn passphrase_set() -> bool {
    matches!(std::env::var(PASSPHRASE_ENV), Ok(p) if !p.trim().is_empty())
}

fn passphrase() -> Result<String, String> {
    match std::env::var(PASSPHRASE_ENV) {
        Ok(p) if !p.trim().is_empty() => Ok(p),
        _ => Err(format!(
            "set the {PASSPHRASE_ENV} environment variable to lock/unlock the secrets file"
        )),
    }
}

fn encrypt(pass: &str, plaintext: &[u8]) -> Result<Vec<u8>, String> {
    let mut cocoon = cocoon::Cocoon::new(pass.as_bytes());
    cocoon
        .wrap(plaintext)
        .map_err(|e| format!("encrypt failed: {e:?}"))
}

fn decrypt(pass: &str, blob: &[u8]) -> Result<Vec<u8>, String> {
    let cocoon = cocoon::Cocoon::new(pass.as_bytes());
    cocoon
        .unwrap(blob)
        .map_err(|e| format!("wrong passphrase or corrupt secrets file: {e:?}"))
}

/// Collect every stored provider key from the keyring into a bundle.
fn collect_bundle() -> SecretBundle {
    let mut keys = BTreeMap::new();
    for provider in provider_registry() {
        if let Ok(Some(key)) = load_api_key(provider.id) {
            if !key.is_empty() {
                keys.insert(pid_to_str(provider.id), key);
            }
        }
    }
    SecretBundle {
        keys,
        values: BTreeMap::new(),
    }
}

/// Encrypt the current keyring secrets to `path` (commit this file to git).
/// Returns the number of keys written.
pub fn export_to(path: &Path) -> Result<usize, String> {
    let pass = passphrase()?;
    let bundle = collect_bundle();
    let count = bundle.keys.len();
    if count == 0 {
        return Err("no API keys stored yet — add at least one in Settings first".into());
    }
    let json = serde_json::to_vec(&bundle).map_err(|e| e.to_string())?;
    let blob = encrypt(&pass, &json)?;
    std::fs::write(path, blob).map_err(|e| e.to_string())?;
    Ok(count)
}

/// Decrypt `path` and load keys into the keyring. When `overwrite` is false,
/// only fills providers that have no key yet (in-app edits win). Returns the
/// number of keys applied.
pub fn import_from(path: &Path, overwrite: bool) -> Result<usize, String> {
    let pass = passphrase()?;
    let blob = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let json = decrypt(&pass, &blob)?;
    let bundle: SecretBundle = serde_json::from_slice(&json).map_err(|e| e.to_string())?;

    let mut applied = 0usize;
    for (pid_str, key) in &bundle.keys {
        let Some(provider) = str_to_pid(pid_str) else {
            continue; // unknown provider id — skip, never fail the whole import
        };
        let already = load_api_key(provider).ok().flatten().unwrap_or_default();
        if overwrite || already.is_empty() {
            store_api_key(provider, key).map_err(|e| e.to_string())?;
            applied += 1;
        }
    }
    Ok(applied)
}

/// Startup seed: if the passphrase is set and the default file exists, load
/// any missing keys. Silent no-op otherwise — never blocks startup.
pub fn seed_on_startup() {
    if !passphrase_set() {
        return;
    }
    let path = default_path();
    if path.exists() {
        let _ = import_from(&path, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_roundtrip_and_rejects_wrong_passphrase() {
        let bundle = SecretBundle {
            keys: BTreeMap::from([("anthropic".into(), "sk-test-123".into())]),
            values: BTreeMap::from([("region".into(), "us".into())]),
        };
        let json = serde_json::to_vec(&bundle).unwrap();

        let blob = encrypt("correct horse battery staple", &json).unwrap();
        // Ciphertext must not leak the key material.
        assert!(!blob.windows(11).any(|w| w == b"sk-test-123"));

        let back = decrypt("correct horse battery staple", &blob).unwrap();
        let restored: SecretBundle = serde_json::from_slice(&back).unwrap();
        assert_eq!(restored.keys.get("anthropic").unwrap(), "sk-test-123");
        assert_eq!(restored.values.get("region").unwrap(), "us");

        // A wrong passphrase fails closed.
        assert!(decrypt("wrong passphrase", &blob).is_err());
    }

    #[test]
    fn provider_id_string_roundtrip() {
        for p in provider_registry() {
            let s = pid_to_str(p.id);
            assert_eq!(str_to_pid(&s), Some(p.id), "{s}");
        }
        assert_eq!(str_to_pid("not_a_provider"), None);
    }
}
