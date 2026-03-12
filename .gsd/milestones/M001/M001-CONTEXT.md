# M001: Storage Management — Full Implementation

## Goal
Implement comprehensive storage management in the Settings page, covering storage usage visibility, retention policy configuration, and manual cleanup operations.

## Current State
- **Frontend**: `StorageSection.tsx` is a placeholder with no functionality
- **Backend**: `RetentionPolicy` model is fully defined in `uc-core` (ByAge, ByCount, ByTotalSize, ByContentType, Sensitive) with defaults (30 days, 500 items)
- **Backend**: `SettingContext` already has `updateRetentionPolicy` method
- **Backend**: No `get_storage_stats` command exists
- **Backend**: No `clear_all_history` command exists
- **Backend**: `SpoolJanitor` exists for spool cleanup but no general retention executor
- **Backend**: `DeleteClipboardEntry` use case exists for single entry deletion
- **Backend**: `get_clipboard_stats` command exists (total_items, total_size)
- **Frontend types**: `RetentionPolicy`, `RetentionRule` types exist in `src/types/setting.ts`
- **i18n**: Storage section keys exist in both en-US and zh-CN

## Architecture Constraints
- Hexagonal: `uc-core` → ports, `uc-infra`/`uc-platform` → adapters
- All Tauri commands require `_trace: Option<TraceMetadata>` 
- Settings are persisted via `update_settings` command (full Settings object)
- Frontend uses `useSetting` hook + `SettingContext`

## Data Paths
- DB: `{app_data_root}/uniclipboard.db`
- Blob vault: `{app_data_root}/vault/`
- Cache: `{app_cache_root}/`
- Logs: `{app_data_root}/logs/`
- Settings: `{app_data_root}/settings.json`
