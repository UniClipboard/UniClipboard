# S01 Summary: Retention Policy Settings UI

---
status: done
---

## What was done
1. Fixed `RetentionRule` TypeScript type to match Rust's externally-tagged serde format (`{ by_age: { max_age: ... } }` instead of `{ tag: 'by_age', max_age: ... }`)
2. Implemented `StorageSection.tsx` with four functional controls:
   - Auto-clear toggle (maps to `retention_policy.enabled`)
   - History retention period selector (7/14/30/60/90/180/365 days, maps to `ByAge` rule)
   - Max history items selector (100/200/500/1000/2000/5000, maps to `ByCount` rule)
   - Skip pinned items toggle (maps to `retention_policy.skip_pinned`)
3. Updated i18n keys in both `en-US.json` and `zh-CN.json` with comprehensive storage section translations
4. No backend changes were needed — all existing `RetentionPolicy` infrastructure was sufficient

## Files changed
- `src/types/setting.ts` — fixed `RetentionRule` type definition
- `src/components/setting/StorageSection.tsx` — full rewrite from placeholder
- `src/i18n/locales/en-US.json` — storage section i18n keys
- `src/i18n/locales/zh-CN.json` — storage section i18n keys
