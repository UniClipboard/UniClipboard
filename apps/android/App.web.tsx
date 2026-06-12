/**
 * App.web.tsx —— Web 平台的 UI Showcase (Material 3 Expressive)
 * 不挂载真实业务,仅用 mock data 演示新设计语言。
 *
 * 设计参考:Material 3 baseline (source color #6750A4)
 * - Tonal elevation 替代 box-shadow
 * - Surface container 五档层级
 * - Lucide icons (1.75 stroke, 一致线宽)
 * - 大圆角(Card 16, FAB 16, Pill 全圆)
 * - M3 type scale (Display / Headline / Title / Body / Label)
 */

import React, { useState, useEffect } from 'react';
import {
  Appearance,
  Image,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
  type TextStyle,
  type ViewStyle,
} from 'react-native';
import {
  Menu,
  Settings as SettingsIcon,
  Plus,
  Search,
  FileText,
  Image as ImageIcon,
  File as FileIcon,
  Star,
  MoreVertical,
  Copy,
  Share2,
  AlertCircle,
  Cloud,
  Check,
  Download,
  RefreshCw,
  Scissors,
  Link as LinkIcon,
  Palette,
} from 'lucide-react-native';

// ------------------------------------------------------------------
// Material 3 Token System (baseline + custom source #6750A4)
// ------------------------------------------------------------------

type M3Scheme = {
  primary: string;
  onPrimary: string;
  primaryContainer: string;
  onPrimaryContainer: string;
  secondary: string;
  onSecondary: string;
  secondaryContainer: string;
  onSecondaryContainer: string;
  tertiary: string;
  onTertiary: string;
  tertiaryContainer: string;
  onTertiaryContainer: string;
  error: string;
  onError: string;
  errorContainer: string;
  onErrorContainer: string;
  background: string;
  onBackground: string;
  surface: string;
  onSurface: string;
  surfaceVariant: string;
  onSurfaceVariant: string;
  outline: string;
  outlineVariant: string;
  surfaceContainerLowest: string;
  surfaceContainerLow: string;
  surfaceContainer: string;
  surfaceContainerHigh: string;
  surfaceContainerHighest: string;
  inverseSurface: string;
  inverseOnSurface: string;
};

// ------------------------------------------------------------------
// Source color presets —— 切换 source 时,主题色 + icon 背景色 + 选中态全部联动
// 每套是手调的 M3 风调色板,light/dark 各一份
// ------------------------------------------------------------------

type PaletteId = 'purple' | 'indigo' | 'teal' | 'rose' | 'amber';

type PaletteEntry = {
  id: PaletteId;
  label: string;
  swatch: string; // 给 picker 显示的代表色
  light: Pick<
    M3Scheme,
    | 'primary'
    | 'onPrimary'
    | 'primaryContainer'
    | 'onPrimaryContainer'
    | 'background'
    | 'surface'
    | 'surfaceContainerLowest'
    | 'surfaceContainerLow'
    | 'surfaceContainer'
    | 'surfaceContainerHigh'
    | 'surfaceContainerHighest'
    | 'surfaceVariant'
    | 'onSurfaceVariant'
    | 'outline'
    | 'outlineVariant'
  >;
  dark: PaletteEntry['light'];
};

