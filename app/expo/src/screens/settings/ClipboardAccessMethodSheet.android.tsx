/**
 * 后台读取 Bottom Sheet(Android):方式选择 / ADB 授权 / Shizuku 设置 / MIUI 限制。
 *
 * 与原仓库 UniClip 保持一致的 M3 Expressive 设计:
 * - 方式选择是竖向单选列表，三种方式一屏可见；选中项展开细节，底部固定「继续」按钮，
 *   选择不会立即生效（避免误触），按钮在选中当前方式时置灰。
 * - ADB / Shizuku 引导是纵向步骤，命令放在终端块里，带 48dp 的复制按钮（经 Engine 原生写入）。
 * 所有面板共用同一个 AppBottomSheet，内容靠 reducer 里的 content 类型切换。
 */
import { Ionicons } from '@expo/vector-icons';
import {
  createContext,
  memo,
  useCallback,
  useContext,
  useMemo,
  useReducer,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';
import {
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
  type ColorValue,
} from 'react-native';
import { AppBottomSheet } from '@/components/ui';
import { unicEngineNative } from '@/engine/nativeModule';
import type { ClipboardAccessMethod } from '@/engine/types';
import { useTheme } from '@/hooks/useTheme';
import { ADB_DEVICES_COMMAND } from '@/utils/backgroundClipboardAccess';
import {
  clipboardAccessSheetReducer,
  getSheetContentMaxHeight,
  INITIAL_CLIPBOARD_ACCESS_SHEET_STATE,
} from './ClipboardAccessMethodSheet.state';
import type {
  AdbSetupStage,
  ClipboardAccessMethodSheetController,
  OpenAdbSetupSheetOptions,
  OpenClipboardAccessMethodSheetOptions,
  OpenClipboardRestrictionSheetOptions,
  OpenShizukuSetupSheetOptions,
  ShizukuSetupStage,
} from './ClipboardAccessMethodSheet.types';

const METHODS: ClipboardAccessMethod[] = ['overlay-polling', 'overlay-event', 'shizuku'];

type IoniconName = ComponentProps<typeof Ionicons>['name'];

const ClipboardAccessMethodSheetContext =
  createContext<ClipboardAccessMethodSheetController | null>(null);

export function useClipboardAccessMethodSheet(): ClipboardAccessMethodSheetController {
  const controller = useContext(ClipboardAccessMethodSheetContext);
  if (!controller) {
    throw new Error('useClipboardAccessMethodSheet must be used inside its provider');
  }
  return controller;
}

export function ClipboardAccessMethodSheetProvider({ children }: { children: ReactNode }) {
  const { theme } = useTheme();
  const [{ visible, content, isSelecting }, dispatch] = useReducer(
    clipboardAccessSheetReducer,
    INITIAL_CLIPBOARD_ACCESS_SHEET_STATE
  );
  const selectingRef = useRef(false);

  const openMethodSheet = useCallback((options: OpenClipboardAccessMethodSheetOptions) => {
    dispatch({ type: 'open-methods', options });
  }, []);
  const openAdbSetupSheet = useCallback((options: OpenAdbSetupSheetOptions) => {
    dispatch({ type: 'open-adb', options });
  }, []);
  const openShizukuSetupSheet = useCallback((options: OpenShizukuSetupSheetOptions) => {
    dispatch({ type: 'open-shizuku', options });
  }, []);
  const openRestrictionSheet = useCallback((options: OpenClipboardRestrictionSheetOptions) => {
    dispatch({ type: 'open-restriction', options });
  }, []);
  const closeSheet = useCallback(() => dispatch({ type: 'close' }), []);

  const controller = useMemo(
    () => ({
      openMethodSheet,
      openAdbSetupSheet,
      openShizukuSetupSheet,
      openRestrictionSheet,
      closeSheet,
    }),
    [closeSheet, openAdbSetupSheet, openMethodSheet, openRestrictionSheet, openShizukuSetupSheet]
  );

  const handleSelect = useCallback(
    async (method: ClipboardAccessMethod) => {
      if (content?.type !== 'methods' || selectingRef.current) return;
      selectingRef.current = true;
      dispatch({ type: 'selection-started' });
      const onSelect = content.onSelect;
      try {
        await onSelect(method);
      } finally {
        selectingRef.current = false;
        dispatch({ type: 'selection-finished' });
      }
    },
    [content]
  );

  return (
    <ClipboardAccessMethodSheetContext.Provider value={controller}>
      {children}
      <AppBottomSheet
        visible={visible}
        onDismiss={closeSheet}
        containerColor={theme.colors.surfaceMid}
      >
        {content?.type === 'methods' ? (
          <MethodChooserSheet
            key={content.selectedMethod}
            currentMethod={content.selectedMethod}
            isSelecting={isSelecting}
            onSelect={handleSelect}
            onClose={closeSheet}
          />
        ) : null}
        {content?.type === 'adb' ? (
          <AdbSetupSheet
            stage={content.stage}
            command={content.command}
            onCopy={content.onCopy}
            onCheck={content.onCheck}
            onClose={closeSheet}
          />
        ) : null}
        {content?.type === 'shizuku' ? (
          <ShizukuSetupSheet
            stage={content.stage}
            command={content.command}
            onAction={content.onAction}
            onCheck={content.onCheck}
            onClose={closeSheet}
          />
        ) : null}
        {content?.type === 'restriction' ? (
          <RestrictionSheet onConfirm={content.onConfirm} onClose={closeSheet} />
        ) : null}
      </AppBottomSheet>
    </ClipboardAccessMethodSheetContext.Provider>
  );
}

// ---------------------------------------------------------------------------
// Method chooser
// ---------------------------------------------------------------------------

interface MethodChooserSheetProps {
  currentMethod: ClipboardAccessMethod;
  isSelecting: boolean;
  onSelect: (method: ClipboardAccessMethod) => void;
  onClose: () => void;
}

const MethodChooserSheet = memo(function MethodChooserSheet({
  currentMethod,
  isSelecting,
  onSelect,
  onClose,
}: MethodChooserSheetProps) {
  const { t } = useTranslation('settingsBackground');
  const { height: windowHeight } = useWindowDimensions();
  const [selected, setSelected] = useState(currentMethod);
  const isCurrent = selected === currentMethod;

  return (
    <View style={[styles.container, { maxHeight: getSheetContentMaxHeight(windowHeight) }]}>
      <SheetHeader
        title={t('advanced.clipboardAccess.sheet.title')}
        subtitle={t('advanced.clipboardAccess.sheet.subtitle')}
        onClose={onClose}
      />
      <ScrollView
        nestedScrollEnabled
        showsVerticalScrollIndicator={false}
        style={styles.scroll}
        contentContainerStyle={styles.cardList}
      >
        {METHODS.map((method) => (
          <MethodCard
            key={method}
            method={method}
            selected={method === selected}
            current={method === currentMethod}
            onPress={setSelected}
          />
        ))}
      </ScrollView>
      <SheetFooter>
        <PrimaryButton
          testID="clipboard-method-confirm"
          icon={isCurrent ? 'checkmark' : 'chevron-forward'}
          label={
            isCurrent
              ? t('advanced.clipboardAccess.sheet.currentButton')
              : t(`advanced.clipboardAccess.sheet.continue.${selected}`)
          }
          disabled={isCurrent || isSelecting}
          onPress={() => onSelect(selected)}
        />
        {selected === 'overlay-event' && !isCurrent ? (
          <FooterHint text={t('advanced.clipboardAccess.sheet.nextHint.overlay-event')} />
        ) : null}
      </SheetFooter>
    </View>
  );
});

interface MethodCardProps {
  method: ClipboardAccessMethod;
  selected: boolean;
  current: boolean;
  onPress: (method: ClipboardAccessMethod) => void;
}

const MethodCard = memo(function MethodCard({
  method,
  selected,
  current,
  onPress,
}: MethodCardProps) {
  const { t } = useTranslation('settingsBackground');
  const { theme } = useTheme();
  const prefix = `advanced.clipboardAccess.sheet.methods.${method}`;
  const radioColor = selected ? theme.colors.accent : theme.colors.border;

  return (
    <Pressable
      testID={`clipboard-method-${method}`}
      onPress={() => onPress(method)}
      accessibilityRole="radio"
      accessibilityState={{ selected }}
      accessibilityLabel={t(`${prefix}.title`)}
      android_ripple={{ color: theme.colors.fillSecondary as string }}
      style={[
        styles.methodCard,
        selected ? styles.methodCardSelected : styles.methodCardIdle,
        {
          backgroundColor: theme.colors.surface,
          borderColor: selected ? theme.colors.accent : theme.colors.separator,
        },
      ]}
    >
      <View style={[styles.radio, { borderColor: radioColor }]}>
        {selected ? (
          <View style={[styles.radioDot, { backgroundColor: theme.colors.accent }]} />
        ) : null}
      </View>
      <View style={[styles.methodIcon, { backgroundColor: theme.colors.accentContainer }]}>
        <Ionicons name={getMethodIcon(method)} size={22} color={theme.colors.onAccentContainer} />
      </View>
      <View style={styles.methodBody}>
        <View style={styles.methodTitleRow}>
          <Text style={[styles.methodTitle, { color: theme.colors.textPrimary }]}>
            {t(`${prefix}.title`)}
          </Text>
          {current ? (
            <Tag
              label={t('advanced.clipboardAccess.sheet.inUse')}
              background={theme.colors.surfaceHighest}
              color={theme.colors.textPrimary}
            />
          ) : null}
        </View>
        <Text style={[styles.methodSummary, { color: theme.colors.textSecondary }]}>
          {t(`${prefix}.summary`)}
        </Text>
        {selected ? (
          <View style={[styles.detailList, { borderTopColor: theme.colors.separator }]}>
            {(['bestFor', 'tradeoff', 'setup'] as const).map((field) => (
              <View key={field}>
                <Text style={[styles.detailLabel, { color: theme.colors.accent }]}>
                  {t(`advanced.clipboardAccess.sheet.labels.${field}`)}
                </Text>
                <Text style={[styles.detailValue, { color: theme.colors.textPrimary }]}>
                  {t(`${prefix}.${field}`)}
                </Text>
              </View>
            ))}
          </View>
        ) : (
          <View style={styles.chipRow}>
            {(['tag1', 'tag2'] as const).map((tag) => (
              <Tag
                key={tag}
                label={t(`${prefix}.${tag}`)}
                background={theme.colors.surfaceHighest}
                color={theme.colors.textPrimary}
                chip
              />
            ))}
          </View>
        )}
      </View>
    </Pressable>
  );
});

function Tag({
  label,
  background,
  color,
  chip = false,
}: {
  label: string;
  background: ColorValue;
  color: ColorValue;
  chip?: boolean;
}) {
  return (
    <View style={[styles.tag, chip && styles.tagChip, { backgroundColor: background }]}>
      <Text style={[styles.tagText, { color }]}>{label}</Text>
    </View>
  );
}

// ---------------------------------------------------------------------------
// ADB guide
// ---------------------------------------------------------------------------

interface AdbSetupSheetProps {
  stage: AdbSetupStage;
  command: string;
  onCopy: () => void;
  onCheck: () => void;
  onClose: () => void;
}

const AdbSetupSheet = memo(function AdbSetupSheet({
  stage,
  command,
  onCopy,
  onCheck,
  onClose,
}: AdbSetupSheetProps) {
  const { t } = useTranslation('settingsBackground');
  const { height: windowHeight } = useWindowDimensions();
  const [devicesCopied, setDevicesCopied] = useState(false);
  const [grantCopied, setGrantCopied] = useState(stage === 'copied');
  const prefix = 'advanced.clipboardAccess.adbGuide';

  const copyDevices = useCallback(async () => {
    await unicEngineNative.copyPlainText(ADB_DEVICES_COMMAND);
    setDevicesCopied(true);
  }, []);
  const copyGrant = useCallback(() => {
    setGrantCopied(true);
    onCopy();
  }, [onCopy]);

  return (
    <View style={[styles.container, { maxHeight: getSheetContentMaxHeight(windowHeight) }]}>
      <SheetHeader
        title={t(`${prefix}.title`)}
        subtitle={t(`${prefix}.subtitle`)}
        onClose={onClose}
      />
      {stage === 'copied' ? (
        <Banner tone="info" icon="information-circle-outline" text={t(`${prefix}.status.copied`)} />
      ) : null}
      {stage === 'notDetected' ? (
        <Banner
          tone="warning"
          icon="alert-circle-outline"
          text={t(`${prefix}.status.notDetected`)}
        />
      ) : null}
      <ScrollView
        nestedScrollEnabled
        showsVerticalScrollIndicator={false}
        style={styles.scroll}
        contentContainerStyle={styles.stepsContent}
      >
        <Step index={1} title={t(`${prefix}.step1.title`)}>
          <StepText text={t(`${prefix}.step1.text`)} />
        </Step>
        <Step index={2} title={t(`${prefix}.step2.title`)}>
          <StepText text={t(`${prefix}.step2.text`)} />
          <CommandBlock
            command={ADB_DEVICES_COMMAND}
            copied={devicesCopied}
            onCopy={() => void copyDevices()}
          />
        </Step>
        <Step index={3} title={t(`${prefix}.step3.title`)} last>
          <StepText text={t(`${prefix}.step3.text`)} />
          <CommandBlock command={command} copied={grantCopied} onCopy={copyGrant} />
        </Step>
      </ScrollView>
      <SheetFooter>
        <PrimaryButton
          testID="adb-guide-check"
          icon="checkmark"
          label={t(stage === 'notDetected' ? `${prefix}.checkAgain` : `${prefix}.check`)}
          onPress={onCheck}
        />
        {stage === 'instructions' || stage === 'notDetected' ? (
          <TextActionButton
            icon="copy-outline"
            label={t(stage === 'notDetected' ? `${prefix}.copyAgain` : `${prefix}.copyCommand`)}
            onPress={copyGrant}
          />
        ) : null}
      </SheetFooter>
    </View>
  );
});

// ---------------------------------------------------------------------------
// Shizuku guide
// ---------------------------------------------------------------------------

interface ShizukuSetupSheetProps {
  stage: ShizukuSetupStage;
  command: string;
  onAction: () => void;
  onCheck: () => void;
  onClose: () => void;
}

const ShizukuSetupSheet = memo(function ShizukuSetupSheet({
  stage,
  command,
  onAction,
  onCheck,
  onClose,
}: ShizukuSetupSheetProps) {
  const { t } = useTranslation('settingsBackground');
  const { height: windowHeight } = useWindowDimensions();
  const [commandCopied, setCommandCopied] = useState(false);
  const prefix = 'advanced.clipboardAccess.shizukuGuide';
  const authorize = stage === 'authorize';

  const copyCommand = useCallback(async () => {
    await unicEngineNative.copyPlainText(command);
    setCommandCopied(true);
  }, [command]);

  return (
    <View style={[styles.container, { maxHeight: getSheetContentMaxHeight(windowHeight) }]}>
      <SheetHeader
        title={t(`${prefix}.title`)}
        subtitle={t(`${prefix}.subtitle`)}
        onClose={onClose}
      />
      <Banner
        tone={authorize ? 'info' : 'warning'}
        icon={authorize ? 'information-circle-outline' : 'alert-circle-outline'}
        text={t(`${prefix}.status.${stage}`)}
      />
      <ScrollView
        nestedScrollEnabled
        showsVerticalScrollIndicator={false}
        style={styles.scroll}
        contentContainerStyle={styles.stepsContent}
      >
        <Step index={1} title={t(`${prefix}.step1.title`)} done>
          <StepText text={t(authorize ? `${prefix}.step1.done` : `${prefix}.step1.text`)} />
        </Step>
        <Step index={2} title={t(`${prefix}.step2.title`)} done={authorize}>
          {authorize ? (
            <StepText text={t(`${prefix}.step2.done`)} />
          ) : (
            <>
              <StepText text={t(`${prefix}.step2.text`)} />
              <CommandBlock
                command={command}
                copied={commandCopied}
                onCopy={() => void copyCommand()}
              />
            </>
          )}
        </Step>
        <Step index={3} title={t(`${prefix}.step3.title`)} last>
          <StepText
            text={t(authorize ? `${prefix}.step3.authorizeText` : `${prefix}.step3.text`)}
          />
        </Step>
      </ScrollView>
      <SheetFooter>
        <PrimaryButton
          testID="shizuku-guide-action"
          icon={authorize ? 'lock-closed-outline' : 'chevron-forward'}
          label={t(authorize ? `${prefix}.authorize` : `${prefix}.open`)}
          onPress={onAction}
        />
        <TextActionButton icon="checkmark" label={t(`${prefix}.checkAgain`)} onPress={onCheck} />
      </SheetFooter>
    </View>
  );
});

// ---------------------------------------------------------------------------
// MIUI restriction
// ---------------------------------------------------------------------------

const RestrictionSheet = memo(function RestrictionSheet({
  onConfirm,
  onClose,
}: OpenClipboardRestrictionSheetOptions & { onClose: () => void }) {
  const { t } = useTranslation('settingsBackground');
  const { theme } = useTheme();
  const prefix = 'advanced.clipboardAccess.restriction';

  return (
    <View style={styles.container}>
      <SheetHeader
        title={t(`${prefix}.title`)}
        subtitle={t(`${prefix}.subtitle`)}
        onClose={onClose}
      />
      <Banner tone="warning" icon="alert-circle-outline" text={t(`${prefix}.warning`)} />
      <Text style={[styles.restrictionText, { color: theme.colors.textSecondary }]}>
        {t(`${prefix}.text`)}
      </Text>
      <SheetFooter>
        <PrimaryButton label={t(`${prefix}.confirm`)} onPress={onConfirm} />
        <TextActionButton label={t(`${prefix}.dismiss`)} onPress={onClose} />
      </SheetFooter>
    </View>
  );
});

// ---------------------------------------------------------------------------
// Shared building blocks
// ---------------------------------------------------------------------------

function SheetHeader({
  title,
  subtitle,
  onClose,
}: {
  title: string;
  subtitle: string;
  onClose: () => void;
}) {
  const { t } = useTranslation('common');
  const { theme } = useTheme();
  return (
    <View style={styles.header}>
      <View style={styles.headerTextGroup}>
        <Text style={[styles.sheetTitle, { color: theme.colors.textPrimary }]}>{title}</Text>
        <Text style={[styles.sheetSubtitle, { color: theme.colors.textSecondary }]}>
          {subtitle}
        </Text>
      </View>
      <Pressable
        onPress={onClose}
        android_ripple={{
          color: theme.colors.fillSecondary as string,
          borderless: true,
        }}
        accessibilityRole="button"
        accessibilityLabel={t('action.close')}
        style={styles.closeButton}
      >
        <Ionicons name="close" size={24} color={theme.colors.textSecondary} />
      </Pressable>
    </View>
  );
}

function SheetFooter({ children }: { children: ReactNode }) {
  const { theme } = useTheme();
  return (
    <View style={[styles.footer, { borderTopColor: theme.colors.separator }]}>{children}</View>
  );
}

function FooterHint({ text }: { text: string }) {
  const { theme } = useTheme();
  return <Text style={[styles.footerHint, { color: theme.colors.textSecondary }]}>{text}</Text>;
}

function PrimaryButton({
  testID,
  icon,
  label,
  disabled = false,
  onPress,
}: {
  testID?: string;
  icon?: IoniconName;
  label: string;
  disabled?: boolean;
  onPress: () => void;
}) {
  const { theme } = useTheme();
  const foreground = disabled ? theme.colors.textDisabled : theme.colors.onAccent;
  return (
    <Pressable
      testID={testID}
      onPress={onPress}
      disabled={disabled}
      accessibilityRole="button"
      accessibilityState={{ disabled }}
      style={({ pressed }) => [
        styles.primaryButton,
        {
          backgroundColor: disabled ? theme.colors.surfaceHighest : theme.colors.accent,
          opacity: pressed ? 0.72 : 1,
        },
      ]}
    >
      {icon ? <Ionicons name={icon} size={20} color={foreground} /> : null}
      <Text style={[styles.primaryButtonText, { color: foreground }]}>{label}</Text>
    </Pressable>
  );
}

function TextActionButton({
  icon,
  label,
  onPress,
}: {
  icon?: IoniconName;
  label: string;
  onPress: () => void;
}) {
  const { theme } = useTheme();
  return (
    <Pressable
      onPress={onPress}
      android_ripple={{ color: theme.colors.fillSecondary as string }}
      accessibilityRole="button"
      style={styles.textButton}
    >
      {icon ? <Ionicons name={icon} size={18} color={theme.colors.accent} /> : null}
      <Text style={[styles.textButtonText, { color: theme.colors.accent }]}>{label}</Text>
    </Pressable>
  );
}

function Banner({
  tone,
  icon,
  text,
}: {
  tone: 'info' | 'warning';
  icon: IoniconName;
  text: string;
}) {
  const { theme } = useTheme();
  const background = tone === 'info' ? theme.colors.accentContainer : theme.colors.warningContainer;
  const foreground =
    tone === 'info' ? theme.colors.onAccentContainer : theme.colors.onWarningContainer;
  return (
    <View style={[styles.banner, { backgroundColor: background }]}>
      <Ionicons name={icon} size={20} color={foreground} style={styles.bannerIcon} />
      <Text style={[styles.bannerText, { color: foreground }]}>{text}</Text>
    </View>
  );
}

function Step({
  index,
  title,
  done = false,
  last = false,
  children,
}: {
  index: number;
  title: string;
  done?: boolean;
  last?: boolean;
  children: ReactNode;
}) {
  const { theme } = useTheme();
  return (
    <View style={styles.step}>
      <View style={styles.stepRail}>
        <View
          style={[
            styles.stepDot,
            {
              backgroundColor: done ? theme.colors.accent : theme.colors.accentContainer,
            },
          ]}
        >
          {done ? (
            <Ionicons name="checkmark" size={16} color={theme.colors.onAccent} />
          ) : (
            <Text style={[styles.stepDotText, { color: theme.colors.onAccentContainer }]}>
              {index}
            </Text>
          )}
        </View>
        {last ? null : (
          <View style={[styles.stepLine, { backgroundColor: theme.colors.separator }]} />
        )}
      </View>
      <View style={[styles.stepBody, !last && styles.stepBodySpaced]}>
        <Text style={[styles.stepTitle, { color: theme.colors.textPrimary }]}>{title}</Text>
        {children}
      </View>
    </View>
  );
}

function StepText({ text }: { text: string }) {
  const { theme } = useTheme();
  return <Text style={[styles.stepText, { color: theme.colors.textSecondary }]}>{text}</Text>;
}

/** 终端风格命令块:等宽字体、可选中,右上角 48dp 复制按钮(复制后变为「已复制」)。 */
function CommandBlock({
  command,
  copied,
  onCopy,
}: {
  command: string;
  copied: boolean;
  onCopy: () => void;
}) {
  const { t } = useTranslation('settingsBackground');
  const { theme } = useTheme();
  const prefix = 'advanced.clipboardAccess.adbGuide';
  return (
    <View style={[styles.commandBlock, { backgroundColor: theme.colors.inverseSurface }]}>
      <View style={styles.commandToolbar}>
        <Text style={[styles.commandLabel, { color: theme.colors.inverseOnSurface }]}>
          {t(`${prefix}.terminalLabel`)}
        </Text>
        <Pressable
          onPress={onCopy}
          accessibilityRole="button"
          accessibilityLabel={t(copied ? `${prefix}.copied` : `${prefix}.copy`)}
          style={[
            styles.copyButton,
            {
              backgroundColor: copied
                ? theme.colors.accentContainer
                : theme.colors.inverseOnSurface,
            },
          ]}
        >
          <Ionicons
            name={copied ? 'checkmark' : 'copy-outline'}
            size={18}
            color={copied ? theme.colors.onAccentContainer : theme.colors.inverseSurface}
          />
          <Text
            style={[
              styles.copyButtonText,
              {
                color: copied ? theme.colors.onAccentContainer : theme.colors.inverseSurface,
              },
            ]}
          >
            {t(copied ? `${prefix}.copied` : `${prefix}.copy`)}
          </Text>
        </Pressable>
      </View>
      <View style={styles.commandLine}>
        <Text style={[styles.commandPrompt, { color: theme.colors.inverseAccent }]}>$</Text>
        <Text selectable style={[styles.commandText, { color: theme.colors.inverseOnSurface }]}>
          {command}
        </Text>
      </View>
    </View>
  );
}

function getMethodIcon(method: ClipboardAccessMethod): IoniconName {
  if (method === 'overlay-polling') return 'timer-outline';
  if (method === 'overlay-event') return 'terminal-outline';
  return 'layers-outline';
}

const styles = StyleSheet.create({
  container: { flexShrink: 1 },
  scroll: { flexShrink: 1 },
  header: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    paddingHorizontal: 24,
    paddingTop: 4,
    paddingBottom: 12,
    gap: 12,
  },
  headerTextGroup: { flex: 1, gap: 4 },
  sheetTitle: {
    fontSize: 22,
    lineHeight: 28,
    fontWeight: '500',
    letterSpacing: 0,
  },
  sheetSubtitle: { fontSize: 14, lineHeight: 20, letterSpacing: 0 },
  closeButton: {
    width: 48,
    height: 48,
    borderRadius: 24,
    alignItems: 'center',
    justifyContent: 'center',
    marginTop: -8,
    marginRight: -8,
  },
  cardList: {
    paddingHorizontal: 16,
    paddingTop: 4,
    paddingBottom: 16,
    gap: 10,
  },
  methodCard: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    gap: 12,
    borderRadius: 16,
    overflow: 'hidden',
  },
  methodCardIdle: { borderWidth: 1, padding: 16 },
  methodCardSelected: { borderWidth: 2, padding: 15 },
  radio: {
    width: 20,
    height: 20,
    borderRadius: 10,
    borderWidth: 2,
    alignItems: 'center',
    justifyContent: 'center',
    marginTop: 2,
  },
  radioDot: { width: 10, height: 10, borderRadius: 5 },
  methodIcon: {
    width: 40,
    height: 40,
    borderRadius: 12,
    alignItems: 'center',
    justifyContent: 'center',
  },
  methodBody: { flex: 1, minWidth: 0, gap: 2 },
  methodTitleRow: {
    flexDirection: 'row',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: 8,
  },
  methodTitle: {
    fontSize: 16,
    lineHeight: 24,
    fontWeight: '500',
    letterSpacing: 0,
  },
  methodSummary: { fontSize: 14, lineHeight: 20, letterSpacing: 0 },
  tag: { borderRadius: 8, paddingHorizontal: 8, paddingVertical: 2 },
  tagChip: { paddingHorizontal: 10, paddingVertical: 4 },
  tagText: {
    fontSize: 12,
    lineHeight: 16,
    fontWeight: '500',
    letterSpacing: 0,
  },
  chipRow: { flexDirection: 'row', flexWrap: 'wrap', gap: 8, marginTop: 4 },
  detailList: {
    gap: 10,
    marginTop: 8,
    paddingTop: 12,
    borderTopWidth: StyleSheet.hairlineWidth,
  },
  detailLabel: {
    fontSize: 12,
    lineHeight: 16,
    fontWeight: '500',
    letterSpacing: 0,
  },
  detailValue: { fontSize: 14, lineHeight: 20, letterSpacing: 0 },
  footer: {
    borderTopWidth: StyleSheet.hairlineWidth,
    paddingHorizontal: 24,
    paddingTop: 16,
    paddingBottom: 8,
    gap: 8,
  },
  footerHint: {
    fontSize: 12,
    lineHeight: 16,
    textAlign: 'center',
    letterSpacing: 0,
  },
  primaryButton: {
    height: 56,
    borderRadius: 28,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
    paddingHorizontal: 16,
  },
  primaryButtonText: {
    fontSize: 16,
    lineHeight: 24,
    fontWeight: '500',
    letterSpacing: 0,
  },
  textButton: {
    height: 48,
    borderRadius: 24,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 8,
  },
  textButtonText: {
    fontSize: 15,
    lineHeight: 20,
    fontWeight: '500',
    letterSpacing: 0,
  },
  banner: {
    flexDirection: 'row',
    alignItems: 'flex-start',
    gap: 12,
    marginHorizontal: 24,
    marginBottom: 16,
    paddingVertical: 12,
    paddingHorizontal: 16,
    borderRadius: 16,
  },
  bannerIcon: { marginTop: 1 },
  bannerText: {
    flex: 1,
    fontSize: 14,
    lineHeight: 20,
    fontWeight: '500',
    letterSpacing: 0,
  },
  stepsContent: { paddingHorizontal: 24, paddingTop: 8, paddingBottom: 16 },
  step: { flexDirection: 'row', gap: 16 },
  stepRail: { alignItems: 'center' },
  stepDot: {
    width: 28,
    height: 28,
    borderRadius: 14,
    alignItems: 'center',
    justifyContent: 'center',
  },
  stepDotText: {
    fontSize: 14,
    lineHeight: 20,
    fontWeight: '500',
    letterSpacing: 0,
  },
  stepLine: { flexGrow: 1, width: 2, marginTop: 4, borderRadius: 1 },
  stepBody: { flex: 1, minWidth: 0 },
  stepBodySpaced: { paddingBottom: 24 },
  stepTitle: {
    fontSize: 16,
    lineHeight: 28,
    fontWeight: '500',
    letterSpacing: 0,
  },
  stepText: { fontSize: 14, lineHeight: 20, letterSpacing: 0 },
  commandBlock: {
    borderRadius: 16,
    paddingTop: 4,
    paddingLeft: 16,
    paddingRight: 4,
    paddingBottom: 16,
    marginTop: 12,
  },
  commandToolbar: {
    minHeight: 48,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  commandLabel: {
    fontSize: 12,
    lineHeight: 16,
    fontWeight: '500',
    letterSpacing: 0.4,
    opacity: 0.8,
  },
  copyButton: {
    height: 48,
    borderRadius: 24,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingLeft: 12,
    paddingRight: 16,
  },
  copyButtonText: {
    fontSize: 14,
    lineHeight: 20,
    fontWeight: '500',
    letterSpacing: 0,
  },
  commandLine: { flexDirection: 'row', gap: 8, paddingRight: 12 },
  commandPrompt: { fontSize: 13, lineHeight: 20, fontFamily: 'monospace' },
  commandText: {
    flex: 1,
    fontSize: 13,
    lineHeight: 20,
    fontFamily: 'monospace',
    letterSpacing: 0,
  },
  restrictionText: {
    paddingHorizontal: 24,
    paddingBottom: 24,
    fontSize: 14,
    lineHeight: 20,
  },
});
