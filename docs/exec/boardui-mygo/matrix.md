# BoardUI item and capability matrix

Research snapshot: 2026-11-20. See [research report](../boardui-mygo-research.md) for source identities, licensing, capabilities, E2E state expansion, architecture and delivery gates. This is not an implementation acceptance record.

`free-pinned` = official MIT export at `ec5c5eb3e29c2441be63b9422cd54f411ef23466`. `free-skill-only` = metadata in skill 2026.11.10; live source unavailable. `pro` / `pro-template` = metadata only; source not requested. Each row is one install item, not a variant. Literal domains and direct CSS states are in [catalogue.json](catalogue.json). Inherited framework states require the capability matrix and E2E expansion in the report.

Difficulty is relative: L paint/layout; M compound state; H semantic or renderer gap; VH multi-component application/graphics. It is not a time estimate. Each row inherits its group capability risks below.

## Foundation

Primitives: `ui.Theme, RegisterFont, Direction, Animation`. Existing Kit: reuse token-generation location; do not reuse three-skin semantic names. Gap: CSS AST resolution, locale propagation, legal asset provenance. Native/accessibility risk: font fallback, RTL and theme invalidation.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `agent-runtime` | free-pinned; [app/api/chat/route.ts](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/app/api/chat/route.ts) | KnownProviderId=anthropic, openrouter, openai, google, gateway, groq, xai, mistral, deepseek | @ai-sdk/anthropic, @ai-sdk/google, @ai-sdk/openai, ai | N/A |
| `chevrons` | free-pinned; [components/foundations/icons/chevrons.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/foundations/icons/chevrons.tsx) | No local literal domain; composition/inherited API | react | N/A |
| `cx` | free-pinned; [utils/cx.ts](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/utils/cx.ts) | No local literal domain; composition/inherited API | tailwind-merge | N/A |
| `direction` | free-pinned; [components/foundations/direction/direction.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/foundations/direction/direction.tsx) | Direction=ltr, rtl | react, react-aria-components | N/A |
| `directional-icon` | free-pinned; [utils/directional-icon.ts](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/utils/directional-icon.ts) | No local literal domain; composition/inherited API | @remixicon/react | N/A |
| `globals` | free-pinned; [styles/globals.css](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/styles/globals.css) | CSS: hover, focus-within | tailwindcss | N/A |
| `logo` | free-pinned; [components/foundations/brand/logo.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/foundations/brand/logo.tsx) | props: mono, decorative, priority | next | N/A |
| `rules` | free-pinned; [AGENTS.md](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/AGENTS.md) | No local literal domain; composition/inherited API | See source import graph in JSON | N/A |
| `scroll-direction` | free-pinned; [utils/scroll-direction.ts](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/utils/scroll-direction.ts) | No local literal domain; composition/inherited API | See source import graph in JSON | N/A |
| `template-copy` | free-pinned; [components/foundations/template-copy/template-copy.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/foundations/template-copy/template-copy.tsx) | No local literal domain; composition/inherited API | react | N/A |
| `theme` | free-pinned; [styles/theme.css](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/styles/theme.css) | CSS: hover, disabled, active | See source import graph in JSON | N/A |
| `typography` | free-pinned; [styles/typography.css](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/styles/typography.css) | No local literal domain; composition/inherited API | See source import graph in JSON | N/A |
| `use-count-up` | free-pinned; [hooks/use-count-up.ts](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/hooks/use-count-up.ts) | No local literal domain; composition/inherited API | react | N/A |
| `use-dismiss-on-outside-press` | free-pinned; [utils/use-dismiss-on-outside-press.ts](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/utils/use-dismiss-on-outside-press.ts) | No local literal domain; composition/inherited API | react | N/A |

## Button

