//! Phase 10: `.sair` backup/restore container, crypto, and staging (FR-014,
//! docs/10, ADR-007 as refreshed by the Phase 10 review — argon2 0.6 +
//! aes-gcm 0.11).

use std::path::Path;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use ring::rand::{SecureRandom, SystemRandom};
use sair_vault::SecretBytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const FORMAT_ID: &str = "sair-backup";
pub const FORMAT_VERSION: i64 = 1;
/// docs/10 import size cap.
pub const MAX_BACKUP_BYTES: usize = 64 * 1024 * 1024;
/// Argon2id parameters (ADR-007 baseline; recorded in the envelope).
pub const KDF_M_KIB: u32 = 65536;
pub const KDF_T: u32 = 3;
pub const KDF_P: u32 = 1;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error("envelope is not a valid backup: {0}")]
    Invalid(String),
    #[error("backup exceeds the size cap")]
    TooLarge,
    #[error("decryption failed (wrong passphrase or tampered data)")]
    AuthFailed,
}

/// Fixed-order AAD struct (F-10): authenticates the envelope metadata so a
/// tampered mode/version/kdf invalidates the ciphertext.
#[derive(Serialize)]
struct Aad<'a> {
    format: &'a str,
    format_version: i64,
    mode: &'a str,
    created_at: &'a str,
    m_kib: u32,
    t: u32,
    p: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub format: String,
    pub format_version: i64,
    pub app_version: String,
    pub created_at: String,
    pub mode: String, // "config" | "encrypted"
    pub schema_version: i64,
    pub included_sections: Vec<String>,
    #[serde(default)]
    pub kdf: Option<KdfParams>,
    #[serde(default)]
    pub integrity: Option<String>, // sha256 hex over the canonical data string
    pub data: Value,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt_b64: String,
}

fn b64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn unb64(data: &str) -> Result<Vec<u8>, BackupError> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| BackupError::Invalid("base64 decode failed".to_string()))
}

/// Derives the AES-256 key from the passphrase (Argon2id v19, ADR-007).
/// Runs blocking — callers wrap in spawn_blocking.
fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], BackupError> {
    use argon2::Argon2;
    let params = argon2::Params::new(KDF_M_KIB, KDF_T, KDF_P, Some(32))
        .map_err(|e| BackupError::Invalid(format!("kdf params: {e}")))?;
    let argon = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; 32];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| BackupError::Invalid(format!("kdf: {e}")))?;
    Ok(key)
}

