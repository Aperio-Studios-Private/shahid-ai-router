# System Architecture

## Style
Use a modular monolith for v1, not premature microservices. Keep clean boundaries between desktop UI, application core, gateway, router, adapters, credential vault, persistence, telemetry and backup service.

## Recommended flow
Client -> loopback gateway -> local auth -> protocol parser -> model/alias resolution -> capability filtering -> routing policy -> secure credential retrieval -> provider adapter -> upstream API -> response/stream mapping -> redacted telemetry -> client.

## Components
1. Tauri desktop shell.
2. React/TypeScript frontend.
3. Rust application/domain services.
4. Axum local HTTP gateway.
5. Routing and retry engine.
6. Provider adapters.
7. OS credential vault abstraction.
8. SQLite persistence.
9. Structured redacted logging.
10. Backup/restore subsystem.

## Protocol design
Do not force every provider into one lowest-common-denominator schema. Keep protocol-specific request/response types and shared routing metadata. Each adapter declares supported operations and capabilities.

## Concurrency
Use Tokio. Do not hold database locks or transactions across network awaits. Bound request sizes, concurrency, queues and timeouts. Protect rotation/cooldown state from races.

## Failure isolation
Provider failures must not crash the gateway. UI failures must not corrupt gateway state. Migration failures must stop safely. Adapter failures must be contained and observable.

## Security boundaries
Loopback bind by default; local gateway token; strict Tauri capabilities; no secrets in frontend storage; SSRF protections for custom endpoints; validate headers and redirects.