const PALETTES: PaletteEntry[] = [
  {
    id: 'purple',
    label: '深紫',
    swatch: '#6750A4',
    light: {
      primary: '#6750A4',
      onPrimary: '#FFFFFF',
      primaryContainer: '#EADDFF',
      onPrimaryContainer: '#21005D',
      background: '#FEF7FF',
      surface: '#FEF7FF',
      surfaceContainerLowest: '#FFFFFF',
      surfaceContainerLow: '#F7F2FA',
      surfaceContainer: '#F3EDF7',
      surfaceContainerHigh: '#ECE6F0',
      surfaceContainerHighest: '#E6E0E9',
      surfaceVariant: '#E7E0EC',
      onSurfaceVariant: '#49454F',
      outline: '#79747E',
      outlineVariant: '#CAC4D0',
    },
    dark: {
      primary: '#D0BCFF',
      onPrimary: '#381E72',
      primaryContainer: '#4F378B',
      onPrimaryContainer: '#EADDFF',
      background: '#141218',
      surface: '#141218',
      surfaceContainerLowest: '#0F0D13',
      surfaceContainerLow: '#1D1B20',
      surfaceContainer: '#211F26',
      surfaceContainerHigh: '#2B2930',
      surfaceContainerHighest: '#36343B',
      surfaceVariant: '#49454F',
      onSurfaceVariant: '#CAC4D0',
      outline: '#938F99',
      outlineVariant: '#49454F',
    },
  },
  {
    id: 'indigo',
    label: '靛蓝',
    swatch: '#4A4FCF',
    light: {
      primary: '#4A4FCF',
      onPrimary: '#FFFFFF',
      primaryContainer: '#DEDFFF',
      onPrimaryContainer: '#000F73',
      background: '#FBF8FF',
      surface: '#FBF8FF',
      surfaceContainerLowest: '#FFFFFF',
      surfaceContainerLow: '#F4F1F9',
      surfaceContainer: '#EEEBF3',
      surfaceContainerHigh: '#E9E6EE',
      surfaceContainerHighest: '#E3E0E8',
      surfaceVariant: '#E3E1EC',
      onSurfaceVariant: '#46464F',
      outline: '#767680',
      outlineVariant: '#C7C5D0',
    },
    dark: {
      primary: '#BCC2FF',
      onPrimary: '#1A23A6',
      primaryContainer: '#333BB7',
      onPrimaryContainer: '#DEDFFF',
      background: '#121318',
      surface: '#121318',
      surfaceContainerLowest: '#0D0E13',
      surfaceContainerLow: '#1A1B21',
      surfaceContainer: '#1F1F25',
      surfaceContainerHigh: '#29292F',
      surfaceContainerHighest: '#34343A',
      surfaceVariant: '#46464F',
      onSurfaceVariant: '#C7C5D0',
      outline: '#91909A',
      outlineVariant: '#46464F',
    },
  },
  {
    id: 'teal',
    label: '青绿',
    swatch: '#006A60',
    light: {
      primary: '#006A60',
      onPrimary: '#FFFFFF',
      primaryContainer: '#74F8E5',
      onPrimaryContainer: '#00201C',
      background: '#F4FBF8',
      surface: '#F4FBF8',
      surfaceContainerLowest: '#FFFFFF',
      surfaceContainerLow: '#EEF5F2',
      surfaceContainer: '#E8EFED',
      surfaceContainerHigh: '#E2EAE7',
      surfaceContainerHighest: '#DCE4E2',
      surfaceVariant: '#DAE5E1',
      onSurfaceVariant: '#3F4946',
      outline: '#6F7976',
      outlineVariant: '#BEC9C5',
    },
    dark: {
      primary: '#53DBC9',
      onPrimary: '#003731',
      primaryContainer: '#005048',
      onPrimaryContainer: '#74F8E5',
      background: '#0F1513',
      surface: '#0F1513',
      surfaceContainerLowest: '#0A100E',
      surfaceContainerLow: '#171D1B',
      surfaceContainer: '#1B2220',
      surfaceContainerHigh: '#262D2A',
      surfaceContainerHighest: '#313836',
      surfaceVariant: '#3F4946',
      onSurfaceVariant: '#BEC9C5',
      outline: '#89938F',
      outlineVariant: '#3F4946',
    },
  },
  {
    id: 'rose',
    label: '玫瑰',
    swatch: '#984061',
    light: {
      primary: '#984061',
      onPrimary: '#FFFFFF',
      primaryContainer: '#FFD9E2',
      onPrimaryContainer: '#3E001D',
      background: '#FFF8F8',
      surface: '#FFF8F8',
      surfaceContainerLowest: '#FFFFFF',
      surfaceContainerLow: '#FAF1F2',
      surfaceContainer: '#F4EBED',
      surfaceContainerHigh: '#EEE6E7',
      surfaceContainerHighest: '#E8E0E1',
      surfaceVariant: '#F2DDE1',
      onSurfaceVariant: '#514347',
      outline: '#837377',
      outlineVariant: '#D5C2C6',
    },
    dark: {
      primary: '#FFB1C8',
      onPrimary: '#5E1133',
      primaryContainer: '#7B2949',
      onPrimaryContainer: '#FFD9E2',
      background: '#191113',
      surface: '#191113',
      surfaceContainerLowest: '#140C0E',
      surfaceContainerLow: '#21191B',
      surfaceContainer: '#251D1F',
      surfaceContainerHigh: '#30272A',
      surfaceContainerHighest: '#3B3235',
      surfaceVariant: '#514347',
      onSurfaceVariant: '#D5C2C6',
      outline: '#9E8C90',
      outlineVariant: '#514347',
    },
  },
  {
    id: 'amber',
    label: '琥珀',
    swatch: '#825512',
    light: {
      primary: '#825512',
      onPrimary: '#FFFFFF',
      primaryContainer: '#FFDDB7',
      onPrimaryContainer: '#2A1700',
      background: '#FFF8F4',
      surface: '#FFF8F4',
      surfaceContainerLowest: '#FFFFFF',
      surfaceContainerLow: '#FAF1EB',
      surfaceContainer: '#F4ECE6',
      surfaceContainerHigh: '#EFE6E0',
      surfaceContainerHighest: '#E9E0DA',
      surfaceVariant: '#F1E0CF',
      onSurfaceVariant: '#4F4639',
      outline: '#817567',
      outlineVariant: '#D4C4B4',
    },
    dark: {
      primary: '#FBBA73',
      onPrimary: '#482900',
      primaryContainer: '#663D00',
      onPrimaryContainer: '#FFDDB7',
      background: '#181210',
      surface: '#181210',
      surfaceContainerLowest: '#120D0B',
      surfaceContainerLow: '#211A17',
      surfaceContainer: '#251E1B',
      surfaceContainerHigh: '#302925',
      surfaceContainerHighest: '#3B3330',
      surfaceVariant: '#4F4639',
      onSurfaceVariant: '#D4C4B4',
      outline: '#9C8E7E',
      outlineVariant: '#4F4639',
    },
  },
];

// 通用配色(error / surface inverse / on*),不随 source 变化
const fixedLight = {
  onBackground: '#1D1B20',
  onSurface: '#1D1B20',
  error: '#B3261E',
  onError: '#FFFFFF',
  errorContainer: '#F9DEDC',
  onErrorContainer: '#410E0B',
  inverseSurface: '#322F35',
  inverseOnSurface: '#F5EFF7',
  // 兼容旧字段(已不再用作 icon 背景,但保留以免类型缺失)
  secondary: '#625B71',
  onSecondary: '#FFFFFF',
  secondaryContainer: '#E8DEF8',
  onSecondaryContainer: '#1D192B',
  tertiary: '#7D5260',
  onTertiary: '#FFFFFF',
  tertiaryContainer: '#FFD8E4',
  onTertiaryContainer: '#31111D',
};

