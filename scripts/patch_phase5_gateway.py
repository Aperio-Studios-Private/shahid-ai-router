import io

# ---- routing.rs: requirements param + capability filter (F-6 order) ----
p = 'crates/sair-gateway/src/routing.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''use sair_domain::routing::{Exclusion, RouteCandidate, RoutingDecision, Strategy};''', '''use sair_domain::routing::{CapabilityRequirements, Exclusion, RouteCandidate, RoutingDecision, Strategy};''')
src = src.replace('''pub fn plan_route(
    alias: &str,
    family: ProtocolFamily,
    strategy: Strategy,
    candidates: &[RouteCandidate],
    excluded: &[ExcludedCandidate],
    rotation: &RotationState,
) -> Result<RoutingDecision, RouteError> {''', '''pub fn plan_route(
    alias: &str,
    family: ProtocolFamily,
    strategy: Strategy,
    requirements: CapabilityRequirements,
    candidates: &[RouteCandidate],
    excluded: &[ExcludedCandidate],
    rotation: &RotationState,
) -> Result<RoutingDecision, RouteError> {''')
src = src.replace('''    let eligible: Vec<RouteCandidate> = candidates
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
        .collect();''', '''    let eligible: Vec<RouteCandidate> = candidates
        .iter()
        .filter(|c| {
            // F-6: protocol filter first, capability filter second; one
            // reason per candidate (first failing check).
            let reason = if !c.capabilities.supports_protocol(family) {
                Some("protocol_mismatch")
            } else {
                requirements.first_unmet(&c.capabilities)
            };
            if let Some(reason) = reason {
                exclusions.push(Exclusion {
                    deployment_id: c.deployment_id.clone(),
                    credential_id: c.credential_id.clone(),
                    reason: reason.to_string(),
                });
                false
            } else {
                true
            }
        })
        .cloned()
        .collect();''')
# tests: add requirements arg (default) to all plan_route calls in tests
src = src.replace('''            Strategy::Priority,
            &candidates,''', '''            Strategy::Priority,
            sair_domain::routing::CapabilityRequirements::default(),
            &candidates,''')
src = src.replace('''            Strategy::RoundRobin,
            &candidates,''', '''            Strategy::RoundRobin,
            sair_domain::routing::CapabilityRequirements::default(),
            &candidates,''')
src = src.replace('''            Strategy::Weighted,
            &candidates,''', '''            Strategy::Weighted,
            sair_domain::routing::CapabilityRequirements::default(),
            &candidates,''')
# provider_type in test constructor
src = src.replace('''            credential_account_id: None,''', '''            credential_account_id: None,
            provider_type: "openai_compatible".to_string(),''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('routing patched')

# ---- api.rs: requirements from body, dispatch per provider_type, exclusions in 503 ----
p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()
src = src.replace('''    let decision = match plan_route(
        alias,
        sair_domain::protocol::ProtocolFamily::OpenAiChat,
        strategy,
        &candidates,
        &excluded,
        &api.rotation,
    ) {''', '''    let requirements = sair_domain::routing::CapabilityRequirements::derive_from_body(&parsed);
    let decision = match plan_route(
        alias,
        sair_domain::protocol::ProtocolFamily::OpenAiChat,
        strategy,
        requirements,
        &candidates,
        &excluded,
        &api.rotation,
    ) {''')
src = src.replace('''                crate::routing::RouteError::NoEligibleDeployment => {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "no_eligible_deployment".to_string(),
                    )
                }''', '''                crate::routing::RouteError::NoEligibleDeployment => {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "no_eligible_deployment".to_string(),
                    )
                }
                _ => unreachable!("all RouteError variants covered"),''')
# remove the unreachable arm approach: instead keep as-is (match is exhaustive already)
src = src.replace('''                _ => unreachable!("all RouteError variants covered"),\n''', '')
# dispatch per candidate provider_type (F-1) + attribution config
src = src.replace('''        let client = if candidate.allow_loopback {
            &api.client_loopback
        } else {
            &api.client_strict
        };
        let base = candidate
            .endpoint_override
            .clone()
            .unwrap_or_else(|| candidate.base_url.clone());

        match send_chat(
            client,
            &base,
            &candidate.model_id,
            &secret,
            parsed.clone(),
            Duration::from_millis(policy.timeout_ms.max(1) as u64),
        )
        .await''', '''        let client = if candidate.allow_loopback {
            &api.client_loopback
        } else {
            &api.client_strict
        };
        let base = candidate
            .endpoint_override
            .clone()
            .unwrap_or_else(|| candidate.base_url.clone());

        // F-1: attribution headers are OpenRouter-only, per candidate
        // dispatch. Never leaked to other providers.
        let send_result = if candidate.provider_type == "openrouter" {
            let extra = crate::api::attribution_for(&candidate.provider_id, &api);
            send_chat_with_headers(
                client,
                &base,
                &candidate.model_id,
                &secret,
                parsed.clone(),
                Duration::from_millis(policy.timeout_ms.max(1) as u64),
                &extra,
            )
            .await
        } else {
            send_chat(
                client,
                &base,
                &candidate.model_id,
                &secret,
                parsed.clone(),
                Duration::from_millis(policy.timeout_ms.max(1) as u64),
            )
            .await
        };
        match send_result''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('api patched')