fn sha256_hex(data: &[u8]) -> String {
    use ring::digest::{SHA256, digest};
    digest(&SHA256, data)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Builds the envelope for CONFIG-ONLY mode: `payload` must already exclude
/// all secret material (callers use the allowlisted projection).
pub fn build_config_envelope(
    app_version: &str,
    payload: Value,
    included_sections: Vec<String>,
) -> Result<Value, BackupError> {
    let created_at = chrono::Utc::now().to_rfc3339();
    let data_str =
        serde_json::to_string(&payload).map_err(|e| BackupError::Invalid(e.to_string()))?;
    Ok(serde_json::json!({
        "format": FORMAT_ID,
        "format_version": FORMAT_VERSION,
        "app_version": app_version,
        "created_at": created_at,
        "mode": "config",
        "schema_version": FORMAT_VERSION,
        "included_sections": included_sections,
        "integrity": sha256_hex(data_str.as_bytes()),
        "data": payload,
    }))
}

/// Builds the envelope for ENCRYPTED mode (F-2: `payload` INCLUDES credential
/// secrets — they exist only inside the authenticated ciphertext).
pub fn build_encrypted_envelope(
    app_version: &str,
    payload: Value,
    included_sections: Vec<String>,
    passphrase: &str,
) -> Result<Value, BackupError> {
    let created_at = chrono::Utc::now().to_rfc3339();
    let rng = SystemRandom::new();
    let mut salt = [0u8; 16];
    rng.fill(&mut salt)
        .map_err(|_| BackupError::Invalid("CSPRNG failure".to_string()))?;
    let kdf = KdfParams {
        m_kib: KDF_M_KIB,
        t: KDF_T,
        p: KDF_P,
        salt_b64: b64(&salt),
    };
    let key_bytes = derive_key(passphrase, &salt)?;
    let key = Key::<Aes256Gcm>::try_from(&key_bytes[..]).expect("32-byte key");
    let cipher = Aes256Gcm::new(&key);
    let mut nonce_bytes = [0u8; 12];
    rng.fill(&mut nonce_bytes)
        .map_err(|_| BackupError::Invalid("CSPRNG failure".to_string()))?;
    let nonce = Nonce::try_from(&nonce_bytes[..]).expect("12-byte nonce");
    let payload_str =
        serde_json::to_string(&payload).map_err(|e| BackupError::Invalid(e.to_string()))?;
    let aad = Aad {
        format: FORMAT_ID,
        format_version: FORMAT_VERSION,
        mode: "encrypted",
        created_at: &created_at,
        m_kib: KDF_M_KIB,
        t: KDF_T,
        p: KDF_P,
    };
    let aad_json = serde_json::to_vec(&aad).map_err(|e| BackupError::Invalid(e.to_string()))?;
    let sealed = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: payload_str.as_bytes(),
                aad: &aad_json,
            },
        )
        .map_err(|_| BackupError::AuthFailed)?;
    // nonce || ciphertext || tag (GCM appends the tag).
    let combined = [nonce_bytes.as_slice(), sealed.as_slice()].concat();
    Ok(serde_json::json!({
        "format": FORMAT_ID,
        "format_version": FORMAT_VERSION,
        "app_version": app_version,
        "created_at": created_at,
        "mode": "encrypted",
        "schema_version": FORMAT_VERSION,
        "included_sections": included_sections,
        "kdf": kdf,
        "integrity": sha256_hex(payload_str.as_bytes()),
        "data": b64(&combined),
    }))
}

/// Decrypts an encrypted envelope (passphrase + AAD verification; wrong
/// passphrase/tampering → [`BackupError::AuthFailed`]). Blocking — callers
/// wrap in spawn_blocking.
pub fn decrypt_envelope(envelope: &Envelope, passphrase: &str) -> Result<Value, BackupError> {
    let kdf = envelope
        .kdf
        .as_ref()
        .ok_or_else(|| BackupError::Invalid("missing kdf params".to_string()))?;
    let salt = unb64(&kdf.salt_b64)?;
    let key_bytes = derive_key(passphrase, &salt)?;
    let combined = unb64(
        envelope
            .data
            .as_str()
            .ok_or_else(|| BackupError::Invalid("data is not a string".to_string()))?,
    )?;
    if combined.len() < 12 {
        return Err(BackupError::AuthFailed);
    }
    let (nonce_bytes, sealed) = combined.split_at(12);
    let key = Key::<Aes256Gcm>::try_from(&key_bytes[..]).expect("32-byte key");
    let cipher = Aes256Gcm::new(&key);
    let aad = Aad {
        format: FORMAT_ID,
        format_version: envelope.format_version,
        mode: &envelope.mode,
        created_at: &envelope.created_at,
        m_kib: kdf.m_kib,
        t: kdf.t,
        p: kdf.p,
    };
    let aad_json = serde_json::to_vec(&aad).map_err(|e| BackupError::Invalid(e.to_string()))?;
    let plain = cipher
        .decrypt(
            &Nonce::try_from(nonce_bytes).expect("12-byte nonce"),
            Payload {
                msg: sealed,
                aad: &aad_json,
            },
        )
        .map_err(|_| BackupError::AuthFailed)?;
    serde_json::from_str(&String::from_utf8_lossy(&plain))
        .map_err(|e| BackupError::Invalid(format!("decrypted payload is not valid JSON: {e}")))
}

