# Provider & Protocol Specification

## Initial provider candidates
OpenAI-compatible custom endpoints, OpenRouter, OpenAI, Anthropic and Google Gemini. Later candidates may include DeepSeek, NVIDIA, Groq, Together AI and Fireworks AI after official API and terms review.

This is a roadmap, not a claim of existing implementation.

## Adapter contract
Every adapter declares provider ID/version, supported protocols and operations, auth scheme, model ID mapping, streaming, tool calling, vision/multimodal, structured output, reasoning features, context/output limits when known, usage reporting, error classification and unsupported parameters.

## Error categories
InvalidCredential, PermissionDenied, BillingOrQuota, RateLimited, ModelUnavailable, InvalidRequest, ContextLimit, Timeout, ProviderUnavailable, TransportFailure and Unknown. Preserve upstream status/code and safe diagnostics.

Do not assume every HTTP 429 means quota exhaustion. Distinguish transient rate limiting from billing/account quota when provider signals allow it.

## Provider onboarding
Research official docs; verify terms; build capability matrix; implement auth and URL validation; request/response mapping; streaming; tool/multimodal behavior; error classification; mock tests; opt-in live tests; security review; document limitations.