Primitives: `ButtonBase, FocusVisible, FocusRing, Icon, ParseSVG`. Existing Kit: reuse Base behavior; redraw BoardUI gradients/shape. Gap: link activation, grouped focus, 24 branded asset rights. Native/accessibility risk: accessible icon-only name; disabled activation.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `button` | free-pinned; [components/base/buttons/button.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/buttons/button.tsx) | ButtonVariant=primary, secondary, ghost, danger; ButtonSize=medium, small, xs; props: iconOnly; CSS: focus-visible, disabled, aria-disabled, hover, active | react | L–M |
| `button-group` | free-pinned; [components/base/buttons/button-group.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/buttons/button-group.tsx) | ButtonGroupSize=medium, small; props: selected, iconOnly; CSS: focus-visible, hover, active, disabled | react | L–M |
| `close-button` | free-pinned; [components/base/buttons/close-button.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/buttons/close-button.tsx) | CloseButtonSize=2xs, xs, sm, md; CSS: hover, focus-visible | react | L–M |
| `icon-button` | free-pinned; [components/base/buttons/icon-button.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/buttons/icon-button.tsx) | IconButtonSize=medium, small; CSS: focus-visible, hover, active, disabled | react | L–M |
| `link-button` | free-pinned; [components/base/buttons/link-button.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/buttons/link-button.tsx) | LinkButtonVariant=primary, secondary; LinkButtonSize=medium, small, xs; props: disabled; CSS: hover, focus-visible, disabled, aria-disabled, active | react | L–M |
| `social-button` | free-pinned; [components/base/social-button/social-button.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/social-button/social-button.tsx) | SocialButtonSize=medium, small; SocialButtonAppearance=colorful, black, white; props: iconOnly, fullWidth; CSS: focus-visible, disabled, aria-disabled, hover, active | react | L–M |

## Text

Primitives: `TextInputBase, TextAreaBase, ComboboxBase, NumberInput, TimeInput`. Existing Kit: reuse editors and selection; Kit.TextField geometry differs. Gap: segmented locale editing, auto-grow, OTP distribution, validation relation. Native/accessibility risk: IME commit, grapheme selection, password/autofill and focus.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `auth-card` | free-pinned; [components/application/auth/auth-card.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/auth/auth-card.tsx) | AuthLayout=stacked, inline, grid; AuthMode=signin, signup, verify; props: centered, confirmPassword, headingLevel | @remixicon/react, react | H |
| `combobox` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `input` | free-pinned; [components/base/input/input.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/input/input.tsx) | InputSize=medium, small; props: isRequired, isInvalid, isDisabled, isReadOnly, inputDir; CSS: disabled | react, react-aria-components | H |
| `input-otp` | free-pinned; [components/base/input-otp/input-otp.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/input-otp/input-otp.tsx) | props: isDisabled, isInvalid; CSS: hover, focus-visible | react | H |
| `number-field` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `search-field` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `textarea` | free-pinned; [components/base/textarea/textarea.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/textarea/textarea.tsx) | TextareaResize=none, vertical; props: autoResize, showCount; CSS: disabled | react, react-aria-components | H |
| `time-field` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |

## Choice

Primitives: `CheckboxStateBase, RadioBase, SwitchBase, SliderBase, RangeSlider, TabsBase, ColorPicker`. Existing Kit: reuse behavior; BoardUI shape/tick differs. Gap: multi-thumb focus, custom range paint, mixed state and roving keys. Native/accessibility risk: space/arrows/Home/End; checked and disabled exposure.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `checkbox` | free-pinned; [components/base/checkbox/checkbox.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/checkbox/checkbox.tsx) | CheckboxSize=sm, md | react, react-aria-components | M–H |
| `checkbox-card` | free-pinned; [components/base/checkbox/checkbox-card.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/checkbox/checkbox-card.tsx) | CheckboxSize=sm, md | react, react-aria-components | M–H |
| `color-picker` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | M–H |
| `radio` | free-pinned; [components/base/radio/radio.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/radio/radio.tsx) | RadioSize=sm, md; props: selected, focusVisible | react, react-aria-components | M–H |
| `radio-card` | free-pinned; [components/base/radio/radio-card.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/radio/radio-card.tsx) | RadioSize=sm, md | react, react-aria-components | M–H |
| `segmented-control` | free-pinned; [components/base/segmented-control/segmented-control.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/segmented-control/segmented-control.tsx) | SegmentedControlVariant=solid, plain; CSS: focus-visible, hover | react, react-aria-components | M–H |
| `slider` | free-pinned; [components/base/slider/slider.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/slider/slider.tsx) | props: showTooltip; CSS: active | react, react-aria-components | M–H |
| `switch` | free-pinned; [components/base/switch/switch.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/switch/switch.tsx) | SwitchSize=sm, md, lg; SwitchShape=pill, rectangle; props: isSelected, isDisabled, isFocusVisible | react, react-aria-components | M–H |
| `switch-card` | free-pinned; [components/base/switch/switch-card.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/switch/switch-card.tsx) | SwitchSize=sm, md, lg; SwitchShape=pill, rectangle | react, react-aria-components | M–H |
| `tabs` | free-pinned; [components/base/tabs/tabs.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/tabs/tabs.tsx) | CSS: focus-visible | react, react-aria-components | M–H |
| `tag-group` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | M–H |
| `theme-toggle` | free-pinned; [components/application/theme/theme-toggle.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/theme/theme-toggle.tsx) | ThemeMode=light, dark; props: persist, collapsed, appearance, size; CSS: focus-visible, hover | @remixicon/react, react, react-aria-components, react-dom | M–H |

