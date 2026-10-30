# Routing, Rotation & Failover

## Definitions
Provider = upstream service. Credential = API key/token. Deployment = provider + model + endpoint + eligible credential. Alias = stable local model name. Rotation = choosing another eligible credential/deployment. Failover = moving after a classified failure.

## Selection algorithm
1. Parse protocol, operation and requested model.
2. Resolve alias.
3. Determine required capabilities.
4. Exclude disabled, invalid, incompatible and cooling-down deployments.
5. Apply allow/deny rules and priority.
6. Apply configured strategy.
7. Retrieve secret securely.
8. Send with bounded timeout and retry budget.
9. Classify result and update health/cooldown.
10. Return response or structured error.

## Strategies
Priority, round-robin, weighted, least-recently-used, latency-aware (with safeguards), and manual pin. Cost-aware routing only when current pricing and capability equivalence are verified.

## Retry safety
Respect Retry-After. Use bounded exponential backoff with jitter. Avoid retry storms. Do not retry invalid requests, unsupported models or context-limit errors blindly. Authentication failure may quarantine one credential. Account-level quota failure must not blindly rotate keys from the same shared account.

## Streaming
After response events/bytes are committed, transparent failover is generally unsafe. Retry only before commitment or with explicit protocol/client resumability. Never concatenate unrelated model streams.

## Duplicate execution
Generation requests may incur duplicate costs if retried after ambiguous timeouts. Use provider idempotency support where available; otherwise surface ambiguity instead of blind retry.

## Quota truth
Multiple keys under one account/project may share quota and billing. Never market key rotation as quota multiplication. Model account grouping where known.