const fixedDark = {
  onBackground: '#E6E0E9',
  onSurface: '#E6E0E9',
  error: '#F2B8B5',
  onError: '#601410',
  errorContainer: '#8C1D18',
  onErrorContainer: '#F9DEDC',
  inverseSurface: '#E6E0E9',
  inverseOnSurface: '#322F35',
  secondary: '#CCC2DC',
  onSecondary: '#332D41',
  secondaryContainer: '#4A4458',
  onSecondaryContainer: '#E8DEF8',
  tertiary: '#EFB8C8',
  onTertiary: '#492532',
  tertiaryContainer: '#633B48',
  onTertiaryContainer: '#FFD8E4',
};

function buildScheme(paletteId: PaletteId, isDark: boolean): M3Scheme {
  const p = PALETTES.find((x) => x.id === paletteId) ?? PALETTES[0];
  const variant = isDark ? p.dark : p.light;
  const fixed = isDark ? fixedDark : fixedLight;
  return { ...fixed, ...variant } as M3Scheme;
}

const shape = {
  none: 0,
  xs: 4,
  sm: 8,
  md: 12,
  lg: 16,
  xl: 28,
  full: 999,
};

const space = (n: number) => n * 4;

const type = {
  displayMedium: { fontSize: 45, lineHeight: 52, fontWeight: '400' } as TextStyle,
  headlineSmall: { fontSize: 24, lineHeight: 32, fontWeight: '400' } as TextStyle,
  titleLarge: { fontSize: 22, lineHeight: 28, fontWeight: '500' } as TextStyle,
  titleMedium: {
    fontSize: 16,
    lineHeight: 24,
    fontWeight: '500',
    letterSpacing: 0.15,
  } as TextStyle,
  titleSmall: { fontSize: 14, lineHeight: 20, fontWeight: '500', letterSpacing: 0.1 } as TextStyle,
  bodyLarge: { fontSize: 16, lineHeight: 24, fontWeight: '400', letterSpacing: 0.5 } as TextStyle,
  bodyMedium: { fontSize: 14, lineHeight: 20, fontWeight: '400', letterSpacing: 0.25 } as TextStyle,
  bodySmall: { fontSize: 12, lineHeight: 16, fontWeight: '400', letterSpacing: 0.4 } as TextStyle,
  labelLarge: { fontSize: 14, lineHeight: 20, fontWeight: '500', letterSpacing: 0.1 } as TextStyle,
  labelMedium: { fontSize: 12, lineHeight: 16, fontWeight: '500', letterSpacing: 0.5 } as TextStyle,
  labelSmall: { fontSize: 11, lineHeight: 16, fontWeight: '500', letterSpacing: 0.5 } as TextStyle,
} satisfies Record<string, TextStyle>;

// ------------------------------------------------------------------
// Top-level App
// ------------------------------------------------------------------

type ThemeMode = 'auto' | 'light' | 'dark';

export default function App() {
  const [systemDark, setSystemDark] = useState(Appearance.getColorScheme() === 'dark');
  const [mode, setMode] = useState<ThemeMode>('auto');
  const [paletteId, setPaletteId] = useState<PaletteId>('indigo');

  useEffect(() => {
    const sub = Appearance.addChangeListener(({ colorScheme }) => {
      setSystemDark(colorScheme === 'dark');
    });
    return () => sub.remove();
  }, []);

  const isDark = mode === 'auto' ? systemDark : mode === 'dark';
  const c = buildScheme(paletteId, isDark);

  return (
    <View style={[styles.root, { backgroundColor: c.background }]}>
      <ScrollView contentContainerStyle={styles.scroll}>
        <TopAppBar c={c} />

        <View style={styles.brandBlock}>
          <Text style={[type.displayMedium, { color: c.onBackground }]}>UniClip</Text>
          <Text style={[type.bodyLarge, { color: c.onSurfaceVariant, marginTop: space(1) }]}>
            Material 3 Expressive · UI 重设计预览
          </Text>
        </View>

        <SegmentedThemeSwitcher c={c} value={mode} onChange={setMode} />

        <ColorPicker c={c} value={paletteId} onChange={setPaletteId} />

        <SectionLabel c={c} icon={<Cloud size={16} color={c.onSurfaceVariant} strokeWidth={2} />}>
          当前剪贴板
        </SectionLabel>

        <ClipboardCardFilled
          c={c}
          icon={<FileText size={20} color={c.onPrimaryContainer} strokeWidth={2} />}
          type="文本"
          timestamp="刚刚"
          size="128 B"
          preview={
            '欢迎使用 UniClip。\n这是一段示例文本,用于预览新的 Material 3 Expressive 卡片样式。'
          }
          source="远程"
        />

        <View style={{ height: space(3) }} />

        <ClipboardCardImage
          c={c}
          imageUrl="https://picsum.photos/seed/uniclip-m3-2/640/360"
          type="图片"
          timestamp="3 分钟前"
          size="245 KB"
          fileName="design-mockup-v3.png"
          source="本地"
        />

        <SectionLabel
          c={c}
          icon={<AlertCircle size={16} color={c.onSurfaceVariant} strokeWidth={2} />}
        >
          错误状态(M3 errorContainer)
        </SectionLabel>

        <ErrorCard c={c} />

        <SectionLabel c={c} icon={<Search size={16} color={c.onSurfaceVariant} strokeWidth={2} />}>
          历史记录
        </SectionLabel>

        <FilterChipsRow c={c} />

        <View style={{ height: space(2) }} />

        <ListSurface c={c}>
          <M3ListItem
            c={c}
            leading={
              <LeadingIconAvatar
                bg={c.primaryContainer}
                fg={c.onPrimaryContainer}
                icon={<FileText size={20} strokeWidth={2} color={c.onPrimaryContainer} />}
              />
            }
            headline="项目周会笔记"
            supporting="刚刚 · 文本 · 128 B"
            trailing={<Star size={20} color={c.primary} fill={c.primary} strokeWidth={0} />}
          />
          <Divider c={c} />
          <M3ListItem
            c={c}
            leading={
              <LeadingImageThumb
                url="https://picsum.photos/seed/uniclip-h1/120/120"
                radius={shape.md}
              />
            }
            headline="screenshot.png"
            supporting="2 小时前 · 图片 · 245 KB"
            trailing={<MoreVertical size={20} color={c.onSurfaceVariant} strokeWidth={2} />}
          />
          <Divider c={c} />
          <M3ListItem
            c={c}
            leading={
              <LeadingIconAvatar
                bg={c.primaryContainer}
                fg={c.onPrimaryContainer}
                icon={<FileIcon size={20} strokeWidth={2} color={c.onPrimaryContainer} />}
              />
            }
            headline="report-2025-11.pdf"
            supporting="昨天 · 文件 · 1.4 MB"
            trailing={<Download size={20} color={c.primary} strokeWidth={2} />}
          />
          <Divider c={c} />
          <M3ListItem
            c={c}
            leading={
              <LeadingIconAvatar
                bg={c.primaryContainer}
                fg={c.onPrimaryContainer}
                icon={<LinkIcon size={20} strokeWidth={2} color={c.onPrimaryContainer} />}
              />
            }
            headline="https://anthropic.com"
            supporting="昨天 · 链接"
            trailing={<MoreVertical size={20} color={c.onSurfaceVariant} strokeWidth={2} />}
          />
        </ListSurface>

        <SectionLabel
          c={c}
          icon={<RefreshCw size={16} color={c.onSurfaceVariant} strokeWidth={2} />}
        >
          按钮 / 状态指示
        </SectionLabel>

        <ButtonShowcase c={c} />

        <SectionLabel c={c} icon={<Palette size={16} color={c.onSurfaceVariant} strokeWidth={2} />}>
          色板 · M3 Tonal Surface
        </SectionLabel>

        <TonalPaletteGrid c={c} />

        <View style={{ height: space(20) }} />
      </ScrollView>

      <ExtendedFAB c={c} />
    </View>
  );
}

