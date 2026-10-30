# Project Charter

## Product
Shahid AI Router (working title).

## Vision
A local-first, cross-platform desktop application and local HTTP gateway that manages multiple AI providers, multiple credentials per provider, model deployments and routing policies. Compatible AI coding agents and API clients connect to one stable local endpoint while the gateway selects eligible upstream deployments.

## Core problem
Users manually switch API keys when rate limits, quota errors or provider outages occur. Different AI tools also expect different API protocols and configuration conventions.

## Goals
- Manage multiple providers and multiple credentials per provider.
- Provide a local authenticated gateway endpoint.
- Route requests through capability-aware model aliases and deployment groups.
- Support safe credential rotation, bounded retries and failover.
- Preserve protocol-specific behavior and streaming where supported.
- Store secrets in native OS secure storage.
- Export/import portable configuration and optional encrypted full backups.
- Provide verified setup profiles for AI coding agents.
- Run on macOS and Windows.
- Provide diagnostics without leaking secrets or prompt content.

## Non-goals for v1
- Hosting or training models.
- Circumventing provider quotas, billing limits, account restrictions or terms.
- Claiming multiple keys create additional quota.
- Cloud sync or mandatory user accounts.
- Arbitrary third-party plugin execution.
- Automatically modifying external agent configuration without user consent.

## Success criteria
A user can configure providers and credentials once, create a virtual model alias, point a supported agent to the local gateway, and receive transparent routing/failure diagnostics. Importing a backup on another machine restores configuration and re-stores secrets in that machine's OS credential vault.

## Principles
Security by default; local-first; explicit routing decisions; provider-aware protocols; recoverable state; small verified increments; no false completion claims.

## Assumptions and constraints
Provider APIs and model capabilities change. Some providers do not expose authoritative quota data. Multiple keys may share account-level quotas. Internet is needed for upstream inference, not for local dashboard operation. The initial target is a single-user desktop application.