## Overlay

Primitives: `DialogBase, PopoverBase, MenuBase, TooltipBase, AttachTo, Modal, Transition`. Existing Kit: reuse overlay behavior; current Kit native menus conflict visually. Gap: exit presence, nested focus restore, collision geometry, toast queue. Native/accessibility risk: Escape propagation, outside click, focus trap and live announcements.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `command-palette` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `context-menu` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `drawer` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `dropdown` | free-pinned; [components/base/dropdown/dropdown.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/dropdown/dropdown.tsx) | props: isOpen, selected; CSS: focus-visible | react, react-aria-components | H |
| `modal` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `notification` | free-pinned; [components/base/notification/notification.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/notification/notification.tsx) | NotificationStatus=neutral, information, success, error; NotificationPosition=top-start, top-end, bottom-start, bottom-end, top-left, top-center, top-right, bottom-left, bottom-center, bottom-right; props: presence, dismissible | @remixicon/react, motion, react, react-dom | H |
| `notification-center` | free-pinned; [components/application/notification-center/notification-center.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/notification-center/notification-center.tsx) | NotificationCenterTab=all, mentions, system; NotificationCenterStatus=neutral, information, success, error; props: unread | @remixicon/react, react | H |
| `popover` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `select` | free-pinned; [components/base/select/select.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/select/select.tsx) | SelectSize=sm, md; CSS: hover, focus-visible, disabled | react, react-aria-components | H |
| `settings-modal` | free-pinned; [components/application/settings/settings-modal.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/settings/settings-modal.tsx) | FileKind=document, spreadsheet, video; SortKey=name, uploadedOn, bytes; RowAnimationPhase=idle, entering, exiting; ServerStatus=connected, error; SettingsPage=general, profile, storage, tools, marketplace; props: isOpen; CSS: focus-visible, hover | @remixicon/react, react, react-dom | H |
| `toast` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `tooltip` | free-pinned; [components/base/tooltip/tooltip.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/tooltip/tooltip.tsx) | TooltipSize=sm, md; props: showArrow; CSS: entering, exiting | react, react-aria-components | H |

## Navigation

Primitives: `TreeBase, List, ScrollState, FocusGroup, Collapsible`. Existing Kit: reuse virtual list mechanics; navigation visuals differ. Gap: RTL roving focus, responsive collapse, carousel snap and scroll fade. Native/accessibility risk: tree selection/expansion, touch scrolling, hidden focus targets.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `accordion` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | M–H |
| `app-shell` | free-pinned; [components/application/app-shell/app-shell.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/app-shell/app-shell.tsx) | No local literal domain; composition/inherited API | @remixicon/react, next, react | M–H |
| `breadcrumb` | free-pinned; [components/base/breadcrumb/breadcrumb.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/breadcrumb/breadcrumb.tsx) | props: current; CSS: hover, focus-visible | next, react | M–H |
| `carousel` | free-pinned; [components/base/carousel/carousel.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/carousel/carousel.tsx) | props: showArrows, showDots, align; CSS: focus-visible, hover | @remixicon/react, react | M–H |
| `pagination` | free-pinned; [components/base/pagination/pagination.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/pagination/pagination.tsx) | CSS: focus-visible, hover | @remixicon/react, react | M–H |
| `sidebar` | free-pinned; [components/application/dashboard/dashboard-sidebar.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/dashboard/dashboard-sidebar.tsx) | AvatarColor=neutral, lime, pink; props: collapsed, isSelected, secondaryMatch, mobile, fluid, showThemeToggle, flat; CSS: hover | @remixicon/react, react | M–H |
| `tree-view` | free-pinned; [components/base/tree-view/tree-view.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/tree-view/tree-view.tsx) | props: size, showGuides, expanded, direction; CSS: empty, focus-visible, hover | @remixicon/react, motion, react, react-aria-components | M–H |

## Date