// ------------------------------------------------------------------
// Components
// ------------------------------------------------------------------

function TopAppBar({ c }: { c: M3Scheme }) {
  return (
    <View style={[styles.topBar, { backgroundColor: c.surface }]}>
      <Pressable style={({ hovered }: any) => [styles.iconBtn, hovered && stateLayer(c, 'hover')]}>
        <Menu size={24} color={c.onSurface} strokeWidth={2} />
      </Pressable>
      <Text style={[type.titleLarge, { color: c.onSurface, flex: 1, marginLeft: space(3) }]}>
        UniClip
      </Text>
      <Pressable style={({ hovered }: any) => [styles.iconBtn, hovered && stateLayer(c, 'hover')]}>
        <Search size={24} color={c.onSurfaceVariant} strokeWidth={2} />
      </Pressable>
      <Pressable style={({ hovered }: any) => [styles.iconBtn, hovered && stateLayer(c, 'hover')]}>
        <SettingsIcon size={24} color={c.onSurfaceVariant} strokeWidth={2} />
      </Pressable>
    </View>
  );
}

function SegmentedThemeSwitcher({
  c,
  value,
  onChange,
}: {
  c: M3Scheme;
  value: ThemeMode;
  onChange: (v: ThemeMode) => void;
}) {
  const options: { v: ThemeMode; label: string }[] = [
    { v: 'auto', label: '跟随系统' },
    { v: 'light', label: '浅色' },
    { v: 'dark', label: '深色' },
  ];
  return (
    <View style={[styles.segmented, { borderColor: c.outline }]}>
      {options.map((opt, i) => {
        const active = value === opt.v;
        const isFirst = i === 0;
        const isLast = i === options.length - 1;
        return (
          <Pressable
            key={opt.v}
            onPress={() => onChange(opt.v)}
            style={({ hovered, pressed }: any) => [
              styles.segmentedItem,
              {
                backgroundColor: active ? c.primaryContainer : 'transparent',
                borderLeftWidth: isFirst ? 0 : 1,
                borderLeftColor: c.outline,
              },
              hovered && !active && { backgroundColor: alpha(c.onSurface, 0.04) },
              pressed && { backgroundColor: alpha(c.onSurface, 0.08) },
              isFirst && { borderTopLeftRadius: shape.full, borderBottomLeftRadius: shape.full },
              isLast && { borderTopRightRadius: shape.full, borderBottomRightRadius: shape.full },
            ]}
          >
            {active && (
              <Check
                size={18}
                color={c.onPrimaryContainer}
                strokeWidth={2}
                style={{ marginRight: space(2) }}
              />
            )}
            <Text style={[type.labelLarge, { color: active ? c.onPrimaryContainer : c.onSurface }]}>
              {opt.label}
            </Text>
          </Pressable>
        );
      })}
    </View>
  );
}

function SectionLabel({
  c,
  icon,
  children,
}: {
  c: M3Scheme;
  icon?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <View style={styles.sectionLabel}>
      {icon}
      <Text
        style={[type.labelLarge, { color: c.onSurfaceVariant, marginLeft: icon ? space(2) : 0 }]}
      >
        {children}
      </Text>
    </View>
  );
}

