---
phase: quick
plan: 007
type: execute
wave: 1
depends_on: []
files_modified:
  - src/components/device/DeviceSettingsPanel.tsx
  - src/types/setting.ts
  - src/api/p2p.ts
  - src/i18n/locales/en-US.json
  - src/i18n/locales/zh-CN.json
  - src-tauri/crates/uc-core/src/settings/model.rs
  - src-tauri/crates/uc-app/src/usecases/update_settings.rs
autonomous: true
requirements: []
must_haves:
  truths:
    - 'Device sync settings panel only shows text, image, file, rich_text toggles'
    - 'Sync links and sync code snippets options are removed from UI'
  artifacts:
    - path: 'src/components/device/DeviceSettingsPanel.tsx'
      provides: 'UI component for device sync settings'
    - path: 'src/types/setting.ts'
      provides: 'TypeScript ContentTypes interface'
    - path: 'src/api/p2p.ts'
      provides: 'API ContentTypes interface'
    - path: 'src-tauri/crates/uc-core/src/settings/model.rs'
      provides: 'Rust ContentTypes struct'
  key_links:
    - from: 'DeviceSettingsPanel.tsx'
      to: 'ContentTypes'
      via: 'contentTypeEntries array'
---

<objective>
Remove "Sync Links" and "Sync Code Snippets" content type toggle options from the device sync settings panel, and clean up related TypeScript types and Rust backend fields.

Purpose: Simplify sync settings UI by removing unsupported content type sync options.
Output: Clean sync settings with only text, image, file, and rich_text options.
</objective>

<execution_context>
@/Users/mark/.claude/get-shit-done/workflows/execute-plan.md
@/Users/mark/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@src/components/device/DeviceSettingsPanel.tsx (lines 16-23: contentTypeEntries array with all content types)
@src/types/setting.ts (lines 33-40: ContentTypes interface)
@src/api/p2p.ts (lines 170-190: ContentTypes interface for device sync)
</context>

<tasks>

<task type="auto">
  <name>Task 1: Remove link and code_snippet from DeviceSettingsPanel contentTypeEntries</name>
  <files>src/components/device/DeviceSettingsPanel.tsx</files>
  <action>
Remove the following entries from the contentTypeEntries array (lines 16-23):
- { field: 'link', i18nKey: 'syncLink' }
- { field: 'code_snippet', i18nKey: 'syncCodeSnippet' }

Keep only: text, image, file, rich_text

This removes the UI toggles for sync links and sync code snippets.
</action>
<verify>
<automated>grep -n "link\|code_snippet" src/components/device/DeviceSettingsPanel.tsx || echo "No link/code_snippet references found"</automated>
</verify>
<done>DeviceSettingsPanel no longer renders link and code_snippet sync toggles</done>
</task>

<task type="auto">
  <name>Task 2: Update TypeScript ContentTypes interfaces</name>
  <files>src/types/setting.ts, src/api/p2p.ts</files>
  <action>
In src/types/setting.ts (lines 33-40), remove from ContentTypes interface:
- link: boolean
- code_snippet: boolean

In src/api/p2p.ts (lines 170-177), remove from ContentTypes interface:

- link: boolean
- code_snippet: boolean

Keep only: text, image, file, rich_text
</action>
<verify>
<automated>grep -n "link\|code_snippet" src/types/setting.ts | head -5</automated>
</verify>
<done>ContentTypes interfaces updated to exclude link and code_snippet</done>
</task>

<task type="auto">
  <name>Task 3: Remove i18n translation keys for syncLink and syncCodeSnippet</name>
  <files>src/i18n/locales/en-US.json, src/i18n/locales/zh-CN.json</files>
  <action>
In src/i18n/locales/en-US.json (around lines 413-420), remove:
- "syncLink": { "title": "Sync Links", "description": "Allow syncing URL/link content" }
- "syncCodeSnippet": { "title": "Sync Code Snippets", "description": "Allow syncing code snippet content" }

In src/i18n/locales/zh-CN.json (around lines 413-420), remove:

- "syncLink": { "title": "同步链接", "description": "允许同步 URL/链接内容" }
- "syncCodeSnippet": { "title": "同步代码片段", "description": "允许同步代码片段内容" }
  </action>
  <verify>
  <automated>grep -n "syncLink\|syncCodeSnippet" src/i18n/locales/en-US.json src/i18n/locales/zh-CN.json</automated>
  </verify>
  <done>i18n files no longer contain syncLink and syncCodeSnippet keys</done>
  </task>

<task type="auto">
  <name>Task 4: Update Rust ContentTypes struct and update_settings use case</name>
  <files>src-tauri/crates/uc-core/src/settings/model.rs, src-tauri/crates/uc-app/src/usecases/update_settings.rs</files>
  <action>
In src-tauri/crates/uc-core/src/settings/model.rs (lines 41-48), modify ContentTypes struct:
- Remove: pub link: bool
- Remove: pub code_snippet: bool
- Add #[serde(default)] to struct to maintain backward compatibility with existing stored settings

Keep only: text, image, file, rich_text

In src-tauri/crates/uc-app/src/usecases/update_settings.rs (lines 231-236), remove from content_types comparison:

- || old.content_types.link != new.content_types.link
- || old.content_types.code_snippet != new.content_types.code_snippet
  </action>
  <verify>
  <automated>cd src-tauri && cargo check 2>&1 | head -20</automated>
  </verify>
  <done>Rust ContentTypes struct updated, cargo check passes</done>
  </task>

</tasks>

<verification>
1. DeviceSettingsPanel only shows text, image, file, rich_text toggles
2. TypeScript types exclude link and code_snippet
3. i18n keys removed
4. Rust backend compiles without errors
</verification>

<success_criteria>

- Device sync settings panel shows only 4 content type toggles (text, image, file, rich_text)
- Sync links and sync code snippets toggles are no longer visible
- TypeScript and Rust type definitions are consistent
- Application builds successfully
  </success_criteria>

<output>
After completion, create .planning/quick/007-remove-sync-links-and-sync-code-snippets/007-SUMMARY.md
</output>