Primitives: `DateInput, Calendar, TimeInput, PopoverBase`. Existing Kit: existing Kit has no equivalent full date/range suite. Gap: two-month range, unavailable dates, locale calendar and timezone. Native/accessibility risk: segment keys, week starts, DST and screen reader date names.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `calendar` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `date-calendar` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |
| `date-picker` | free-pinned; [components/base/date-picker/date-picker.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/date-picker/date-picker.tsx) | props: isDisabled, isOpen | @internationalized/date, @remixicon/react, motion, react, react-aria-components | H |
| `date-range-picker` | free-pinned; [components/base/date-picker/date-range-picker.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/date-picker/date-range-picker.tsx) | props: isDisabled; CSS: hover | @internationalized/date, @remixicon/react, motion, react, react-aria-components | H |
| `meeting-scheduler` | free-pinned; [components/base/date-picker/meeting-scheduler.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/date-picker/meeting-scheduler.tsx) | props: defaultOpen, use24h; CSS: hover | @internationalized/date, @remixicon/react, next, react, react-aria-components | H |
| `most-active-days-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H |

## Table

Primitives: `Table, ListState, TableColumn, SortOrder, ListState.CellSelect`. Existing Kit: Kit.Table is only partial visual reuse. Gap: header styling, stable row IDs, cell edit/TSV/undo, selection after filtering. Native/accessibility risk: virtual row access indices, keyboard range, resize and sorting announcements.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `data-grid` | free-pinned; [components/application/data-grid/data-grid.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/data-grid/data-grid.tsx) | props: type, editable, filterable, align, summary, disabled, autoFocus, descending; CSS: hover, active, disabled, focus-visible | @remixicon/react, motion, react, react-aria-components | H |
| `data-table` | free-pinned; [components/application/data-table/data-table.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/data-table/data-table.tsx) | StatusColor=lime, yellow, rose, cyan; props: initialsColor, purchase, dir, showSizeToggle; CSS: hover | @remixicon/react, @tanstack/react-table, react, react-aria-components | H |
| `table` | free-pinned; [components/base/table/table.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/table/table.tsx) | TableSize=sm, md | react, react-aria-components | H |

## Display

Primitives: `Text, Box, Draw, Progress, Animation, Role, Busy, Range`. Existing Kit: reuse layout/paint, not old skin visuals. Gap: exact card composition, sheen, count-up, status tone and contrast. Native/accessibility risk: status vs meter/progress role; reduced motion and image labels.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `announcement` | free-pinned; [components/base/announcement/announcement.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/announcement/announcement.tsx) | props: dismissible | @remixicon/react, motion, react | L–M |
| `avatar` | free-pinned; [components/base/avatar/avatar.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/avatar/avatar.tsx) | AvatarSize=xs, sm, md, lg; AvatarColor=neutral, blue, lime, pink | react | L–M |
| `badge` | free-pinned; [components/base/badges/badge.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/badges/badge.tsx) | BadgeColor=primary, neutral | react | L–M |
| `chip` | free-pinned; [components/base/badges/chip.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/badges/chip.tsx) | ChipVariant=bold, subtle, caption; ChipColor=orange, lime, rose, yellow, cyan, blue, purple, neutral, gray, soft | react | L–M |
| `divider` | free-pinned; [components/base/divider/divider.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/divider/divider.tsx) | DividerVariant=single, double, fill; DividerAlign=start, center, end | react | L–M |
| `important-alerts-card` | free-pinned; [components/application/medical/important-alerts-card.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/medical/important-alerts-card.tsx) | No local literal domain; composition/inherited API | @remixicon/react, react | L–M |
| `kbd` | free-pinned; [components/base/kbd/kbd.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/kbd/kbd.tsx) | No local literal domain; composition/inherited API | react | L–M |
| `meter` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | L–M |
| `patient-info-card` | free-pinned; [components/application/medical/patient-info-card.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/medical/patient-info-card.tsx) | No local literal domain; composition/inherited API | @remixicon/react, react | L–M |
| `progress` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | L–M |
| `skeleton` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | L–M |
| `spinner` | free-skill-only; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | L–M |
| `stat-cards` | free-pinned; [components/application/dashboard/stat-cards.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/dashboard/stat-cards.tsx) | StatCardsVariant=plain, footer; StatTone=blue, orange, purple, pink, sky, emerald; props: deltaColor, columns; CSS: hover, focus-visible | @remixicon/react, react, react-aria-components | L–M |
| `status-dot` | free-pinned; [components/base/badges/status-dot.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/badges/status-dot.tsx) | StatusDotColor=green, yellow, indigo | react | L–M |

## Upload

Primitives: `DropData, DataDragOver, Draggable, ButtonBase, Progress`. Existing Kit: reuse OS host file interfaces after review. Gap: host-owned picker/drop, validation and cancel/completion ownership. Native/accessibility risk: keyboard browse; no fake upload completion; file URLs private.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `file-upload` | free-pinned; [components/base/file-upload/file-upload.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/base/file-upload/file-upload.tsx) | UploadPhase=idle, uploading, complete; StaggerState=shown, hiding, hidden; CSS: active, focus-visible, hover | @remixicon/react, react | H |

## Agent

Primitives: `TextAreaBase, markdown, List, Disclosure/Collapsible, Animation, Draw`. Existing Kit: only primitives; no BoardUI agent equivalent. Gap: streaming state machine, markdown/code, nested controls; Pro permission. Native/accessibility risk: live-region noise, IME submit, interrupt/cancel, drag and focus.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `agent-avatar` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `agent-chat` | free-pinned; [components/application/agent-chat/agent-chat.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/agent-chat/agent-chat.tsx) | props: status, contained, configured, demo; CSS: hover | @ai-sdk/react, @remixicon/react, ai, react | H–VH |
| `agent-creator` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `agent-limits-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `agent-log` | free-pinned; [components/application/agent-log/agent-log.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/agent-log/agent-log.tsx) | props: run, first, last, reduce | motion, react | H–VH |
| `agent-progress` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `agent-thinking` | free-pinned; [components/application/agent-thinking/agent-thinking.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/agent-thinking/agent-thinking.tsx) | AgentThinkingVariant=wave, spin, stars, infinity; AgentThinkingTone=subtle, default, primary, accent; props: shimmer, showTimer, variant | react | H–VH |
| `composer` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `composer-attachments` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `composer-loader` | free-pinned; [components/application/composer-loader/composer-loader.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/composer-loader/composer-loader.tsx) | props: active, reverse, bloomOnly, surface | react | H–VH |
| `composer-panel` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `multi-agent-chat` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `project-board` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `questionnaire` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `task-list` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `web-search` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |

