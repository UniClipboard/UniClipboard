import { Minus, Plus, RotateCcw, X } from 'lucide-react'
import { useState, type MouseEvent } from 'react'
import { HexColorPicker } from 'react-colorful'
import { useTranslation } from 'react-i18next'
import {
  Button,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui'
import { DEFAULT_THEME_COLOR, THEME_COLORS } from '@/constants/theme'
import { useSetting, type Theme } from '@/hooks/useSetting'
import { useUiScale } from '@/hooks/useUiScale'
import { hexToOklch, oklchToHexSafe } from '@/lib/color-convert'
import { createLogger } from '@/lib/logger'
import {
  themePresets,
  type OverridableToken,
  type ThemeMode,
  type ThemeTokens,
} from '@/lib/theme-engine'
import { setTransitionOrigin } from '@/lib/theme-transition'
import { cn } from '@/lib/utils'
import { SettingGroup } from './SettingGroup'

const log = createLogger('appearance-section')

/** 一个可点击的 token 色块行——点开 Popover 用 react-colorful 改色,带 reset。 */
interface ColorPickerRowProps {
  label: string
  /** preset 默认值（oklch 字符串）。 */
  presetColor: string
  /** 用户当前 override 值（oklch 字符串）；为空则跟随 preset。 */
  overrideColor: string | null
  /** picker 拾色后回调,传入 oklch 字符串。 */
  onChange: (oklch: string) => void
  /** 重置按钮回调,清掉 override。 */
  onReset: () => void
  resetLabel: string
  modifiedLabel: string
  hexInputLabel: string
}

function ColorPickerRow({
  label,
  presetColor,
  overrideColor,
  onChange,
  onReset,
  resetLabel,
  modifiedLabel,
  hexInputLabel,
}: ColorPickerRowProps) {
  const isModified = overrideColor !== null
  const effectiveColor = overrideColor ?? presetColor
  const hex = oklchToHexSafe(effectiveColor)

  // Popover 打开时的本地 hex,用于即时预览;关闭后才正式 commit。
  const [open, setOpen] = useState(false)
  const [draftHex, setDraftHex] = useState(hex)

  // Closed-state 直接用最新 hex,避免靠 useEffect 在 hex 变化时回写 draftHex
  // (那是一个由 prop 派生的状态链,踩 no-chain-state-updates)。打开时
  // 才用本地 draftHex 做即时预览。
  const displayedHex = open ? draftHex : hex

  const handleOpenChange = (next: boolean) => {
    if (next && !open) {
      // 打开瞬间把外部 hex snapshot 进 draft,作为编辑起点。
      setDraftHex(hex)
    }
    setOpen(next)
  }

  const handleHexChange = (next: string) => {
    setDraftHex(next)
    try {
      onChange(hexToOklch(next))
    } catch (err) {
      log.error({ err, hex: next }, 'Invalid hex from picker')
    }
  }

  return (
    <div className="flex items-center justify-between px-4 py-2.5 text-sm">
      <span className="text-muted-foreground">{label}</span>
      <span className="flex items-center gap-2">
        {isModified && (
          <span className="text-[10px] uppercase tracking-wider text-primary/80">
            {modifiedLabel}
          </span>
        )}
        <Popover open={open} onOpenChange={handleOpenChange}>
          <PopoverTrigger asChild>
            <button
              type="button"
              className={cn(
                'flex items-center gap-2 rounded-md border px-2 py-1 transition-all',
                'hover:border-primary/40 hover:shadow-sm',
                isModified ? 'border-primary/50' : 'border-border/60'
              )}
            >
              <span
                className="size-4 rounded-sm border border-border/40 shadow-inner"
                style={{ backgroundColor: hex }}
                aria-hidden
              />
              <span className="font-mono text-xs text-foreground/80">{hex.toUpperCase()}</span>
            </button>
          </PopoverTrigger>
          <PopoverContent className="w-auto p-3" align="end">
            <HexColorPicker color={displayedHex} onChange={handleHexChange} />
            <div className="mt-3 flex items-center justify-between gap-2">
              <input
                type="text"
                aria-label={hexInputLabel}
                value={displayedHex}
                onChange={e => {
                  const v = e.target.value.trim()
                  // 只在合法 6/3 位 hex 时调 onChange,避免输入中途崩溃。
                  if (/^#[0-9a-fA-F]{3}$|^#[0-9a-fA-F]{6}$/.test(v)) {
                    handleHexChange(v)
                  } else {
                    setDraftHex(v)
                  }
                }}
                className="flex-1 rounded-md border border-border/60 bg-background px-2 py-1 font-mono text-xs"
                spellCheck={false}
              />
              {isModified && (
                <Button
                  variant="ghost"
                  size="icon-xs"
                  onClick={() => {
                    onReset()
                    setOpen(false)
                  }}
                  aria-label={resetLabel}
                  title={resetLabel}
                >
                  <X />
                </Button>
              )}
            </div>
          </PopoverContent>
        </Popover>
      </span>
    </div>
  )
}

/** Light / Dark 双窗预览,**点窗口本身切主题模式**。 */
interface ThemeWindowPreviewProps {
  label: string
  tokens: ThemeTokens
  selected: boolean
  hint: string
  onSelect: (e: MouseEvent) => void
}

function ThemeWindowPreview({ label, tokens, selected, hint, onSelect }: ThemeWindowPreviewProps) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      aria-label={!selected ? hint : label}
      className={cn(
        'group relative h-full min-h-[6.625rem] rounded-xl border overflow-hidden p-0 text-left shadow-sm transition-all',
        selected ? 'border-primary/60 ring-2 ring-primary/30 shadow-md' : 'border-border/60',
        'hover:border-primary/40 hover:shadow-md'
      )}
      style={{ backgroundColor: tokens.background }}
    >
      <div
        className="flex h-full min-h-[6.625rem]"
        style={{ backgroundColor: tokens.background, color: tokens.foreground }}
      >
        <div
          className="w-1/3 py-3 px-2 border-r flex flex-col gap-1.5"
          style={{ backgroundColor: tokens.sidebar, borderColor: tokens.sidebarBorder }}
        >
          <span
            className="h-1.5 rounded-full"
            style={{ backgroundColor: tokens.sidebarPrimary, width: '70%' }}
          />
          <span
            className="h-1.5 rounded-full"
            style={{ backgroundColor: tokens.sidebarAccent, width: '50%' }}
          />
          <span
            className="h-1.5 rounded-full"
            style={{ backgroundColor: tokens.sidebarBorder, width: '60%' }}
          />
        </div>
        <div className="flex-1 p-3 flex flex-col gap-1.5">
          <span
            className="h-1.5 rounded-full"
            style={{ backgroundColor: tokens.foreground, opacity: 0.85, width: '60%' }}
          />
          <span
            className="h-1.5 rounded-full"
            style={{ backgroundColor: tokens.mutedForeground, width: '85%' }}
          />
          <span
            className="h-1.5 rounded-full"
            style={{ backgroundColor: tokens.mutedForeground, opacity: 0.6, width: '70%' }}
          />
          <span
            className="mt-1 inline-block self-start rounded-md px-2 py-0.5 text-[10px] font-medium"
            style={{
              backgroundColor: tokens.primary,
              color: tokens.primaryForeground,
            }}
          >
            Action
          </span>
        </div>
      </div>
      {/* hover 时浮一个 hint 标签。 */}
      {!selected && (
        <span className="pointer-events-none absolute inset-x-0 bottom-0 translate-y-full bg-primary/90 px-3 py-1 text-center text-[11px] font-medium text-primary-foreground transition-transform group-hover:translate-y-0">
          {hint}
        </span>
      )}
    </button>
  )
}

