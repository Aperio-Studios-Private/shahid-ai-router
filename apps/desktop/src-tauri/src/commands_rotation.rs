// ---- Phase 4 commands: rotation & health completion (FR-002/012) ----

use serde::Serialize;
use serde_json::Value;
use tauri::State;

use std::time::{Duration, Instant};

use crate::backup;
use crate::state::AppState;

#[tauri::command]
#[allow(clippy::too_many_arguments)] // none: 4 args only — kept symmetrical
pub async fn set_credential_priority_weight(
    state: State<'_, AppState>,
    id: String,
    priority: i64,
    weight: i64,
) -> Result<(), String> {
    // Cloud auto-backup (Phase 12): config may change → schedule a sync.
    state.mark_cloud_dirty();

    state
        .db
        .repos()
        .set_credential_priority_weight(&id, priority, weight)
        .await
}

#[tauri::command]
pub async fn list_provider_accounts(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    state
        .db
        .repos()
        .list_provider_accounts(&provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_provider_account(
    state: State<'_, AppState>,
    provider_id: String,
    label: String,
    group_key: Option<String>,
) -> Result<String, String> {
    // Cloud auto-backup (Phase 12): config may change → schedule a sync.
    state.mark_cloud_dirty();

    state
        .db
        .repos()
        .insert_provider_account(&provider_id, &label, group_key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn assign_credential_account(
    state: State<'_, AppState>,
    credential_id: String,
    account_id: String,
) -> Result<(), String> {
    // Cloud auto-backup (Phase 12): config may change → schedule a sync.
    state.mark_cloud_dirty();

    state
        .db
        .repos()
        .assign_credential_account(&credential_id, &account_id)
        .await
}

#[derive(Debug, Serialize)]
pub struct HealthEventView {
    pub ts: String,
    pub subject_type: String,
    pub subject_id: String,
    pub event: String,
    pub category: Option<String>,
    pub detail: Option<String>,
    pub correlation_id: Option<String>,
}

#[tauri::command]
pub async fn list_health_events(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<HealthEventView>, String> {
    Ok(state
        .db
        .repos()
        .list_health_events(limit.unwrap_or(100))
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|v| HealthEventView {
            ts: v["ts"].as_str().unwrap_or_default().to_string(),
            subject_type: v["subject_type"].as_str().unwrap_or_default().to_string(),
            subject_id: v["subject_id"].as_str().unwrap_or_default().to_string(),
            event: v["event"].as_str().unwrap_or_default().to_string(),
            category: v["category"].as_str().map(str::to_string),
            detail: v["detail"].as_str().map(str::to_string),
            correlation_id: v["correlation_id"].as_str().map(str::to_string),
        })
        .collect())
}

#[tauri::command]
pub async fn credential_health_summary(
    state: State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    state
        .db
        .repos()
        .credential_health_summary()
        .await
        .map_err(|e| e.to_string())
}

// ---- Phase 8: agent profile generation + consent-gated application ----

use crate::profile_apply;

#[tauri::command]
pub async fn generate_agent_profile(
    state: State<'_, AppState>,
    agent_id: String,
    alias: String,
    fast_alias: Option<String>,
) -> Result<serde_json::Value, String> {
    // F-5: validate alias existence + same-protocol deployment presence.
    if !state
        .db
        .repos()
        .alias_exists(&alias)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err(format!("alias `{alias}` does not exist"));
    }
    let family = match agent_id.as_str() {
        "claude-code" => "anthropic_messages",
        "gemini-cli" => "gemini_generate_content",
        "generic-openai" | "codex-cli" => "openai_chat",
        _ => return Err("unknown agent".to_string()),
    };
    if !state
        .db
        .repos()
        .alias_has_family_deployments(&alias, family)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err(format!(
            "alias `{alias}` has no enabled {family} deployment"
        ));
    }

    let settings = state
        .db
        .get_setting(crate::state::SETTING_GATEWAY)
        .await
        .map_err(|e| e.to_string())?;
    let port = settings
        .as_ref()
        .and_then(|o| o.get("port"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(8787) as u16;

    let profile =
        sair_domain::profiles::generate_profile(&agent_id, port, alias.clone(), fast_alias)
            .ok_or("unknown agent")?;
    Ok(serde_json::json!({
        "agent_id": profile.agent_id,
        "config_text": profile.config_text,
        "files": profile.files,
        "rollback": profile.rollback,
        "limitations": profile.limitations,
        "applicable": profile.applicable,
        "verification": format!(
            "{} on {} (contract tests)",
            sair_domain::profiles::VERIFICATION_PREFIX,
            chrono::Utc::now().format("%Y-%m-%d")
        ),
    }))
}

#[tauri::command]
pub async fn hash_profile_target(agent_id: String, file_index: usize) -> Result<String, String> {
    let profile = sair_domain::profiles::generate_profile(&agent_id, 8787, String::new(), None)
        .ok_or("unknown agent")?;
    let target = profile_apply::resolve_target(&profile, file_index)?;
    if !target.exists() {
        return Ok(String::new()); // new file: empty hash = no precondition
    }
    profile_apply::hash_file(&target)
}

#[tauri::command]
pub async fn apply_agent_profile(
    state: State<'_, AppState>,
    agent_id: String,
    file_index: usize,
    content: String,
    expected_existing: Option<String>,
) -> Result<String, String> {
    // Cloud auto-backup (Phase 12): config may change → schedule a sync.
    state.mark_cloud_dirty();

    // Consent is the UI's confirm dialog; the backend enforces the rest.
    let profile = sair_domain::profiles::generate_profile(&agent_id, 8787, String::new(), None)
        .ok_or("unknown agent")?;
    let target = profile_apply::resolve_target(&profile, file_index)?;
    profile_apply::apply_content(&target, &content, expected_existing.as_deref())
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn list_request_records(
    state: State<'_, AppState>,
    limit: Option<i64>,
    alias_filter: Option<String>,
    errors_only: Option<bool>,
) -> Result<Vec<serde_json::Value>, String> {
    state
        .db
        .repos()
        .list_request_records(
            limit.unwrap_or(100),
            alias_filter.as_deref(),
            errors_only.unwrap_or(false),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn usage_summary(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    state
        .db
        .repos()
        .usage_summary()
        .await
        .map_err(|e| e.to_string())
}

// ---- Phase 10: backup/restore commands ----

#[tauri::command]
pub async fn export_backup(
    state: State<'_, AppState>,
    path: String,
    mode: String,
    passphrase: Option<String>,
) -> Result<serde_json::Value, String> {
    if path.is_empty() {
        return Err("export path is required".to_string());
    }
    match mode.as_str() {
        "config" => {
            let (payload, sections) = backup::build_config_payload(&state.db).await?;
            let envelope =
                backup::build_config_envelope(env!("CARGO_PKG_VERSION"), payload, sections)
                    .map_err(|e| e.to_string())?;
            let bytes = serde_json::to_vec_pretty(&envelope).map_err(|e| e.to_string())?;
            std::fs::write(&path, bytes).map_err(|e| format!("write failed: {e}"))?;
            Ok(serde_json::json!({
                "path": path,
                "mode": "config",
                "warning": "Config-only backups contain NO API keys. Credential statuses are set to incomplete.",
            }))
        }
        "encrypted" => {
            let Some(passphrase) = passphrase else {
                return Err("a passphrase is required for encrypted backups".to_string());
            };
            if passphrase.len() < 12 {
                return Err("passphrase must be at least 12 characters".to_string());
            }
            let (payload, sections) =
                backup::build_encrypted_payload(&state.db, state.vault.as_ref()).await?;
            let app_version = env!("CARGO_PKG_VERSION").to_string();
            let envelope = tauri::async_runtime::spawn_blocking(move || {
                backup::build_encrypted_envelope(&app_version, payload, sections, &passphrase)
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
            let bytes = serde_json::to_vec_pretty(&envelope).map_err(|e| e.to_string())?;
            std::fs::write(&path, bytes).map_err(|e| format!("write failed: {e}"))?;
            Ok(serde_json::json!({
                "path": path,
                "mode": "encrypted",
                "warning": "Losing the passphrase means the backup is UNRECOVERABLE (docs/10). Store it safely.",
            }))
        }
        other => Err(format!("unknown backup mode: {other}")),
    }
}

/// Held payload for the two-step import (F-3: SecretBytes + TTL). `mode`
/// is captured at preview time so commit never re-reads the file (no TOCTOU
/// between preview and commit).
struct HeldImport {
    payload_bytes: sair_vault::SecretBytes,
    expires: Instant,
    mode: String,
}

fn held_import_slot() -> &'static tokio::sync::Mutex<Option<(String, HeldImport)>> {
    static SLOT: std::sync::OnceLock<tokio::sync::Mutex<Option<(String, HeldImport)>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| tokio::sync::Mutex::new(None))
}
const IMPORT_TTL: Duration = Duration::from_secs(600);

#[tauri::command]
pub async fn preview_backup(
    state: State<'_, AppState>,
    path: String,
    passphrase: Option<String>,
) -> Result<serde_json::Value, String> {
    let bytes = backup::read_backup_file(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    let envelope = backup::parse_envelope(&bytes).map_err(|e| e.to_string())?;
    let counts = json_counts(&envelope);
    // Both modes hold the payload for commit (audit P1 fix: config mode
    // previously could never commit because only encrypted previews were
    // held). Config payloads are not secret; SecretBytes is a harmless
    // zeroizing wrapper for them.
    let payload = if envelope.mode == "encrypted" {
        let Some(passphrase) = passphrase else {
            return Err("a passphrase is required for encrypted backups".to_string());
        };
        let envelope_clone = envelope.clone();
        tauri::async_runtime::spawn_blocking(move || {
            backup::decrypt_envelope(&envelope_clone, &passphrase)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
    } else {
        envelope.data.clone()
    };
    let conflicts = conflict_report(&state.db, &payload).await?;
    // F-3: hold as SecretBytes with TTL; commit re-reads from here.
    let sealed = backup::seal_decrypted(payload);
    let mut held = held_import_slot().lock().await;
    *held = Some((
        path.clone(),
        HeldImport {
            payload_bytes: sealed,
            expires: Instant::now() + IMPORT_TTL,
            mode: envelope.mode.clone(),
        },
    ));
    Ok(serde_json::json!({
        "mode": envelope.mode,
        "created_at": envelope.created_at,
        "app_version": envelope.app_version,
        "included_sections": envelope.included_sections,
        "counts": counts,
        "conflicts": conflicts,
        "lost_passphrase_warning": envelope.mode == "encrypted",
    }))
}

fn json_counts(envelope: &backup::Envelope) -> Value {
    let count = |key: &str| -> usize {
        envelope
            .data
            .get(key)
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0)
    };
    serde_json::json!({
        "providers": count("providers"),
        "credentials": count("credentials"),
        "models": count("models"),
        "deployments": count("deployments"),
        "aliases": count("aliases"),
        "routing_policies": count("routing_policies"),
        "agent_profiles": count("agent_profiles"),
    })
}

/// Real conflict detection (audit P2 fix: the stub always reported zero).
/// Reports the natural keys the importer would SKIP, per section, so the
/// consent dialog shows what already exists locally.
async fn conflict_report(db: &sair_persistence::Db, payload: &Value) -> Result<Value, String> {
    let mut conflicts = serde_json::Map::new();

    let mut provider_keys = Vec::new();
    for item in payload
        .get("providers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = item.get("id").and_then(Value::as_str) {
            let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers WHERE id = ?1")
                .bind(id)
                .fetch_one(db.pool())
                .await
                .map_err(|e| e.to_string())?;
            if exists > 0 {
                provider_keys.push(id.to_string());
            }
        }
    }
    conflicts.insert(
        "providers".to_string(),
        Value::Array(provider_keys.into_iter().map(Value::String).collect()),
    );

    let mut alias_keys = Vec::new();
    for item in payload
        .get("aliases")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(alias) = item.get("alias").and_then(Value::as_str) {
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM model_aliases WHERE alias = ?1")
                    .bind(alias)
                    .fetch_one(db.pool())
                    .await
                    .map_err(|e| e.to_string())?;
            if exists > 0 {
                alias_keys.push(alias.to_string());
            }
        }
    }
    conflicts.insert(
        "aliases".to_string(),
        Value::Array(alias_keys.into_iter().map(Value::String).collect()),
    );

    let mut credential_keys = Vec::new();
    for item in payload
        .get("credentials")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(provider_id), Some(label)) = (
            item.get("provider_id").and_then(Value::as_str),
            item.get("label").and_then(Value::as_str),
        ) else {
            continue;
        };
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM credentials WHERE provider_id = ?1 AND label = ?2",
        )
        .bind(provider_id)
        .bind(label)
        .fetch_one(db.pool())
        .await
        .map_err(|e| e.to_string())?;
        if exists > 0 {
            credential_keys.push(format!("{provider_id}/{label}"));
        }
    }
    conflicts.insert(
        "credentials".to_string(),
        Value::Array(credential_keys.into_iter().map(Value::String).collect()),
    );

    let mut model_keys = Vec::new();
    for item in payload
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(provider_id), Some(model_id)) = (
            item.get("provider_id").and_then(Value::as_str),
            item.get("model_id").and_then(Value::as_str),
        ) else {
            continue;
        };
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM models WHERE provider_id = ?1 AND model_id = ?2",
        )
        .bind(provider_id)
        .bind(model_id)
        .fetch_one(db.pool())
        .await
        .map_err(|e| e.to_string())?;
        if exists > 0 {
            model_keys.push(format!("{provider_id}/{model_id}"));
        }
    }
    conflicts.insert(
        "models".to_string(),
        Value::Array(model_keys.into_iter().map(Value::String).collect()),
    );

    let mut policy_keys = Vec::new();
    for item in payload
        .get("routing_policies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = item.get("name").and_then(Value::as_str) {
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM routing_policies WHERE name = ?1")
                    .bind(name)
                    .fetch_one(db.pool())
                    .await
                    .map_err(|e| e.to_string())?;
            if exists > 0 {
                policy_keys.push(name.to_string());
            }
        }
    }
    conflicts.insert(
        "routing_policies".to_string(),
        Value::Array(policy_keys.into_iter().map(Value::String).collect()),
    );

    Ok(Value::Object(conflicts))
}

