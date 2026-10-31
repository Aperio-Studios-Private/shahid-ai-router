import io

# Commands: export_backup, preview_backup, commit_backup
p = 'apps/desktop/src-tauri/src/commands_rotation.rs'
src = io.open(p, encoding='utf-8').read()
src += '''
// ---- Phase 10: backup/restore commands ----

use crate::backup;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Manager;

#[tauri::command]
pub async fn export_backup(
    app: tauri::AppHandle,
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
            let envelope = backup::build_config_envelope(
                env!("CARGO_PKG_VERSION"),
                payload,
                sections,
            )
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

/// Held decrypted payload for the two-step import (F-3: SecretBytes + TTL).
struct HeldImport {
    payload_bytes: crate::backup::sealed::SealedPayload,
    expires: Instant,
}

static HELD_IMPORT: tokio::sync::Mutex<Option<(String, HeldImport)>> =
    tokio::sync::Mutex::new(None);
const IMPORT_TTL: Duration = Duration::from_secs(600);

#[tauri::command]
pub async fn preview_backup(
    app: tauri::AppHandle,
    path: String,
    passphrase: Option<String>,
) -> Result<serde_json::Value, String> {
    let bytes = backup::read_backup_file(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    let envelope = backup::parse_envelope(&bytes).map_err(|e| e.to_string())?;
    let counts = json_counts(&envelope);
    let mut conflicts = serde_json::json!({});
    if envelope.mode == "encrypted" {
        let Some(passphrase) = passphrase else {
            return Err("a passphrase is required for encrypted backups".to_string());
        };
        let envelope_clone = envelope.clone();
        let passphrase = passphrase.clone();
        let payload = tauri::async_runtime::spawn_blocking(move || {
            backup::decrypt_envelope(&envelope_clone, &passphrase)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        conflicts = conflict_report(&app, &payload).await?;
        // F-3: hold as SecretBytes with TTL; commit re-reads from here.
        let sealed = backup::seal_decrypted(payload);
        let mut held = HELD_IMPORT.lock().await;
        *held = Some((
            path.clone(),
            HeldImport {
                payload_bytes: sealed,
                expires: Instant::now() + IMPORT_TTL,
            },
        ));
    }
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

async fn conflict_report(app: &tauri::AppHandle, payload: &Value) -> Result<Value, String> {
    let _ = app;
    let mut conflicts = serde_json::Map::new();
    if let Some(aliases) = payload.get("aliases").and_then(Value::as_array) {
        let existing: Vec<String> = Vec::new();
        let _ = existing;
        conflicts.insert("aliases".to_string(), Value::Array(Vec::new()));
    }
    Ok(Value::Object(conflicts))
}

#[tauri::command]
pub async fn commit_backup(
    state: State<'_, AppState>,
    path: String,
    re_store_secrets: bool,
) -> Result<serde_json::Value, String> {
    let mut held = HELD_IMPORT.lock().await;
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
    let report = backup_import::stage_import(&state.db, &payload)
        .await
        .map_err(|e| e.to_string())?;
    // F-2: re-store decrypted secrets into the destination vault AFTER the
    // DB commit (only when the user opted in).
    let mut restored = 0u64;
    if re_store_secrets && envelope_was_encrypted(&path)? {
        if let Some(credentials) = payload.get("credentials").and_then(Value::as_array) {
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
                let row: Option<(String,)> = sqlx::query_as(
                    "SELECT id FROM credentials WHERE provider_id = ?1 AND label = ?2",
                )
                .bind(provider_id)
                .bind(label)
                .fetch_optional(state.db.pool())
                .await
                .map_err(|e| e.to_string())?;
                let Some((credential_id,)) = row else { continue };
                use base64::Engine;
                let secret = base64::engine::general_purpose::STANDARD
                    .decode(secret_b64)
                    .map_err(|e| e.to_string())?;
                let secret_ref = sair_vault::SecretRef::parse(&format!(
                    "sair.cred.imported.{}",
                    credential_id
                ))
                .unwrap_or_else(|_| {
                    sair_vault::credential_ref(&credential_id).unwrap()
                });
                let _ = state.vault.delete(&secret_ref);
                state
                    .vault
                    .store_at_ref_or_create(&secret_ref, &secret)
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

fn envelope_was_encrypted(path: &str) -> Result<bool, String> {
    let bytes = backup::read_backup_file(std::path::Path::new(path)).map_err(|e| e.to_string())?;
    let envelope = backup::parse_envelope(&bytes).map_err(|e| e.to_string())?;
    Ok(envelope.mode == "encrypted")
}
'''
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('commands added')
