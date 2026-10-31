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
///
/// KDF parameters come from the (potentially attacker-supplied) file, so they
/// are bounded before deriving: unbounded Argon2id memory/iterations would let
/// a crafted 3 KB file exhaust RAM/CPU during preview (audit P2 fix).
pub fn decrypt_envelope(envelope: &Envelope, passphrase: &str) -> Result<Value, BackupError> {
    const KDF_M_KIB_MAX: u32 = 1_048_576; // 1 GiB (exports use 64 MiB)
    const KDF_T_MAX: u32 = 10; // exports use 3
    const KDF_P_MAX: u32 = 8; // exports use 1
    let kdf = envelope
        .kdf
        .as_ref()
        .ok_or_else(|| BackupError::Invalid("missing kdf params".to_string()))?;
    if kdf.m_kib == 0
        || kdf.m_kib > KDF_M_KIB_MAX
        || kdf.t == 0
        || kdf.t > KDF_T_MAX
        || kdf.p == 0
        || kdf.p > KDF_P_MAX
    {
        return Err(BackupError::Invalid(
            "kdf parameters out of the allowed range".to_string(),
        ));
    }
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

    // `model` is the models-table NATURAL name (deployments.model_id is the
    // models-row PK, which import cannot resolve by itself — audit P1 fix).
    let deployments: Vec<Value> = sqlx::query(
        "SELECT d.id, d.provider_id, d.model_id, m.model_id AS model_name,
                d.endpoint_override, d.enabled
         FROM deployments d JOIN models m ON m.id = d.model_id",
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
            "model": r.get::<String, _>("model_name"),
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

    // Aliases are exported as UNIQUE rows carrying `policy_name` (resolved —
    // policy row ids do not survive a restore) plus a `bindings` array of
    // (provider_name, model natural name, priority) triples. The previous
    // LEFT-JOIN shape emitted keys the importer never read, so policy links
    // and bindings were silently lost on restore (audit P1 fix).
    let mut aliases: Vec<Value> = sqlx::query(
        "SELECT ma.id, ma.alias, ma.description, p.name AS policy_name
         FROM model_aliases ma
         LEFT JOIN routing_policies p ON p.id = ma.policy_id",
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
            "policy_name": r.get::<Option<String>, _>("policy_name"),
            "bindings": [],
        })
    })
    .collect();
    let binding_rows = sqlx::query(
        "SELECT ma.alias, pr.name AS provider_name, m.model_id AS model_name, ad.priority
         FROM alias_deployments ad
         JOIN model_aliases ma ON ma.id = ad.alias_id
         JOIN deployments d ON d.id = ad.deployment_id
         JOIN models m ON m.id = d.model_id
         JOIN providers pr ON pr.id = d.provider_id
         ORDER BY ma.alias, ad.priority, ma.id",
    )
    .fetch_all(db.pool())
    .await
    .map_err(|e| e.to_string())?;
    for row in &binding_rows {
        let alias: String = row.get("alias");
        let binding = serde_json::json!({
            "provider_name": row.get::<String, _>("provider_name"),
            "model": row.get::<String, _>("model_name"),
            "priority": row.get::<i64, _>("priority"),
        });
        if let Some(entry) = aliases.iter_mut().find(|a| a["alias"] == alias)
            && let Some(b) = entry["bindings"].as_array_mut()
        {
            b.push(binding);
        }
    }
    sections.push("aliases".to_string());

    // Alias REFERENCES in agent_profiles are model_aliases row ids, which do
    // not survive a restore — export the alias NAMES instead so import can
    // re-resolve them (audit P2 fix: profiles were exported but never
    // importable due to dangling id references).
    let profiles: Vec<Value> = sqlx::query(
        "SELECT ap.agent_id, ap.agent_version, ap.protocol_family, ap.endpoint, ap.auth_method,
                ma.alias AS model_alias_name, fa.alias AS fast_alias_name,
                ap.env_json, ap.config_rules_json, ap.notes
         FROM agent_profiles ap
         LEFT JOIN model_aliases ma ON ma.id = ap.model_alias
         LEFT JOIN model_aliases fa ON fa.id = ap.fast_alias",
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
            "model_alias": r.get::<Option<String>, _>("model_alias_name"),
            "fast_alias": r.get::<Option<String>, _>("fast_alias_name"),
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

    #[test]
    fn kdf_params_from_the_file_are_bounded() {
        // A crafted 3 KB backup must not make Argon2id allocate 4 GiB or run
        // a million iterations (audit P2 fix).
        use base64::Engine;
        let mut envelope =
            build_config_envelope("0.1.0", json!({"providers": []}), vec![]).unwrap();
        envelope["mode"] = json!("encrypted");
        let salt_b64 = base64::engine::general_purpose::STANDARD.encode([0u8; 16]);
        envelope["kdf"] = json!({"m_kib": 4_000_000, "t": 3, "p": 1, "salt_b64": salt_b64});
        let envelope: super::Envelope = serde_json::from_value(envelope).unwrap();
        assert!(matches!(
            decrypt_envelope(&envelope, "whatever"),
            Err(BackupError::Invalid(_))
        ));
        let mut envelope = envelope;
        envelope.kdf = Some(super::KdfParams {
            m_kib: 65_536,
            t: 1_000_000,
            p: 1,
            salt_b64: envelope.kdf.as_ref().unwrap().salt_b64.clone(),
        });
        assert!(matches!(
            decrypt_envelope(&envelope, "whatever"),
            Err(BackupError::Invalid(_))
        ));
    }

    /// Regression test for the audit P1 import bugs: a config export must
    /// restore deployments (natural-key `model`), alias policy links, and
    /// alias→deployment bindings.
    #[tokio::test]
    async fn config_export_roundtrips_through_import() {
        // Raw SQL seed (schema-shaped) on the source DB.
        let db1 = sair_persistence::Db::open("sqlite::memory:").await.unwrap();
        db1.run_migrations().await.unwrap();
        for sql in [
            "INSERT INTO providers (id, name, provider_type, base_url, enabled, notes, created_at, updated_at)
             VALUES ('p1','Prov One','openai_compatible','https://api.example.com/v1',1,NULL,'t','t')",
            "INSERT INTO routing_policies (id, name, strategy, max_attempts, retry_budget_ms, timeout_ms, cooldown_seconds, created_at, updated_at)
             VALUES ('pol1','default','priority',3,130000,120000,60,'t','t')",
            "INSERT INTO model_aliases (id, alias, description, policy_id, created_at, updated_at)
             VALUES ('a1','fast-model',NULL,'pol1','t','t')",
            "INSERT INTO models (id, provider_id, model_id, display_name, protocol_family, capabilities, created_at)
             VALUES ('m1','p1','gpt-4o',NULL,'openai_chat','{}','t')",
            "INSERT INTO deployments (id, provider_id, model_id, endpoint_override, enabled, created_at, updated_at)
             VALUES ('d1','p1','m1',NULL,1,'t','t')",
            "INSERT INTO alias_deployments (id, alias_id, deployment_id, priority)
             VALUES ('ad1','a1','d1',10)",
        ] {
            sqlx::query(sql).execute(db1.pool()).await.unwrap();
        }

        let (payload, _sections) = super::build_config_payload(&db1).await.unwrap();
        let envelope = build_config_envelope("0.1.0", payload, vec![]).unwrap();
        let bytes = serde_json::to_vec(&envelope).unwrap();
        let parsed = parse_envelope(&bytes).unwrap();

        // Restore into a FRESH database (the disaster-recovery scenario).
        let db2 = sair_persistence::Db::open("sqlite::memory:").await.unwrap();
        db2.run_migrations().await.unwrap();
        let report = crate::backup_import::stage_import(&db2, &parsed.data)
            .await
            .unwrap();
        assert_eq!(report["providers"]["added"], 1, "{report}");
        assert_eq!(report["models"]["added"], 1, "{report}");
        assert_eq!(report["deployments"]["added"], 1, "{report}");
        assert_eq!(report["deployments"]["skipped"], 0, "{report}");
        assert_eq!(report["aliases"]["added"], 1, "{report}");

        // The deployment resolved the models-row PK from the NATURAL name.
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM deployments d
             JOIN models m ON m.id = d.model_id
             JOIN providers p ON p.id = d.provider_id
             WHERE p.name = 'Prov One' AND m.model_id = 'gpt-4o'",
        )
        .fetch_one(db2.pool())
        .await
        .unwrap();
        assert_eq!(
            row.0, 1,
            "deployment must resolve via the natural model name"
        );

        // The alias policy link and the binding survived.
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM model_aliases ma
             JOIN routing_policies p ON p.id = ma.policy_id
             WHERE ma.alias = 'fast-model' AND p.name = 'default'",
        )
        .fetch_one(db2.pool())
        .await
        .unwrap();
        assert_eq!(row.0, 1, "alias policy link must be restored");

        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM alias_deployments ad
             JOIN model_aliases ma ON ma.id = ad.alias_id
             JOIN deployments d ON d.id = ad.deployment_id
             JOIN models m ON m.id = d.model_id
             WHERE ma.alias = 'fast-model' AND m.model_id = 'gpt-4o' AND ad.priority = 10",
        )
        .fetch_one(db2.pool())
        .await
        .unwrap();
        assert_eq!(row.0, 1, "alias→deployment binding must be restored");
    }
}