function ClipboardCardFilled({
  c,
  icon,
  type: typeLabel,
  timestamp,
  size,
  preview,
  source,
}: {
  c: M3Scheme;
  icon: React.ReactNode;
  type: string;
  timestamp: string;
  size: string;
  preview: string;
  source: string;
}) {
  return (
    <View style={[styles.cardFilled, { backgroundColor: c.surfaceContainerHigh }]}>
      <View style={styles.cardHeader}>
        <LeadingIconAvatar bg={c.primaryContainer} fg={c.onPrimaryContainer} icon={icon} />
        <View style={{ flex: 1, marginLeft: space(3) }}>
          <Text style={[type.titleMedium, { color: c.onSurface }]}>{typeLabel}</Text>
          <Text style={[type.bodySmall, { color: c.onSurfaceVariant, marginTop: 2 }]}>
            {source} · {timestamp} · {size}
          </Text>
        </View>
        <Pressable
          style={({ hovered }: any) => [styles.iconBtnSmall, hovered && stateLayer(c, 'hover')]}
        >
          <MoreVertical size={20} color={c.onSurfaceVariant} strokeWidth={2} />
        </Pressable>
      </View>

      <Text style={[type.bodyLarge, { color: c.onSurface, marginTop: space(3) }]}>{preview}</Text>

      <View style={styles.cardActionRow}>
        <FilledTonalButton
          c={c}
          icon={<Copy size={18} color={c.onPrimaryContainer} strokeWidth={2} />}
          label="复制"
        />
        <View style={{ width: space(2) }} />
        <TextButton
          c={c}
          icon={<Scissors size={18} color={c.primary} strokeWidth={2} />}
          label="分词"
        />
        <View style={{ flex: 1 }} />
        <SmallChip c={c} label="✓ 已同步" tone="success" />
      </View>
    </View>
  );
}

function ClipboardCardImage({
  c,
  imageUrl,
  type: typeLabel,
  timestamp,
  size,
  fileName,
  source,
}: {
  c: M3Scheme;
  imageUrl: string;
  type: string;
  timestamp: string;
  size: string;
  fileName: string;
  source: string;
}) {
  return (
    <View
      style={[
        styles.cardFilled,
        { backgroundColor: c.surfaceContainerLow, padding: 0, overflow: 'hidden' },
      ]}
    >
      <Image
        source={{ uri: imageUrl }}
        style={{ width: '100%', height: 200, backgroundColor: c.surfaceContainerHighest }}
        resizeMode="cover"
      />
      <View style={{ padding: space(4) }}>
        <View style={styles.cardHeader}>
          <LeadingIconAvatar
            bg={c.primaryContainer}
            fg={c.onPrimaryContainer}
            icon={<ImageIcon size={20} color={c.onPrimaryContainer} strokeWidth={2} />}
          />
          <View style={{ flex: 1, marginLeft: space(3) }}>
            <Text style={[type.titleMedium, { color: c.onSurface }]} numberOfLines={1}>
              {fileName}
            </Text>
            <Text style={[type.bodySmall, { color: c.onSurfaceVariant, marginTop: 2 }]}>
              {source} · {typeLabel} · {timestamp} · {size}
            </Text>
          </View>
        </View>

        <View style={styles.cardActionRow}>
          <FilledTonalButton
            c={c}
            icon={<Cloud size={18} color={c.onPrimaryContainer} strokeWidth={2} />}
            label="上传"
          />
          <View style={{ width: space(2) }} />
          <OutlinedButton
            c={c}
            icon={<Share2 size={18} color={c.primary} strokeWidth={2} />}
            label="分享"
          />
          <View style={{ flex: 1 }} />
          <SmallChip c={c} label="待同步" tone="pending" />
        </View>
      </View>
    </View>
  );
}

