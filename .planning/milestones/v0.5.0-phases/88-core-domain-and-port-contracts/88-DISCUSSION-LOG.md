# Phase 88: Core Domain and Port Contracts - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions captured in CONTEXT.md — this log preserves the analysis.

**Date:** 2026-04-10
**Phase:** 88-core-domain-and-port-contracts
**Mode:** discuss
**Areas analyzed:** SearchResult metadata, Error model, Rebuild status domain model

## Assumptions Presented

### SearchResult Metadata

| Option | Description |
|--------|-------------|
| a) Minimal | entry_id + file_type + active_time_ms only; frontend calls clipboard detail API for preview |
| b) With inline preview | + text_preview: Option<String> (80 chars) |
| c) Full metadata | + mime_type + file_extensions in addition to b |

### Error Model

| Option | Description |
|--------|-------------|
| a) SearchError enum | Typed domain enum in uc-core, Result<T, SearchError> at port boundary |
| b) anyhow only | anyhow::Result<T>, runtime error strings |
| c) anyhow + daemon mapping | Port uses anyhow, daemon creates its own HTTP error types |

### Rebuild Status Model

| Option | Description |
|--------|-------------|
| a) RebuildStatus enum | Domain enum in uc-core; port returns/exposes it |
| b) Progress channel | Port accepts Sender<RebuildProgress>; daemon subscribes and forwards WS events |
| c) Pure daemon concern | Port returns Result<RebuildStats>; daemon defines its own notification mechanism |

## Corrections Made

### SearchResult Metadata
- **Chosen:** c) Full metadata — entry_id + file_type + active_time_ms + text_preview + mime_type + file_extensions
- **Reason:** Dashboard filter controls can use mime_type and file_extensions directly from result without second API call

### Error Model
- **Chosen:** a) uc-core SearchError enum
- **Reason:** Typed variants allow daemon routes to map to correct HTTP status codes (400/423/503) without string parsing

### Rebuild Status Model
- **Chosen:** b) Port accepts Sender<RebuildProgress>
- **Reason:** Consistent with existing file transfer progress pattern; uc-core stays decoupled from WS serialization

## No Corrections (All First-Choice Selected)

All three areas were discussed and all recommended options were confirmed.

## External Research

No external research performed — codebase maps and architecture spec provided sufficient evidence.