interface ThemeSystemPreviewProps {
  label: string
  lightTokens: ThemeTokens
  darkTokens: ThemeTokens
  selected: boolean
  hint: string
  onSelect: (e: MouseEvent) => void
}

function SystemHalf({ tokens }: { tokens: ThemeTokens }) {
  return (
    <div
      className="flex min-h-[6.625rem] min-w-0 flex-col gap-2.5 p-3"
      style={{ color: tokens.foreground }}
    >
      <span
        className="h-1.5 rounded-full"
        style={{ backgroundColor: tokens.foreground, opacity: 0.85, width: '58%' }}
      />
      <div
        className="rounded-md p-2"
        style={{
          backgroundColor: tokens.sidebar,
        }}
      >
        <span
          className="block h-1.5 rounded-full"
          style={{ backgroundColor: tokens.sidebarPrimary, width: '62%' }}
        />
      </div>
      <div className="flex flex-col gap-1.5">
        <span
          className="h-1.5 rounded-full"
          style={{ backgroundColor: tokens.mutedForeground, width: '88%' }}
        />
        <span
          className="h-1.5 rounded-full"
          style={{ backgroundColor: tokens.mutedForeground, opacity: 0.6, width: '70%' }}
        />
      </div>
    </div>
  )
}

function ThemeSystemPreview({
  label,
  lightTokens,
  darkTokens,
  selected,
  hint,
  onSelect,
}: ThemeSystemPreviewProps) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      aria-label={!selected ? hint : label}
      className={cn(
        'group relative rounded-xl overflow-hidden p-0 text-left shadow-sm transition-all',
        selected ? 'shadow-md' : '',
        'hover:shadow-md'
      )}
      style={{
        backgroundImage: `linear-gradient(90deg, ${lightTokens.background} 0%, ${lightTokens.background} 50%, ${darkTokens.background} 50%, ${darkTokens.background} 100%)`,
      }}
    >
      <div className="grid min-h-full grid-cols-2">
        <SystemHalf tokens={lightTokens} />
        <SystemHalf tokens={darkTokens} />
      </div>
      <span
        className="pointer-events-none absolute right-3 top-2 text-[10px] uppercase tracking-wider"
        style={{ color: darkTokens.sidebarForeground }}
      >
        {label}
      </span>
      {!selected && (
        <span className="pointer-events-none absolute inset-x-0 bottom-0 translate-y-full bg-primary/90 px-3 py-1 text-center text-[11px] font-medium text-primary-foreground transition-transform group-hover:translate-y-0">
          {hint}
        </span>
      )}
    </button>
  )
}