/// Parses and validates an envelope from file bytes (size cap + structure +
/// integrity for config mode).
pub fn parse_envelope(bytes: &[u8]) -> Result<Envelope, BackupError> {
    if bytes.len() > MAX_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    let envelope: Envelope = serde_json::from_slice(bytes)
        .map_err(|e| BackupError::Invalid(format!("structure: {e}")))?;
    if envelope.format != FORMAT_ID {
        return Err(BackupError::Invalid("unknown format id".to_string()));
    }
    if envelope.format_version != FORMAT_VERSION {
        return Err(BackupError::Invalid(
            "unsupported format version".to_string(),
        ));
    }
    if !matches!(envelope.mode.as_str(), "config" | "encrypted") {
        return Err(BackupError::Invalid("unknown mode".to_string()));
    }
    // Integrity: config mode has a cleartext checksum; encrypted mode's
    // checksum is verified after decryption.
    if envelope.mode == "config"
        && let Some(expected) = &envelope.integrity
    {
        let data_str = serde_json::to_string(&envelope.data)
            .map_err(|e| BackupError::Invalid(e.to_string()))?;
        if sha256_hex(data_str.as_bytes()) != *expected {
            return Err(BackupError::Invalid("integrity check failed".to_string()));
        }
    }
    Ok(envelope)
}

/// Reads a backup file with the size cap enforced.
pub fn read_backup_file(path: &Path) -> Result<Vec<u8>, BackupError> {
    let meta = std::fs::metadata(path).map_err(|e| BackupError::Invalid(e.to_string()))?;
    if meta.len() as usize > MAX_BACKUP_BYTES {
        return Err(BackupError::TooLarge);
    }
    std::fs::read(path).map_err(|e| BackupError::Invalid(e.to_string()))
}

/// Zeroizing wrapper for decrypted payloads held between preview and commit
/// (F-3): content is wiped on drop; callers re-decrypt if expired.
#[must_use]
pub fn seal_decrypted(payload: Value) -> SecretBytes {
    SecretBytes::new(serde_json::to_vec(&payload).unwrap_or_default())
}

/// Unseals a held payload (fails if the TTL dropped zeroized content —
/// callers re-prompt).
#[must_use]
pub fn unseal_decrypted(sealed: &SecretBytes) -> Option<Value> {
    serde_json::from_slice(sealed.expose()).ok()
}

// ---- Export staging over the DB (Phase 10) ----

use sqlx::Row;

use sair_persistence::Db;
use sair_vault::SecretStore;

