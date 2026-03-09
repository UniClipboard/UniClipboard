---
phase: 15-clipboard-management-command-wiring
plan: '02'
status: completed
completed_at: '2026-03-07'
summary: Frontend clipboard stats and favorites APIs wired to uc-tauri commands with Vitest coverage for CONTRACT-03.
---

## What Was Built

- Wired existing frontend clipboard management APIs to new uc-tauri clipboard commands:
  - `getClipboardStats` now uses `invokeWithTrace<ClipboardStats>('get_clipboard_stats')` and returns `{ total_items, total_size }` directly.
  - `favoriteClipboardItem` / `unfavoriteClipboardItem` call `toggle_favorite_clipboard_item` with `{ id, is_favorited: true|false }`, matching the Rust command signature.
  - `getClipboardItem` continues to call `get_clipboard_item` with `{ id, fullContent }` as the payload, matching the planned command parameters.

- Extended Vitest coverage for clipboard management contract:
  - Added tests in `src/api/__tests__/clipboardItems.test.ts` to assert:
    - `getClipboardStats` uses `get_clipboard_stats` with an empty payload and returns the resolved stats unchanged.
    - `favoriteClipboardItem` and `unfavoriteClipboardItem` call `toggle_favorite_clipboard_item` with the expected `id` and `is_favorited` flags.
    - `getClipboardItem` calls `get_clipboard_item` with `{ id, fullContent }` and returns the backend response.

## Files Touched

- `src/api/clipboardItems.ts`
  - Confirmed `ClipboardStats` type matches backend DTO (`total_items`, `total_size`).
  - Updated favorites helpers to send `is_favorited` (snake_case) to align with the Rust command signature.

- `src/api/__tests__/clipboardItems.test.ts`
  - Added stats/favorites/item tests to lock down command names and payload shapes for CONTRACT-03.

## Commands Run

- `bun test src/api/__tests__/clipboardItems.test.ts` — all clipboardItems API tests passing.

## Notes

- Backend `toggle_favorite_clipboard_item` currently returns `CommandError::NotFound` as a placeholder while favorite domain support is pending; the frontend now has explicit error-path tests and can handle this contract without assuming success.