interface ThemePresetSectionProps {
  title: string
  presetLabel: string
  selectedPreset: string
  onPresetChange: (next: string, e: MouseEvent) => Promise<void>
  labels: {
    accent: string
    background: string
    foreground: string
    border: string
  }
  mode: ThemeMode
  overrides: Record<string, string>
  /** 拾色后回调（oklch 字符串）。 */
  onOverrideChange: (token: OverridableToken, oklch: string) => Promise<void>
  /** 重置回调,清除某个 token 的 override。 */
  onOverrideReset: (token: OverridableToken) => Promise<void>
  resetLabel: string
  modifiedLabel: string
  hexInputLabel: string
}

function ThemePresetSection({
  title,
  presetLabel,
  selectedPreset,
  onPresetChange,
  labels,
  mode,
  overrides,
  onOverrideChange,
  onOverrideReset,
  resetLabel,
  modifiedLabel,
  hexInputLabel,
}: ThemePresetSectionProps) {
  const preset = themePresets[selectedPreset] ?? themePresets[DEFAULT_THEME_COLOR]
  const tokens = mode === 'dark' ? preset.dark : preset.light

  const tokenRow = (
    token: OverridableToken,
    label: string,
    presetValue: string
  ): React.ReactNode => (
    <ColorPickerRow
      key={token}
      label={label}
      presetColor={presetValue}
      overrideColor={overrides[token] ?? null}
      onChange={oklch => void onOverrideChange(token, oklch)}
      onReset={() => void onOverrideReset(token)}
      resetLabel={resetLabel}
      modifiedLabel={modifiedLabel}
      hexInputLabel={hexInputLabel}
    />
  )

  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between px-1">
        <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
          {title}
        </h3>
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">{presetLabel}</span>
          <Select
            value={selectedPreset}
            onValueChange={value => {
              const fakeEvent = {
                clientX: window.innerWidth / 2,
                clientY: window.innerHeight / 2,
              } as MouseEvent
              void onPresetChange(value, fakeEvent)
            }}
          >
            <SelectTrigger className="h-7 w-[160px] text-xs">
              <SelectValue placeholder={DEFAULT_THEME_COLOR} />
            </SelectTrigger>
            <SelectContent>
              {THEME_COLORS.map(item => (
                <SelectItem key={item.name} value={item.name} className="text-xs">
                  <span className="flex items-center gap-2">
                    <span className="flex size-3 overflow-hidden rounded-sm">
                      {item.previewDots.slice(0, 3).map((dot, i) => (
                        <span key={i} className="flex-1 h-full" style={{ backgroundColor: dot }} />
                      ))}
                    </span>
                    <span className="capitalize">{item.name}</span>
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>
      <div className="rounded-lg border border-border/60 bg-card divide-y divide-border/40 overflow-hidden">
        {tokenRow('primary', labels.accent, tokens.primary)}
        {tokenRow('background', labels.background, tokens.background)}
        {tokenRow('foreground', labels.foreground, tokens.foreground)}
        {tokenRow('border', labels.border, tokens.border)}
      </div>
    </div>
  )
}

export default function AppearanceSection() {
  const { t } = useTranslation()
  const { setting, updateGeneralSetting } = useSetting()
  const {
    options,
    setScale,
    resetScale,
    isDefault,
    isSelected,
    zoomIn,
    zoomOut,
    canZoomIn,
    canZoomOut,
  } = useUiScale()

  const theme: Theme = setting?.general?.theme || 'system'
  const followSystem = theme === 'system'
  const legacyThemeColor = setting?.general?.themeColor || null
  const lightPreset = setting?.general?.themeColorLight || legacyThemeColor || DEFAULT_THEME_COLOR
  const darkPreset = setting?.general?.themeColorDark || legacyThemeColor || DEFAULT_THEME_COLOR
  const setTheme = async (next: Theme, e: MouseEvent) => {
    try {
      setTransitionOrigin(e.clientX, e.clientY)
      await updateGeneralSetting({ theme: next })
    } catch (error) {
      log.error({ err: error }, 'Failed to change theme')
    }
  }

  const handleLightPresetChange = async (next: string, e: MouseEvent) => {
    try {
      setTransitionOrigin(e.clientX, e.clientY)
      // 显式写入新拆分字段；同时清空旧 themeColor 字段,让 daemon 端不再回退到它。
      await updateGeneralSetting({
        themeColorLight: next,
        themeColor: null,
      })
    } catch (error) {
      log.error({ err: error }, 'Failed to change light theme color')
    }
  }

  const handleDarkPresetChange = async (next: string, e: MouseEvent) => {
    try {
      setTransitionOrigin(e.clientX, e.clientY)
      await updateGeneralSetting({
        themeColorDark: next,
        themeColor: null,
      })
    } catch (error) {
      log.error({ err: error }, 'Failed to change dark theme color')
    }
  }

  const lightOverrides = setting?.general?.themeOverridesLight ?? {}
  const darkOverrides = setting?.general?.themeOverridesDark ?? {}

  const handleOverrideChange = async (
    side: 'light' | 'dark',
    token: OverridableToken,
    oklch: string
  ) => {
    try {
      const current = side === 'dark' ? darkOverrides : lightOverrides
      const nextMap = { ...current, [token]: oklch }
      const fakeEvent = {
        clientX: window.innerWidth / 2,
        clientY: window.innerHeight / 2,
      } as MouseEvent
      setTransitionOrigin(fakeEvent.clientX, fakeEvent.clientY)
      await updateGeneralSetting(
        side === 'dark' ? { themeOverridesDark: nextMap } : { themeOverridesLight: nextMap }
      )
    } catch (error) {
      log.error({ err: error, side, token }, 'Failed to apply token override')
    }
  }

  const handleOverrideReset = async (side: 'light' | 'dark', token: OverridableToken) => {
    try {
      const current = side === 'dark' ? darkOverrides : lightOverrides
      // 移除该 key,daemon 收到不带该 key 的 map 就不再覆盖。
      const { [token]: _removed, ...rest } = current
      void _removed
      await updateGeneralSetting(
        side === 'dark' ? { themeOverridesDark: rest } : { themeOverridesLight: rest }
      )
    } catch (error) {
      log.error({ err: error, side, token }, 'Failed to reset token override')
    }
  }

  const lightTokens = (themePresets[lightPreset] ?? themePresets[DEFAULT_THEME_COLOR]).light
  const darkTokens = (themePresets[darkPreset] ?? themePresets[DEFAULT_THEME_COLOR]).dark

  return (
    <>
      <div className="space-y-1.5">
        <div className="flex items-center justify-between px-1">
          <h3 className="text-xs font-medium text-muted-foreground uppercase tracking-wider">
            {t('settings.sections.appearance.themePreview.title')}
          </h3>
        </div>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <ThemeWindowPreview
            label={t('settings.sections.appearance.themePreview.lightLabel')}
            tokens={lightTokens}
            selected={!followSystem && theme === 'light'}
            hint={t('settings.sections.appearance.themePreview.setAsLight')}
            onSelect={e => void setTheme('light', e)}
          />
          <ThemeWindowPreview
            label={t('settings.sections.appearance.themePreview.darkLabel')}
            tokens={darkTokens}
            selected={!followSystem && theme === 'dark'}
            hint={t('settings.sections.appearance.themePreview.setAsDark')}
            onSelect={e => void setTheme('dark', e)}
          />
          <ThemeSystemPreview
            label={t('settings.sections.appearance.themePreview.followSystem')}
            lightTokens={lightTokens}
            darkTokens={darkTokens}
            selected={followSystem}
            hint={t('settings.sections.appearance.themePreview.followSystem')}
            onSelect={e => void setTheme('system', e)}
          />
        </div>
      </div>

      <ThemePresetSection
        title={t('settings.sections.appearance.lightTheme.title')}
        presetLabel={t('settings.sections.appearance.lightTheme.preset')}
        selectedPreset={lightPreset}
        onPresetChange={handleLightPresetChange}
        labels={{
          accent: t('settings.sections.appearance.lightTheme.accent'),
          background: t('settings.sections.appearance.lightTheme.background'),
          foreground: t('settings.sections.appearance.lightTheme.foreground'),
          border: t('settings.sections.appearance.lightTheme.border'),
        }}
        mode="light"
        overrides={lightOverrides}
        onOverrideChange={(token, oklch) => handleOverrideChange('light', token, oklch)}
        onOverrideReset={token => handleOverrideReset('light', token)}
        resetLabel={t('settings.sections.appearance.tokenPicker.reset')}
        modifiedLabel={t('settings.sections.appearance.tokenPicker.modified')}
        hexInputLabel={t('settings.sections.appearance.tokenPicker.hexInputLabel')}
      />

      <ThemePresetSection
        title={t('settings.sections.appearance.darkTheme.title')}
        presetLabel={t('settings.sections.appearance.darkTheme.preset')}
        selectedPreset={darkPreset}
        onPresetChange={handleDarkPresetChange}
        labels={{
          accent: t('settings.sections.appearance.darkTheme.accent'),
          background: t('settings.sections.appearance.darkTheme.background'),
          foreground: t('settings.sections.appearance.darkTheme.foreground'),
          border: t('settings.sections.appearance.darkTheme.border'),
        }}
        mode="dark"
        overrides={darkOverrides}
        onOverrideChange={(token, oklch) => handleOverrideChange('dark', token, oklch)}
        onOverrideReset={token => handleOverrideReset('dark', token)}
        resetLabel={t('settings.sections.appearance.tokenPicker.reset')}
        modifiedLabel={t('settings.sections.appearance.tokenPicker.modified')}
        hexInputLabel={t('settings.sections.appearance.tokenPicker.hexInputLabel')}
      />

      <SettingGroup title={t('settings.sections.appearance.zoom.title')}>
        <div className="flex flex-col gap-3 p-4">
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="icon-xs"
              disabled={!canZoomOut}
              onClick={zoomOut}
              aria-label="Zoom out"
            >
              <Minus />
            </Button>

            <div className="flex flex-1 items-center rounded-lg bg-muted/60 p-0.5">
              {options.map(option => (
                <button
                  key={option.value}
                  type="button"
                  onClick={() => setScale(option.value)}
                  className={cn(
                    'flex-1 rounded-md px-1 py-1 text-xs font-medium transition-all',
                    isSelected(option)
                      ? 'bg-background text-foreground shadow-sm ring-1 ring-border/50'
                      : 'text-muted-foreground hover:text-foreground'
                  )}
                >
                  {option.label}
                </button>
              ))}
            </div>

            <Button
              variant="outline"
              size="icon-xs"
              disabled={!canZoomIn}
              onClick={zoomIn}
              aria-label="Zoom in"
            >
              <Plus />
            </Button>
          </div>

          <div className="flex items-center justify-between">
            <span className="text-xs text-muted-foreground">
              {t('settings.sections.appearance.zoom.description')}
            </span>
            {!isDefault && (
              <button
                type="button"
                onClick={() => resetScale()}
                className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
              >
                <RotateCcw className="size-3" />
                {t('settings.sections.appearance.zoom.reset')}
              </button>
            )}
          </div>
        </div>
      </SettingGroup>
    </>
  )
}