function ErrorCard({ c }: { c: M3Scheme }) {
  return (
    <View style={[styles.cardFilled, { backgroundColor: c.errorContainer }]}>
      <View style={styles.cardHeader}>
        <View
          style={{
            width: 40,
            height: 40,
            borderRadius: shape.full,
            backgroundColor: alpha(c.onErrorContainer, 0.12),
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <AlertCircle size={20} color={c.onErrorContainer} strokeWidth={2} />
        </View>
        <View style={{ flex: 1, marginLeft: space(3) }}>
          <Text style={[type.titleMedium, { color: c.onErrorContainer }]}>上传失败</Text>
          <Text style={[type.bodySmall, { color: c.onErrorContainer, marginTop: 2 }]}>
            网络连接异常 · 3 秒前
          </Text>
        </View>
      </View>

      <Text style={[type.bodyMedium, { color: c.onErrorContainer, marginTop: space(3) }]}>
        Connection refused (errno: ECONNREFUSED). 请检查服务器地址与网络连接。
      </Text>

      <View style={styles.cardActionRow}>
        <TextButton
          c={c}
          icon={<Copy size={18} color={c.onErrorContainer} strokeWidth={2} />}
          label="复制错误"
          color={c.onErrorContainer}
        />
        <View style={{ flex: 1 }} />
        <FilledTonalButton
          c={c}
          icon={<RefreshCw size={18} color={c.onErrorContainer} strokeWidth={2} />}
          label="重试"
          bg={alpha(c.onErrorContainer, 0.12)}
          fg={c.onErrorContainer}
        />
      </View>
    </View>
  );
}

function FilterChipsRow({ c }: { c: M3Scheme }) {
  const chips = ['全部', '文本', '图片', '文件', '收藏'];
  const [active, setActive] = useState(0);
  return (
    <ScrollView
      horizontal
      showsHorizontalScrollIndicator={false}
      contentContainerStyle={{ gap: space(2), paddingHorizontal: 2 }}
    >
      {chips.map((label, i) => {
        const isActive = i === active;
        return (
          <Pressable
            key={label}
            onPress={() => setActive(i)}
            style={({ hovered, pressed }: any) => [
              styles.filterChip,
              {
                backgroundColor: isActive ? c.primaryContainer : 'transparent',
                borderColor: isActive ? 'transparent' : c.outline,
              },
              hovered && !isActive && { backgroundColor: alpha(c.onSurface, 0.04) },
              pressed && { backgroundColor: alpha(c.onSurface, 0.08) },
            ]}
          >
            {isActive && (
              <Check
                size={18}
                color={c.onPrimaryContainer}
                strokeWidth={2}
                style={{ marginRight: space(1.5) }}
              />
            )}
            <Text
              style={[
                type.labelLarge,
                { color: isActive ? c.onPrimaryContainer : c.onSurfaceVariant },
              ]}
            >
              {label}
            </Text>
          </Pressable>
        );
      })}
    </ScrollView>
  );
}

function ListSurface({ c, children }: { c: M3Scheme; children: React.ReactNode }) {
  return (
    <View style={[styles.listSurface, { backgroundColor: c.surfaceContainerLow }]}>{children}</View>
  );
}

function M3ListItem({
  c,
  leading,
  headline,
  supporting,
  trailing,
}: {
  c: M3Scheme;
  leading?: React.ReactNode;
  headline: string;
  supporting?: string;
  trailing?: React.ReactNode;
}) {
  return (
    <Pressable
      style={({ hovered, pressed }: any) => [
        styles.listItem,
        hovered && { backgroundColor: alpha(c.onSurface, 0.04) },
        pressed && { backgroundColor: alpha(c.onSurface, 0.08) },
      ]}
    >
      {leading}
      <View style={{ flex: 1, marginLeft: leading ? space(4) : 0 }}>
        <Text style={[type.bodyLarge, { color: c.onSurface }]} numberOfLines={1}>
          {headline}
        </Text>
        {supporting && (
          <Text
            style={[type.bodyMedium, { color: c.onSurfaceVariant, marginTop: 2 }]}
            numberOfLines={1}
          >
            {supporting}
          </Text>
        )}
      </View>
      {trailing && <View style={{ marginLeft: space(3) }}>{trailing}</View>}
    </Pressable>
  );
}

function Divider({ c }: { c: M3Scheme }) {
  return <View style={{ height: 1, backgroundColor: c.outlineVariant, marginLeft: 72 }} />;
}

function LeadingIconAvatar({
  bg,
  fg: _fg,
  icon,
}: {
  bg: string;
  fg: string;
  icon: React.ReactNode;
}) {
  return (
    <View
      style={{
        width: 40,
        height: 40,
        borderRadius: shape.full,
        backgroundColor: bg,
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      {icon}
    </View>
  );
}

function LeadingImageThumb({ url, radius }: { url: string; radius: number }) {
  return (
    <Image
      source={{ uri: url }}
      style={{ width: 56, height: 56, borderRadius: radius, backgroundColor: '#eee' }}
    />
  );
}

function FilledTonalButton({
  c,
  icon,
  label,
  bg,
  fg,
}: {
  c: M3Scheme;
  icon?: React.ReactNode;
  label: string;
  bg?: string;
  fg?: string;
}) {
  const background = bg ?? c.primaryContainer;
  const foreground = fg ?? c.onPrimaryContainer;
  return (
    <Pressable
      style={({ hovered, pressed }: any) => [
        styles.btn,
        { backgroundColor: background },
        hovered && { backgroundColor: blend(background, c.onPrimaryContainer, 0.08) },
        pressed && { backgroundColor: blend(background, c.onPrimaryContainer, 0.12) },
      ]}
    >
      {icon && <View style={{ marginRight: label ? space(2) : 0 }}>{icon}</View>}
      <Text style={[type.labelLarge, { color: foreground }]}>{label}</Text>
    </Pressable>
  );
}

function OutlinedButton({
  c,
  icon,
  label,
}: {
  c: M3Scheme;
  icon?: React.ReactNode;
  label: string;
}) {
  return (
    <Pressable
      style={({ hovered, pressed }: any) => [
        styles.btn,
        {
          backgroundColor: 'transparent',
          borderWidth: 1,
          borderColor: c.outline,
        },
        hovered && { backgroundColor: alpha(c.primary, 0.08) },
        pressed && { backgroundColor: alpha(c.primary, 0.12) },
      ]}
    >
      {icon && <View style={{ marginRight: label ? space(2) : 0 }}>{icon}</View>}
      <Text style={[type.labelLarge, { color: c.primary }]}>{label}</Text>
    </Pressable>
  );
}

function TextButton({
  c,
  icon,
  label,
  color,
}: {
  c: M3Scheme;
  icon?: React.ReactNode;
  label: string;
  color?: string;
}) {
  const fg = color ?? c.primary;
  return (
    <Pressable
      style={({ hovered, pressed }: any) => [
        styles.btnText,
        hovered && { backgroundColor: alpha(fg, 0.08) },
        pressed && { backgroundColor: alpha(fg, 0.12) },
      ]}
    >
      {icon && <View style={{ marginRight: label ? space(2) : 0 }}>{icon}</View>}
      <Text style={[type.labelLarge, { color: fg }]}>{label}</Text>
    </Pressable>
  );
}

function ButtonShowcase({ c }: { c: M3Scheme }) {
  return (
    <View
      style={{
        backgroundColor: c.surfaceContainer,
        borderRadius: shape.lg,
        padding: space(4),
        gap: space(3),
      }}
    >
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space(2) }}>
        <FilledTonalButton
          c={c}
          icon={<Copy size={18} color={c.onPrimaryContainer} strokeWidth={2} />}
          label="Filled Tonal"
        />
        <OutlinedButton
          c={c}
          icon={<Share2 size={18} color={c.primary} strokeWidth={2} />}
          label="Outlined"
        />
        <TextButton
          c={c}
          icon={<Scissors size={18} color={c.primary} strokeWidth={2} />}
          label="Text"
        />
      </View>
      <View style={{ flexDirection: 'row', flexWrap: 'wrap', gap: space(2) }}>
        <SmallChip c={c} label="已同步" tone="success" />
        <SmallChip c={c} label="待同步" tone="pending" />
        <SmallChip c={c} label="未下载" tone="info" />
        <SmallChip c={c} label="错误" tone="error" />
      </View>
    </View>
  );
}

function SmallChip({
  c,
  label,
  tone,
}: {
  c: M3Scheme;
  label: string;
  tone: 'success' | 'pending' | 'info' | 'error';
}) {
  // 状态色:绿(已同步) / 中性(待同步) / 主题色淡版(未下载) / 红(错误)
  // 不再使用 secondary/tertiary 实底,所有"主题色相关"统一走 primaryContainer
  const isDark = isDarkColor(c.background);
  const map = {
    success: {
      bg: isDark ? alpha('#A5D6A7', 0.16) : alpha('#1B5E20', 0.12),
      fg: isDark ? '#A5D6A7' : '#1B5E20',
    },
    pending: { bg: c.surfaceContainerHigh, fg: c.onSurfaceVariant },
    info: { bg: c.primaryContainer, fg: c.onPrimaryContainer },
    error: { bg: c.errorContainer, fg: c.onErrorContainer },
  };
  const t = map[tone];
  return (
    <View
      style={{
        paddingHorizontal: space(3),
        paddingVertical: space(1.5),
        borderRadius: shape.sm,
        backgroundColor: t.bg,
      }}
    >
      <Text style={[type.labelMedium, { color: t.fg }]}>{label}</Text>
    </View>
  );
}

function ColorPicker({
  c,
  value,
  onChange,
}: {
  c: M3Scheme;
  value: PaletteId;
  onChange: (v: PaletteId) => void;
}) {
  return (
    <View style={styles.colorPickerWrap}>
      <Text
        style={[
          type.labelMedium,
          { color: c.onSurfaceVariant, marginBottom: space(2), marginLeft: space(1) },
        ]}
      >
        主题色 · SOURCE COLOR
      </Text>
      <View style={{ flexDirection: 'row', gap: space(3) }}>
        {PALETTES.map((p) => {
          const active = p.id === value;
          return (
            <Pressable
              key={p.id}
              onPress={() => onChange(p.id)}
              style={({ hovered, pressed }: any) => [
                styles.colorSwatchWrap,
                hovered && { transform: [{ scale: 1.05 }] },
                pressed && { transform: [{ scale: 0.95 }] },
              ]}
            >
              <View
                style={[
                  styles.colorSwatchRing,
                  {
                    borderColor: active ? p.swatch : 'transparent',
                  },
                ]}
              >
                <View style={[styles.colorSwatch, { backgroundColor: p.swatch }]}>
                  {active && <Check size={20} color="#FFFFFF" strokeWidth={3} />}
                </View>
              </View>
              <Text
                style={[
                  type.labelMedium,
                  {
                    color: active ? c.onSurface : c.onSurfaceVariant,
                    marginTop: space(1.5),
                    fontWeight: active ? '600' : '400',
                  },
                ]}
              >
                {p.label}
              </Text>
            </Pressable>
          );
        })}
      </View>
    </View>
  );
}

function ExtendedFAB({ c }: { c: M3Scheme }) {
  return (
    <View style={styles.fabWrap} pointerEvents="box-none">
      <Pressable
        style={({ hovered, pressed }: any) => [
          styles.fab,
          { backgroundColor: c.primaryContainer },
          hovered && {
            backgroundColor: blend(c.primaryContainer, c.onPrimaryContainer, 0.08),
          },
          pressed && {
            backgroundColor: blend(c.primaryContainer, c.onPrimaryContainer, 0.12),
          },
        ]}
      >
        <Plus size={20} color={c.onPrimaryContainer} strokeWidth={2.5} />
        <Text style={[type.labelLarge, { color: c.onPrimaryContainer, marginLeft: space(2) }]}>
          新建
        </Text>
      </Pressable>
    </View>
  );
}

function TonalPaletteGrid({ c }: { c: M3Scheme }) {
  const swatches: { key: keyof M3Scheme; label: string }[] = [
    { key: 'primary', label: 'primary' },
    { key: 'primaryContainer', label: 'primaryContainer' },
    { key: 'errorContainer', label: 'errorContainer' },
    { key: 'surfaceContainerLowest', label: 'surfaceContainerLowest' },
    { key: 'surfaceContainerLow', label: 'surfaceContainerLow' },
    { key: 'surfaceContainer', label: 'surfaceContainer' },
    { key: 'surfaceContainerHigh', label: 'surfaceContainerHigh' },
    { key: 'surfaceContainerHighest', label: 'surfaceContainerHighest' },
  ];
  return (
    <View
      style={{ borderRadius: shape.lg, overflow: 'hidden', backgroundColor: c.surfaceContainerLow }}
    >
      {swatches.map((s) => {
        const isOnLight = !isDarkColor(c[s.key]);
        const textColor = isOnLight ? '#1D1B20' : '#FFFFFF';
        return (
          <View
            key={s.key}
            style={{
              padding: space(4),
              backgroundColor: c[s.key],
              flexDirection: 'row',
              alignItems: 'center',
            }}
          >
            <Text style={[type.labelLarge, { color: textColor, flex: 1 }]}>{s.label}</Text>
            <Text
              style={[type.bodySmall, { color: alpha(textColor, 0.7), fontFamily: 'monospace' }]}
            >
              {c[s.key]}
            </Text>
          </View>
        );
      })}
    </View>
  );
}

// ------------------------------------------------------------------
// Color helpers
// ------------------------------------------------------------------

function alpha(hex: string, a: number): string {
  // accept #RRGGBB or rgba; if rgba, return as-is with adjusted alpha
  if (hex.startsWith('rgb')) return hex;
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

function blend(base: string, overlay: string, opacity: number): string {
  if (!base.startsWith('#') || !overlay.startsWith('#')) return base;
  const br = parseInt(base.slice(1, 3), 16);
  const bg = parseInt(base.slice(3, 5), 16);
  const bb = parseInt(base.slice(5, 7), 16);
  const or = parseInt(overlay.slice(1, 3), 16);
  const og = parseInt(overlay.slice(3, 5), 16);
  const ob = parseInt(overlay.slice(5, 7), 16);
  const r = Math.round(br * (1 - opacity) + or * opacity);
  const g = Math.round(bg * (1 - opacity) + og * opacity);
  const b = Math.round(bb * (1 - opacity) + ob * opacity);
  return `rgb(${r}, ${g}, ${b})`;
}

function isDarkColor(hex: string): boolean {
  if (!hex.startsWith('#')) return false;
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  const luma = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return luma < 0.5;
}

function stateLayer(c: M3Scheme, state: 'hover' | 'pressed'): ViewStyle {
  return { backgroundColor: alpha(c.onSurface, state === 'hover' ? 0.04 : 0.08) };
}

// ------------------------------------------------------------------
// Styles
// ------------------------------------------------------------------

const styles = StyleSheet.create({
  root: {
    flex: 1,
  },
  scroll: {
    paddingBottom: space(8),
    maxWidth: 720,
    width: '100%',
    alignSelf: 'center',
  },
  topBar: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: space(2),
    paddingVertical: space(2),
    minHeight: 64,
  },
  iconBtn: {
    width: 40,
    height: 40,
    borderRadius: shape.full,
    alignItems: 'center',
    justifyContent: 'center',
  },
  iconBtnSmall: {
    width: 32,
    height: 32,
    borderRadius: shape.full,
    alignItems: 'center',
    justifyContent: 'center',
  },
  brandBlock: {
    paddingHorizontal: space(5),
    paddingTop: space(4),
    paddingBottom: space(6),
  },
  segmented: {
    flexDirection: 'row',
    marginHorizontal: space(5),
    borderRadius: shape.full,
    borderWidth: 1,
    overflow: 'hidden',
    marginBottom: space(6),
  },
  segmentedItem: {
    flex: 1,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    paddingVertical: space(2.5),
    paddingHorizontal: space(3),
  },
  sectionLabel: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: space(5),
    marginTop: space(6),
    marginBottom: space(3),
  },
  cardFilled: {
    borderRadius: shape.lg,
    padding: space(4),
    marginHorizontal: space(5),
  },
  cardHeader: {
    flexDirection: 'row',
    alignItems: 'center',
  },
  cardActionRow: {
    flexDirection: 'row',
    alignItems: 'center',
    marginTop: space(4),
  },
  listSurface: {
    borderRadius: shape.lg,
    marginHorizontal: space(5),
    paddingVertical: space(2),
    overflow: 'hidden',
  },
  listItem: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: space(4),
    paddingVertical: space(3),
    minHeight: 72,
  },
  filterChip: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: space(4),
    paddingVertical: space(2),
    borderRadius: shape.sm,
    borderWidth: 1,
    marginLeft: space(5),
  },
  btn: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: space(6),
    paddingVertical: space(2.5),
    borderRadius: shape.full,
    minHeight: 40,
  },
  btnText: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: space(3),
    paddingVertical: space(2.5),
    borderRadius: shape.full,
    minHeight: 40,
  },
  colorPickerWrap: {
    marginHorizontal: space(5),
    marginBottom: space(6),
  },
  colorSwatchWrap: {
    alignItems: 'center',
  },
  colorSwatchRing: {
    width: 56,
    height: 56,
    borderRadius: shape.full,
    borderWidth: 3,
    padding: 3,
    alignItems: 'center',
    justifyContent: 'center',
  },
  colorSwatch: {
    width: '100%',
    height: '100%',
    borderRadius: shape.full,
    alignItems: 'center',
    justifyContent: 'center',
  },
  fabWrap: {
    position: 'absolute',
    right: space(5),
    bottom: space(5),
  },
  fab: {
    flexDirection: 'row',
    alignItems: 'center',
    paddingHorizontal: space(4),
    paddingVertical: space(4),
    borderRadius: shape.lg,
    minHeight: 56,
    // M3 elevation 3 (web 用 shadow 兼容)
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 4 },
    shadowOpacity: 0.15,
    shadowRadius: 8,
    elevation: 6,
  },
});
