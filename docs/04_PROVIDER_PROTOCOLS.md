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

## Verified v1 protocol notes (implemented; 2026-10-31 audit re-check)

The sections below record what the v1 adapters actually do — previously this
document stopped at the roadmap and the verified facts lived only in code.

- **OpenAI-compatible + OpenAI**: `POST {base}/chat/completions` with
  `Authorization: Bearer <key>`; `{base}` must include the `/v1` segment
  (e.g. `https://api.openai.com/v1`). `/v1/models` is served locally from the
  alias DB, not proxied. SSE on `stream:true`; `data:` frames relayed
  byte-faithful; `[DONE]` handled by the client.
- **OpenRouter**: identical wire to OpenAI plus opt-in attribution headers
  `HTTP-Referer` (http/https) and `X-Title` (≤128 chars), sent only when the
  provider type is `openrouter`.
- **Anthropic**: `POST {base}/v1/messages` and `/v1/messages/count_tokens`
  with `x-api-key` + `anthropic-version` (client header forwarded when
  header-safe, else pinned `2023-06-01`). `stream` stripped from count_tokens.
- **Gemini**: `POST {base}/v1beta/models/{model}:generateContent` and
  `:streamGenerateContent?alt=sse` with `x-goog-api-key` (header form; `?key=`
  query auth is rejected by the gateway). Model id is percent-encoded into the
  path segment.
- **Errors / cooldowns**: `429` + `insufficient_quota` → BillingOrQuota
  (no rotation — shared-account quota truth); plain `429` → RateLimited with
  cooldown. `Retry-After` accepted as seconds OR HTTP-date (capped 3600 s);
  Gemini `RetryInfo.retryDelay` ("5s", "1.5s") also honored. Ambiguous
  timeouts never rotate (ADR-017).

## NVIDIA NIM (supported via the OpenAI-compatible surface)

NVIDIA NIM exposes an OpenAI-compatible API, so it works through the generic
`openai_compatible` provider type with no code changes:

- **Hosted (build.nvidia.com)**: base URL
  `https://integrate.api.nvidia.com/v1` (the `/v1` segment is required by the
  adapter's fixed path join), API key from NVIDIA build (an `nvapi-…` string),
  credential auth = Bearer. Protocol family `openai_chat`.
- **Self-hosted NIM container** (e.g. `nvcr.io/nim/meta/llama-3.1-8b-instruct`):
  base URL `http://<host>:8000/v1` with any non-empty key; enable "Allow
  loopback HTTP" for `localhost`/`127.0.0.1` targets (SSRF policy).
- Streaming (`streamGenerateContent`-free, plain OpenAI SSE) and non-streaming
  calls route like any OpenAI-compatible deployment; usage tokens are
  extracted from the standard `usage` block.

## Cloud backup table (Phase 12, ADR-019)

Run once in your Neon/Supabase SQL editor:

```sql
create table if not exists sair_backups (
  id          text primary key,
  device_id   text not null,
  app_version text not null,
  mode        text not null check (mode in ('encrypted','config')),
  created_at  timestamptz not null default now(),
  payload     text not null,
  sha256      text not null
);
create index if not exists sair_backups_device_idx on sair_backups (device_id, created_at desc);
```