## Chart

Primitives: `ui/chart scales/Line/Area/Pie/Stack, Painter, Path`. Existing Kit: no existing BoardUI chart card equivalent. Gap: axes, legend, tooltip, hit testing, animations and missing chart layouts. Native/accessibility risk: nonvisual data alternative, keyboard exploration, dense-data cost.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `activity-rings-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `area-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `bar-list-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `combo-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `contributions-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `earnings-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `funnel-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `heatmap-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `line-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `orders-chart-card` | free-pinned; [components/application/dashboard/orders-chart-card.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/dashboard/orders-chart-card.tsx) | No local literal domain; composition/inherited API | react, recharts | H–VH |
| `radar-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `radial-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `revenue-chart-card` | free-pinned; [components/application/dashboard/revenue-chart-card.tsx](https://github.com/BoardUI/boardui/blob/ec5c5eb3e29c2441be63b9422cd54f411ef23466/components/application/dashboard/revenue-chart-card.tsx) | No local literal domain; composition/inherited API | react, recharts | H–VH |
| `sankey-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `scatter-chart-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `sleep-score-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `stage-bars-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |
| `steps-card` | pro; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | H–VH |

## Template

Primitives: `composition of accepted native components`. Existing Kit: do not port as one primitive. Gap: source/permission plus all contained controls, data and responsive layouts. Native/accessibility risk: composite focus/scroll, data ownership and resource lifecycle.

| Item | Access / source | Variants / sizes / explicit states | Dependencies | Difficulty |
| --- | --- | --- | --- | --- |
| `template-ai-chat` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |
| `template-ai-image-generation` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |
| `template-ai-profile` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |
| `template-finance` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |
| `template-home-dashboard` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |
| `template-hr` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |
| `template-marketing` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |
| `template-medical-profile` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |
| `template-multi-agent-chat` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |
| `template-project-board` | pro-template; Source unavailable; metadata only | Unknown; see public description in JSON | Unverified | VH |


Public BoardUI-derived catalogue metadata is attributed to Mertcan Dundar Esmergul (BoardUI), 2026. The fixed public export's [MIT notice](BOARDUI-MIT-LICENSE.txt) accompanies this research. Newer skill-only and Pro entries are discovery metadata; no Pro code is included.