/// Builds the config-only payload (F-7: credential metadata included with
/// status='incomplete' and NO secret_ref; settings key allowlist).
pub async fn build_config_payload(db: &Db) -> Result<(Value, Vec<String>), String> {
    let mut sections = Vec::new();
    let providers: Vec<Value> = sqlx::query(
        "SELECT id, name, provider_type, base_url, enabled, allow_loopback, notes FROM providers",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "name": r.get::<String, _>("name"),
            "provider_type": r.get::<String, _>("provider_type"),
            "base_url": r.get::<String, _>("base_url"),
            "enabled": r.get::<i64, _>("enabled"),
            "allow_loopback": r.get::<i64, _>("allow_loopback"),
            "notes": r.get::<Option<String>, _>("notes"),
        })
    })
    .collect();
    sections.push("providers".to_string());

    let accounts: Vec<Value> =
        sqlx::query("SELECT id, provider_id, label, group_key FROM provider_accounts")
            .fetch_all(db.pool())
            .await
            .map_err(|e| e.to_string())?
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.get::<String, _>("id"),
                    "provider_id": r.get::<String, _>("provider_id"),
                    "label": r.get::<String, _>("label"),
                    "group_key": r.get::<Option<String>, _>("group_key"),
                })
            })
            .collect();
    sections.push("provider_accounts".to_string());

    // F-7: credential metadata WITHOUT secret_ref; status forced incomplete.
    let credentials: Vec<Value> =
        sqlx::query("SELECT id, provider_id, account_id, label, priority, weight FROM credentials")
            .fetch_all(db.pool())
            .await
            .map_err(|e| e.to_string())?
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.get::<String, _>("id"),
                    "provider_id": r.get::<String, _>("provider_id"),
                    "account_id": r.get::<Option<String>, _>("account_id"),
                    "label": r.get::<String, _>("label"),
                    "status": "incomplete",
                    "priority": r.get::<i64, _>("priority"),
                    "weight": r.get::<i64, _>("weight"),
                })
            })
            .collect();
    sections.push("credentials".to_string());

    let models: Vec<Value> = sqlx::query(
        "SELECT id, provider_id, model_id, display_name, protocol_family, capabilities FROM models",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "provider_id": r.get::<String, _>("provider_id"),
            "model_id": r.get::<String, _>("model_id"),
            "display_name": r.get::<Option<String>, _>("display_name"),
            "protocol_family": r.get::<String, _>("protocol_family"),
            "capabilities": serde_json::from_str::<Value>(&r.get::<String, _>("capabilities")).unwrap_or(Value::Null),
        })
    })
    .collect();
    sections.push("models".to_string());

    let deployments: Vec<Value> = sqlx::query(
        "SELECT id, provider_id, model_id, endpoint_override, enabled FROM deployments",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "provider_id": r.get::<String, _>("provider_id"),
            "model_id": r.get::<String, _>("model_id"),
            "endpoint_override": r.get::<Option<String>, _>("endpoint_override"),
            "enabled": r.get::<i64, _>("enabled"),
        })
    })
    .collect();
    sections.push("deployments".to_string());

    let policies: Vec<Value> = sqlx::query(
        "SELECT id, name, strategy, max_attempts, retry_budget_ms, timeout_ms, cooldown_seconds FROM routing_policies",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "name": r.get::<String, _>("name"),
            "strategy": r.get::<String, _>("strategy"),
            "max_attempts": r.get::<i64, _>("max_attempts"),
            "retry_budget_ms": r.get::<i64, _>("retry_budget_ms"),
            "timeout_ms": r.get::<i64, _>("timeout_ms"),
            "cooldown_seconds": r.get::<i64, _>("cooldown_seconds"),
        })
    })
    .collect();
    sections.push("routing_policies".to_string());

    let aliases: Vec<Value> = sqlx::query(
        "SELECT ma.id, ma.alias, ma.description, ma.policy_id,
                ad.deployment_id, ad.priority
         FROM model_aliases ma
         LEFT JOIN alias_deployments ad ON ad.alias_id = ma.id",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.get::<String, _>("id"),
            "alias": r.get::<String, _>("alias"),
            "description": r.get::<Option<String>, _>("description"),
            "policy_id": r.get::<Option<String>, _>("policy_id"),
            "deployment_id": r.get::<Option<String>, _>("deployment_id"),
            "priority": r.get::<Option<i64>, _>("priority"),
        })
    })
    .collect();
    sections.push("aliases".to_string());

    let profiles: Vec<Value> = sqlx::query(
        "SELECT agent_id, agent_version, protocol_family, endpoint, auth_method, model_alias, fast_alias, env_json, config_rules_json, notes FROM agent_profiles",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "agent_id": r.get::<String, _>("agent_id"),
            "agent_version": r.get::<Option<String>, _>("agent_version"),
            "protocol_family": r.get::<String, _>("protocol_family"),
            "endpoint": r.get::<String, _>("endpoint"),
            "auth_method": r.get::<String, _>("auth_method"),
            "model_alias": r.get::<Option<String>, _>("model_alias"),
            "fast_alias": r.get::<Option<String>, _>("fast_alias"),
            "env_json": serde_json::from_str::<Value>(&r.get::<String, _>("env_json")).unwrap_or(Value::Null),
            "config_rules_json": serde_json::from_str::<Value>(&r.get::<String, _>("config_rules_json")).unwrap_or(Value::Null),
            "notes": r.get::<Option<String>, _>("notes"),
        })
    })
    .collect();
    sections.push("agent_profiles".to_string());

    // Settings: explicit key allowlist (F-10) — token refs can never appear.
    let mut settings = serde_json::Map::new();
    for key in ["gateway", "retention_days", "openrouter_attribution"] {
        let value = db.get_setting(key).await.map_err(|e| e.to_string())?;
        if let Some(value) = value {
            settings.insert(key.to_string(), value);
        }
    }
    sections.push("settings".to_string());

    Ok((
        serde_json::json!({
            "providers": providers,
            "provider_accounts": accounts,
            "credentials": credentials,
            "models": models,
            "deployments": deployments,
            "routing_policies": policies,
            "aliases": aliases,
            "agent_profiles": profiles,
            "settings": settings,
        }),
        sections,
    ))
}

