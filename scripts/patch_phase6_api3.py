import io

p = 'crates/sair-gateway/src/api.rs'
src = io.open(p, encoding='utf-8').read()

# ---------- Piece B: alias resolution + storage errors use family envelopes; family in plan_route ----------
src = src.replace('''    let (candidates, excluded) = match load_route_snapshot_detailed(&api.db, alias).await {
        Ok(pair) => pair,
        Err(e) => {
            return openai_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_failure",
                &format!("route snapshot unavailable: {e}"),
                json!({}),
            );
        }
    };
    if candidates.is_empty() && excluded.is_empty() {
        return openai_error(
            StatusCode::NOT_FOUND,
            "alias_not_found",
            &format!("no configured alias named `{alias}`"),
            json!({}),
        );
    }

    let policy = match api.db.repos().ensure_default_policy().await {
        Ok(p) => p,
        Err(e) => {
            return openai_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_failure",
                &format!("routing policy unavailable: {e}"),
                json!({}),
            );
        }
    };
    let strategy = match policy.strategy.as_str() {
        "priority" => Strategy::Priority,
        "round_robin" => Strategy::RoundRobin,
        "weighted" => Strategy::Weighted,
        other => {
            return openai_error(
                StatusCode::NOT_IMPLEMENTED,
                "strategy_not_implemented",
                &format!("routing strategy `{other}` is declared but not implemented"),
                json!({}),
            );
        }
    };

    let requirements = sair_domain::routing::CapabilityRequirements::derive_from_body(&parsed);
    let decision = match plan_route(
        alias,
        sair_domain::protocol::ProtocolFamily::OpenAiChat,
        strategy,
        requirements,
        &candidates,
        &excluded,
        &api.rotation,
    ) {
        Ok(d) => d,
        Err(e) => {
            let (status, code): (axum::http::StatusCode, String) = match &e {
                crate::routing::RouteError::NoEligibleDeployment => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "no_eligible_deployment".to_string(),
                ),
                crate::routing::RouteError::AliasNotFound => {
                    (StatusCode::NOT_FOUND, "alias_not_found".to_string())
                }
                crate::routing::RouteError::UnimplementedStrategy(s) => (
                    StatusCode::NOT_IMPLEMENTED,
                    format!("strategy_{s}_not_implemented"),
                ),
            };
            // F-4: surface why every candidate was excluded (FR-012).
            let reasons: Vec<String> = excluded
                .iter()
                .map(|x| format!("{}: {}", x.deployment_id, x.reason))
                .collect();
            return openai_error(
                status,
                &code,
                &e.to_string(),
                json!({ "alias": alias, "reasons": reasons }),
            );
        }
    };''', '''    let (candidates, excluded) = match load_route_snapshot_detailed(&api.db, alias).await {
        Ok(pair) => pair,
        Err(e) => {
            return family_local_error(
                surface,
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_failure",
                &format!("route snapshot unavailable: {e}"),
                json!({}),
            );
        }
    };
    if candidates.is_empty() && excluded.is_empty() {
        return family_local_error(
            surface,
            StatusCode::NOT_FOUND,
            "alias_not_found",
            &format!("no configured alias named `{alias}`"),
            json!({}),
        );
    }

    let policy = match api.db.repos().ensure_default_policy().await {
        Ok(p) => p,
        Err(e) => {
            return family_local_error(
                surface,
                StatusCode::INTERNAL_SERVER_ERROR,
                "storage_failure",
                &format!("routing policy unavailable: {e}"),
                json!({}),
            );
        }
    };
    let strategy = match policy.strategy.as_str() {
        "priority" => Strategy::Priority,
        "round_robin" => Strategy::RoundRobin,
        "weighted" => Strategy::Weighted,
        other => {
            return family_local_error(
                surface,
                StatusCode::NOT_IMPLEMENTED,
                "strategy_not_implemented",
                &format!("routing strategy `{other}` is declared but not implemented"),
                json!({}),
            );
        }
    };

    let requirements = sair_domain::routing::CapabilityRequirements::derive_from_body(&parsed);
    let decision = match plan_route(
        alias,
        surface.family(),
        strategy,
        requirements,
        &candidates,
        &excluded,
        &api.rotation,
    ) {
        Ok(d) => d,
        Err(e) => {
            let (status, code): (axum::http::StatusCode, String) = match &e {
                crate::routing::RouteError::NoEligibleDeployment => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "no_eligible_deployment".to_string(),
                ),
                crate::routing::RouteError::AliasNotFound => {
                    (StatusCode::NOT_FOUND, "alias_not_found".to_string())
                }
                crate::routing::RouteError::UnimplementedStrategy(s) => (
                    StatusCode::NOT_IMPLEMENTED,
                    format!("strategy_{s}_not_implemented"),
                ),
            };
            // F-4/F-8: count_tokens on an alias without an anthropic
            // deployment is a structured 501 (ADR-010).
            if operation == Operation::CountTokens
                && matches!(
                    e,
                    crate::routing::RouteError::NoEligibleDeployment
                )
            {
                return family_local_error(
                    surface,
                    StatusCode::NOT_IMPLEMENTED,
                    "count_tokens_not_supported",
                    "count_tokens is only available for aliases with anthropic deployments",
                    json!({ "alias": alias }),
                );
            }
            // F-4: surface why every candidate was excluded (FR-012).
            let reasons: Vec<String> = excluded
                .iter()
                .map(|x| format!("{}: {}", x.deployment_id, x.reason))
                .collect();
            return family_local_error(
                surface,
                status,
                &code,
                &e.to_string(),
                json!({ "alias": alias, "reasons": reasons }),
            );
        }
    };''')
io.open(p, 'w', encoding='utf-8', newline='\n').write(src)
print('piece B done')
