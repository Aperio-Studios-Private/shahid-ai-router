# P2-T06 Dependency Audit Note (2026-10-31)

Tool: cargo-audit 0.22 (RustSec database, accessed 2026-10-31).

**Result: 0 vulnerabilities. 5 warnings** — all on Linux-only transitive dependencies of Tauri's Linux stack (zbus 5.x family, glib 0.18) plus the unmaintained `proc-macro-error` (via a derive macro). These crates are not compiled into any supported target (Windows x64, macOS x64/aarch64 — ADR-014); they appear in Cargo.lock because the lockfile is platform-independent.

| ID | Crate | Type | Relevance to supported targets |
|---|---|---|---|
| RUSTSEC-2026-0370 | zvariant 5.15.1 | unmaintained | None (Linux D-Bus stack) |
| RUSTSEC-2026-0372 | zvariant_derive 5.15.1 | unmaintained | None (Linux D-Bus stack) |
| RUSTSEC-2026-0371 | zbus_names 5.15.1 | unmaintained | None (Linux D-Bus stack) |
| RUSTSEC-2024-0370 | proc-macro-error | unmaintained | Build-time only, not in shipped binaries |
| RUSTSEC-2024-0429 | glib 0.18.5 | unsound (VariantStrIter) | None (Linux GUI stack) |

Disposition: Informational. Tracked; revisit trigger: any Linux target decision, or if any of these crates enter the Windows/macOS dependency graph. Re-audit scheduled per phase gate.
