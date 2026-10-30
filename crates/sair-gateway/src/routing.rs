//! Routing engine v0 (FR-004/007/012, docs/05 selection algorithm, ADR-017/018).
//!
//! Rotation state sits behind `tokio::sync::Mutex` (ADR-012); critical
//! sections only mutate the per-alias round-robin counter and never span an
//! await. Candidates are (alias-deployment, credential) pairs composed per
//! ADR-018.

use std::collections::HashMap;
use std::sync::Mutex;

use sair_domain::error::UpstreamFailure;
use sair_domain::protocol::ProtocolFamily;
use sair_domain::routing::{Exclusion, RouteCandidate, RoutingDecision, Strategy};
use sair_persistence::repos::ExcludedCandidate;

/// In-memory rotation bookkeeping (ADR-012). Guarded by a mutex with short
/// critical sections.
#[derive(Default)]
pub struct RotationState {
    counters: Mutex<HashMap<String, u64>>,
}

impl RotationState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances and returns the round-robin offset for an alias.
    fn next_offset(&self, alias: &str, len: usize) -> usize {
        let mut counters = self
            .counters
            .lock()
            .expect("rotation counters poisoned; unrecoverable in-process state");
        let counter = counters.entry(alias.to_string()).or_insert(0);
        let offset = (*counter % len.max(1) as u64) as usize;
        *counter = counter.wrapping_add(1);
        offset
    }

    /// Deterministic random in [0, 1) from the OS CSPRNG (weighted shuffle).
    fn random_unit() -> f64 {
        use ring::rand::SecureRandom;
        let rng = ring::rand::SystemRandom::new();
        let mut bytes = [0u8; 8];
        rng.fill(&mut bytes)
            .expect("OS CSPRNG must not fail; unrecoverable");
        u64::from_be_bytes(bytes) as f64 / (u64::MAX as f64 + 1.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error("alias not found")]
    AliasNotFound,
    #[error("no eligible deployment for this request")]
    NoEligibleDeployment,
    #[error(transparent)]
    UnimplementedStrategy(#[from] sair_domain::routing::UnimplementedStrategy),
}

/// Orders candidates and produces the decision record. `excluded` carries
/// the pre-filtered ineligible rows with reasons (FR-012).
///
/// # Errors
/// [`RouteError::AliasNotFound`] when the snapshot is empty,
/// [`RouteError::NoEligibleDeployment`] when nothing survives the filters,
/// [`RouteError::UnimplementedStrategy`] for lru/manual_pin (ADR-018).
pub fn plan_route(
    alias: &str,
    family: ProtocolFamily,
    strategy: Strategy,
    candidates: &[RouteCandidate],
    excluded: &[ExcludedCandidate],
    rotation: &RotationState,
) -> Result<RoutingDecision, RouteError> {
    strategy.ensure_implemented()?;

    if candidates.is_empty() && excluded.is_empty() {
        return Err(RouteError::AliasNotFound);
    }

    // Capability filter (FR-007): same protocol family only (ADR-009).
    let mut exclusions: Vec<Exclusion> = excluded
        .iter()
        .map(|e| Exclusion {
            deployment_id: e.deployment_id.clone(),
            credential_id: e.credential_id.clone(),
            reason: e.reason.clone(),
        })
        .collect();
    let eligible: Vec<RouteCandidate> = candidates
        .iter()
        .filter(|c| {
            let ok = c.capabilities.supports_protocol(family);
            if !ok {
                exclusions.push(Exclusion {
                    deployment_id: c.deployment_id.clone(),
                    credential_id: c.credential_id.clone(),
                    reason: "protocol_mismatch".to_string(),
                });
            }
            ok
        })
        .cloned()
        .collect();

    if eligible.is_empty() {
        return Err(RouteError::NoEligibleDeployment);
    }

    // Base order: lexicographic (alias_priority, credential_priority) — the
    // SQL already orders this way; sort here to make the invariant local.
    let mut ordered = eligible;
    ordered.sort_by(|a, b| a.order_key().cmp(&b.order_key()).reverse());

    match strategy {
        Strategy::Priority => {}
        Strategy::RoundRobin => {
            let offset = rotation.next_offset(alias, ordered.len());
            ordered.rotate_left(offset);
        }
        Strategy::Weighted => {
            ordered = weighted_shuffle(ordered);
        }
        // ensure_implemented rejects these earlier.
        Strategy::Lru | Strategy::ManualPin => unreachable!("rejected by ensure_implemented"),
    }

    Ok(RoutingDecision {
        alias: alias.to_string(),
        strategy,
        selected: ordered.first().cloned(),
        ordered,
        exclusions,
    })
}

/// Weighted shuffle without replacement (Efraimidis–Spirakis: sort by
/// weight^(1/u) descending). Uses credential weights, floor 1 (ADR-018).
fn weighted_shuffle(mut candidates: Vec<RouteCandidate>) -> Vec<RouteCandidate> {
    let mut keyed: Vec<(f64, RouteCandidate)> = candidates
        .drain(..)
        .map(|c| {
            let weight = c.credential_weight.max(1) as f64;
            let u = RotationState::random_unit().max(f64::MIN_POSITIVE);
            (weight.powf(1.0 / u), c)
        })
        .collect();
    keyed.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    keyed.into_iter().map(|(_, c)| c).collect()
}

/// Cooldown seconds to apply for a failure (ADR-017): Retry-After when the
/// provider supplied one, else the policy cooldown.
#[must_use]
pub fn cooldown_for(failure: &UpstreamFailure, policy_cooldown_seconds: i64) -> i64 {
    failure
        .retry_after_secs
        .map_or(policy_cooldown_seconds, |s| s as i64)
        .max(0)
}

#[cfg(test)]
mod tests {
    use super::{RotationState, plan_route};
    use sair_domain::protocol::ProtocolFamily;
    use sair_domain::routing::{RouteCandidate, Strategy};
    use sair_persistence::repos::ExcludedCandidate;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn candidate(
        id: &str,
        cred: &str,
        alias_p: i64,
        cred_p: i64,
        family: ProtocolFamily,
    ) -> RouteCandidate {
        RouteCandidate {
            deployment_id: id.to_string(),
            provider_id: "p".to_string(),
            provider_label: "Provider".to_string(),
            model_id: format!("model-{id}"),
            base_url: "https://upstream.local".to_string(),
            allow_loopback: false,
            endpoint_override: None,
            protocol_family: family,
            capabilities: sair_domain::routing::CapabilityMetadata {
                protocol_families: vec![family],
                ..Default::default()
            },
            credential_id: cred.to_string(),
            credential_secret_ref: format!("sair.cred.{cred}"),
            alias_priority: alias_p,
            credential_priority: cred_p,
            credential_weight: 1,
        }
    }

    #[test]
    fn priority_orders_by_alias_then_credential_priority() {
        let rotation = RotationState::new();
        let candidates = vec![
            candidate("d2", "c2", 5, 0, ProtocolFamily::OpenAiChat),
            candidate("d1", "c1", 10, 0, ProtocolFamily::OpenAiChat),
        ];
        let decision = plan_route(
            "a",
            ProtocolFamily::OpenAiChat,
            Strategy::Priority,
            &candidates,
            &[],
            &rotation,
        )
        .unwrap();
        assert_eq!(decision.ordered[0].deployment_id, "d1");
        assert_eq!(decision.ordered[1].deployment_id, "d2");
        assert!(decision.selected.is_some());
    }

    #[test]
    fn protocol_mismatch_is_excluded_with_reason() {
        let rotation = RotationState::new();
        let candidates = vec![candidate(
            "d1",
            "c1",
            10,
            0,
            ProtocolFamily::AnthropicMessages,
        )];
        let err = plan_route(
            "a",
            ProtocolFamily::OpenAiChat,
            Strategy::Priority,
            &candidates,
            &[],
            &rotation,
        )
        .unwrap_err();
        assert!(matches!(err, super::RouteError::NoEligibleDeployment));
    }

    #[test]
    fn exclusion_reasons_surface_for_ineligible_rows() {
        let rotation = RotationState::new();
        let excluded = vec![ExcludedCandidate {
            deployment_id: "d9".to_string(),
            credential_id: "c9".to_string(),
            reason: "credential_quarantined".to_string(),
        }];
        let candidates = vec![candidate("d1", "c1", 10, 0, ProtocolFamily::OpenAiChat)];
        let decision = plan_route(
            "a",
            ProtocolFamily::OpenAiChat,
            Strategy::Priority,
            &candidates,
            &excluded,
            &rotation,
        )
        .unwrap();
        assert_eq!(decision.exclusions.len(), 1);
        assert_eq!(decision.exclusions[0].reason, "credential_quarantined");
    }

    #[test]
    fn alias_not_found_when_snapshot_empty() {
        let rotation = RotationState::new();
        let err = plan_route(
            "ghost",
            ProtocolFamily::OpenAiChat,
            Strategy::Priority,
            &[],
            &[],
            &rotation,
        )
        .unwrap_err();
        assert!(matches!(err, super::RouteError::AliasNotFound));
    }

    #[test]
    fn unimplemented_strategies_are_rejected_structurally() {
        let rotation = RotationState::new();
        let candidates = vec![candidate("d1", "c1", 1, 0, ProtocolFamily::OpenAiChat)];
        for strategy in [Strategy::Lru, Strategy::ManualPin] {
            let err = plan_route(
                "a",
                ProtocolFamily::OpenAiChat,
                strategy,
                &candidates,
                &[],
                &rotation,
            )
            .unwrap_err();
            assert!(matches!(err, super::RouteError::UnimplementedStrategy(_)));
        }
    }

    /// Deterministic concurrent rotation test brought forward per review
    /// finding R3-04 (ADR-012): 20 concurrent routings must distribute
    /// round-robin strictly across 2 candidates.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_round_robin_distributes_strictly() {
        let rotation = Arc::new(RotationState::new());
        let candidates = Arc::new(vec![
            candidate("d1", "c1", 10, 0, ProtocolFamily::OpenAiChat),
            candidate("d2", "c2", 10, 0, ProtocolFamily::OpenAiChat),
        ]);
        let mut handles = Vec::new();
        for _ in 0..20 {
            let rotation = rotation.clone();
            let candidates = candidates.clone();
            handles.push(tokio::spawn(async move {
                plan_route(
                    "shared-alias",
                    ProtocolFamily::OpenAiChat,
                    Strategy::RoundRobin,
                    &candidates,
                    &[],
                    &rotation,
                )
                .unwrap()
                .selected
                .unwrap()
                .deployment_id
            }));
        }
        let mut counts: HashMap<String, usize> = HashMap::new();
        for handle in handles {
            let id = handle.await.unwrap();
            *counts.entry(id).or_default() += 1;
        }
        assert_eq!(counts.get("d1"), Some(&10));
        assert_eq!(counts.get("d2"), Some(&10));
    }

    #[test]
    fn weighted_shuffle_keeps_all_candidates() {
        let rotation = RotationState::new();
        let candidates = vec![
            candidate("d1", "c1", 10, 0, ProtocolFamily::OpenAiChat),
            candidate("d2", "c2", 10, 0, ProtocolFamily::OpenAiChat),
            candidate("d3", "c3", 10, 0, ProtocolFamily::OpenAiChat),
        ];
        let decision = plan_route(
            "w",
            ProtocolFamily::OpenAiChat,
            Strategy::Weighted,
            &candidates,
            &[],
            &rotation,
        )
        .unwrap();
        assert_eq!(decision.ordered.len(), 3);
        let ids: std::collections::HashSet<&str> = decision
            .ordered
            .iter()
            .map(|c| c.deployment_id.as_str())
            .collect();
        assert_eq!(ids.len(), 3);
    }
}