#[tauri::command]
pub async fn commit_backup(
    state: State<'_, AppState>,
    path: String,
    re_store_secrets: bool,
) -> Result<serde_json::Value, String> {
    // Cloud auto-backup (Phase 12): config may change → schedule a sync.
    state.mark_cloud_dirty();

    let mut held = held_import_slot().lock().await;
    let Some((held_path, held_payload)) = held.as_ref() else {
        return Err("no preview in progress (or it expired) — preview first".to_string());
    };
    if *held_path != path {
        return Err("preview path mismatch — preview the same file first".to_string());
    }
    if held_payload.expires < Instant::now() {
        *held = None;
        return Err("preview expired (10 minutes) — preview again".to_string());
    }
    let Some(payload) = backup::unseal_decrypted(&held_payload.payload_bytes) else {
        return Err("held payload unavailable — preview again".to_string());
    };
    let report = crate::backup_import::stage_import(&state.db, &payload)
        .await
        .map_err(|e| e.to_string())?;
    // F-2: re-store decrypted secrets into the destination vault AFTER the
    // DB commit (only when the user opted in). The mode captured at preview
    // time is authoritative — commit never re-reads the file.
    let mut restored = 0u64;
    if re_store_secrets
        && held_payload.mode == "encrypted"
        && let Some(credentials) = payload.get("credentials").and_then(Value::as_array)
    {
        for credential in credentials {
            let (Some(label), Some(secret_b64)) = (
                credential.get("label").and_then(Value::as_str),
                credential.get("secret_b64").and_then(Value::as_str),
            ) else {
                continue;
            };
            let Some(provider_id) = credential.get("provider_id").and_then(Value::as_str) else {
                continue;
            };
            // Find the imported credential row by (provider_id, label).
            let row: Option<(String,)> =
                sqlx::query_as("SELECT id FROM credentials WHERE provider_id = ?1 AND label = ?2")
                    .bind(provider_id)
                    .bind(label)
                    .fetch_optional(state.db.pool())
                    .await
                    .map_err(|e| e.to_string())?;
            let Some((credential_id,)) = row else {
                continue;
            };
            use base64::Engine;
            let secret = base64::engine::general_purpose::STANDARD
                .decode(secret_b64)
                .map_err(|e| e.to_string())?;
            // Canonical minted ref: the vault generates it; we then
            // persist it on the credential row.
            let secret_ref = state
                .vault
                .store(&secret)
                .map_err(|e| format!("vault re-store failed: {e}"))?;
            sqlx::query("UPDATE credentials SET secret_ref = ?2, status = 'active' WHERE id = ?1")
                .bind(&credential_id)
                .bind(secret_ref.as_str())
                .execute(state.db.pool())
                .await
                .map_err(|e| e.to_string())?;
            restored += 1;
        }
    }
    *held = None;
    Ok(serde_json::json!({
        "report": report,
        "secrets_restored": restored,
        "note": if restored == 0 && re_store_secrets {
            "no secrets were re-stored (config mode or none present); imported credentials are incomplete"
        } else {
            "secrets re-stored into the OS vault"
        },
    }))
}