/// Encrypted payload build: config payload + credential secrets (F-2: values
/// only inside the ciphertext).
pub async fn build_encrypted_payload(
    db: &Db,
    vault: &dyn SecretStore,
) -> Result<(Value, Vec<String>), String> {
    let (mut payload, mut sections) = build_config_payload(db).await?;
    let providers = db
        .repos()
        .list_providers()
        .await
        .map_err(|e| e.to_string())?;
    let mut full_credentials = Vec::new();
    for provider in &providers {
        for credential in db
            .repos()
            .list_credentials(&provider.id)
            .await
            .map_err(|e| e.to_string())?
        {
            let secret_ref =
                sair_vault::SecretRef::parse(&credential.secret_ref).map_err(|e| e.to_string())?;
            let secret = vault
                .retrieve(&secret_ref)
                .map_err(|e| format!("vault unavailable: {e}"))?;
            full_credentials.push(serde_json::json!({
                "id": credential.id,
                "provider_id": credential.provider_id,
                "account_id": credential.account_id,
                "label": credential.label,
                "status": credential.status,
                "priority": credential.priority,
                "weight": credential.weight,
                // F-3: base64 of the raw secret; exists ONLY inside ciphertext.
                "secret_b64": b64(&secret),
            }));
        }
    }
    payload["credentials"] = Value::Array(full_credentials);
    sections.retain(|s| s != "credentials");
    sections.push("credentials+secrets".to_string());
    Ok((payload, sections))
}

#[cfg(test)]
mod tests {
    use super::{
        BackupError, build_config_envelope, build_encrypted_envelope, decrypt_envelope,
        parse_envelope,
    };
    use serde_json::json;

    #[test]
    fn config_envelope_roundtrip_with_integrity() {
        let envelope =
            build_config_envelope("0.1.0", json!({"providers": []}), vec!["providers".into()])
                .unwrap();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        let parsed = parse_envelope(&bytes).unwrap();
        assert_eq!(parsed.mode, "config");
        // Tamper: flip a provider value → integrity fails.
        let mut tampered = envelope;
        tampered["data"] = json!({"providers": [{"tampered": true}]});
        let bytes = serde_json::to_vec(&tampered).unwrap();
        assert!(matches!(
            parse_envelope(&bytes),
            Err(BackupError::Invalid(_))
        ));
    }

    #[test]
    fn encrypted_roundtrip_and_wrong_passphrase() {
        let payload = json!({"credentials": [{"label": "k", "secret": "sk-real"}]});
        let envelope = build_encrypted_envelope(
            "0.1.0",
            payload,
            vec!["credentials".into()],
            "correct horse battery",
        )
        .unwrap();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        let parsed = parse_envelope(&bytes).unwrap();
        // No secret material outside the ciphertext.
        assert!(!serde_json::to_string(&parsed).unwrap().contains("sk-real"));
        let decrypted = decrypt_envelope(&parsed, "correct horse battery").unwrap();
        assert_eq!(decrypted["credentials"][0]["secret"], "sk-real");
        assert!(matches!(
            decrypt_envelope(&parsed, "wrong passphrase entirely"),
            Err(BackupError::AuthFailed)
        ));
        // Tampering with the ciphertext fails authentication.
        let mut tampered = parsed.clone();
        tampered.data = json!("AAAA");
        assert!(matches!(
            decrypt_envelope(&tampered, "correct horse battery"),
            Err(BackupError::AuthFailed)
        ));
    }

    #[test]
    fn size_cap_rejected() {
        let big = vec![b'x'; 65 * 1024 * 1024];
        assert!(matches!(parse_envelope(&big), Err(BackupError::TooLarge)));
    }
}
