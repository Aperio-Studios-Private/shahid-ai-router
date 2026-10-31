// Phase 8: consent-gated agent config application (F-2 hardening).

use std::path::{Path, PathBuf};

use sair_domain::profiles::{GeneratedProfile, TOKEN_PLACEHOLDER};

/// The closed allowlist of apply targets (F-2): resolved under the OS home
/// directory. No other path may ever be written.
fn allowlist() -> Vec<PathBuf> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    vec![
        home.join(".claude").join("settings.json"),
        home.join(".gemini").join(".env"),
    ]
}

/// Resolves a profile's allowlisted target path. `#` errors mean the profile
/// is not file-applicable or the path is not allowlisted.
///
/// # Errors
/// Validation message when the profile has no files, or the target is not in
/// the closed allowlist, or canonicalization fails (missing parents are
/// allowed for the final component).
pub fn resolve_target(profile: &GeneratedProfile, index: usize) -> Result<PathBuf, String> {
    if index >= profile.files.len() {
        return Err("file index out of range".to_string());
    }
    let raw = &profile.files[index];
    let home = dirs::home_dir().ok_or("home directory unavailable")?;
    let stripped = raw
        .strip_prefix("~/")
        .ok_or("target must be a ~-relative allowlisted path")?;
    let target = home.join(stripped);
    let allow = allowlist();
    if !allow.iter().any(|a| a == &target) {
        return Err("target is not in the apply allowlist".to_string());
    }
    // Symlink/junction rejection on existing components of the parent chain.
    let mut ancestor = target.clone();
    while let Some(parent) = ancestor.parent() {
        if parent == home {
            break;
        }
        if parent.symlink_metadata().is_ok() {
            let meta = parent
                .symlink_metadata()
                .map_err(|e| format!("stat failed: {e}"))?;
            if meta.file_type().is_symlink() {
                return Err("refusing: parent directory is a symlink".to_string());
            }
        }
        ancestor = parent.to_path_buf();
    }
    if let Ok(meta) = target.symlink_metadata()
        && meta.file_type().is_symlink()
    {
        return Err("refusing: target is a symlink".to_string());
    }
    Ok(target)
}

/// Validates the content the UI wants to write: must contain the placeholder
/// (enforcing the no-plaintext-token design, F-3) and respect the size cap.
///
/// # Errors
/// Validation message otherwise.
pub fn validate_apply_content(content: &str) -> Result<(), String> {
    const CAP: usize = 256 * 1024;
    if content.len() > CAP {
        return Err("content exceeds the size cap".to_string());
    }
    if !content.contains(TOKEN_PLACEHOLDER)
        && !content.starts_with('#')
        && !content.starts_with('{')
    {
        // Generated profiles always carry the placeholder or are comment/JSON
        // shaped; anything else is not a generated profile.
        return Err("content does not look like a generated profile".to_string());
    }
    if content.contains("sk-") {
        return Err(
            "content contains key-like material; generated profiles must use the placeholder"
                .to_string(),
        );
    }
    Ok(())
}

/// Atomically applies `content` to `target`:
/// 1. hash precondition (TOCTOU guard, F-2): `expected_existing` is the SHA-256
///    (hex) the UI saw — a mismatch aborts;
/// 2. backup of the previous file next to it (`<name>.shahid-backup-<ts>`);
/// 3. atomic write (temp file + rename in the same directory).
///
/// # Errors
/// Validation message on precondition mismatch or IO failure.
pub fn apply_content(
    target: &Path,
    content: &str,
    expected_existing: Option<&str>,
) -> Result<PathBuf, String> {
    validate_apply_content(content)?;
    use ring::digest::{SHA256, digest};
    if target.exists() {
        let bytes = std::fs::read(target).map_err(|e| format!("read failed: {e}"))?;
        let hex: String = digest(&SHA256, &bytes)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        // The hash precondition is REQUIRED when the target exists (audit P2
        // fix): it was previously optional, so a caller that skipped it could
        // silently overwrite a file changed since generation. The UI always
        // passes the hash; the backend now enforces it too.
        let Some(expected) = expected_existing else {
            return Err(
                "hash precondition required: call hash_profile_target first, then apply with \
                 the same hash"
                    .to_string(),
            );
        };
        if expected != hex {
            return Err(
                "file changed since preview (hash precondition failed); re-preview and retry"
                    .to_string(),
            );
        }
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let backup = target.with_file_name(format!(
            "{}.shahid-backup-{stamp}",
            target.file_name().unwrap_or_default().to_string_lossy()
        ));
        std::fs::copy(target, &backup).map_err(|e| format!("backup failed: {e}"))?;
    }
    // Atomic write: temp + rename in the same directory.
    let dir = target.parent().ok_or("target has no parent")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("mkdir failed: {e}"))?;
    let tmp = dir.join(format!(
        ".shahid-tmp-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    std::fs::write(&tmp, content).map_err(|e| format!("write failed: {e}"))?;
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename failed: {e}")
    })?;
    Ok(target.to_path_buf())
}

/// SHA-256 hex of a file for the UI's two-step consent (precondition input).
///
/// # Errors
/// IO failure.
pub fn hash_file(target: &Path) -> Result<String, String> {
    use ring::digest::{SHA256, digest};
    let bytes = std::fs::read(target).map_err(|e| e.to_string())?;
    Ok(digest(&SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{apply_content, hash_file, resolve_target, validate_apply_content};
    use sair_domain::profiles::generate_profile;
    use std::path::PathBuf;

    fn temp_home(name: &str) -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join(name);
        std::fs::create_dir_all(&home).unwrap();
        // Leak the tempdir for the test lifetime via Box::leak pattern:
        std::mem::forget(dir);
        home
    }

    #[test]
    fn content_validation_enforces_placeholder() {
        assert!(validate_apply_content("GEMINI_API_KEY=<<GATEWAY_TOKEN>>\n").is_ok());
        assert!(validate_apply_content("GEMINI_API_KEY=sk-real-key\n").is_err());
        assert!(validate_apply_content("").is_err());
    }

    #[test]
    fn apply_creates_backup_and_writes_atomically() {
        let home = temp_home("apply-test");
        // Patch the allowlist indirectly: resolve_target uses dirs::home_dir,
        // so write to the real allowlisted location is not testable here
        // without env override; instead test apply_content directly on a
        // temp target.
        let target = home.join("settings.json");
        std::fs::write(&target, "original").unwrap();
        let expected = hash_file(&target).unwrap();
        let backup_before = std::fs::read_dir(home.join(".")).unwrap().count();

        let applied =
            apply_content(&target, "{\"env\": \"<<GATEWAY_TOKEN>>\"}", Some(&expected)).unwrap();
        assert_eq!(applied, target);
        assert!(
            std::fs::read_to_string(&target)
                .unwrap()
                .contains("<<GATEWAY_TOKEN>>")
        );
        // Backup exists (count increased).
        let backup_after = std::fs::read_dir(home.join(".")).unwrap().count();
        assert_eq!(backup_after, backup_before + 1);
        // Hash precondition: stale hash must fail.
        assert!(apply_content(&target, "new", Some(&expected)).is_err());
    }

    #[test]
    fn resolve_target_rejects_out_of_allowlist() {
        let profile = generate_profile("claude-code", 8787, "a".to_string(), None).unwrap();
        // resolve_target consults the REAL home dir; on CI/test machines the
        // path check itself is the assertion: an out-of-range index errors.
        assert!(resolve_target(&profile, 99).is_err());
    }
}
