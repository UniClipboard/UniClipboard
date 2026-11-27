import { useEffect, useRef } from 'react';
import {
  ActivityIndicator,
  Animated,
  BackHandler,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';
import PagerView from 'react-native-pager-view';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useTranslation } from 'react-i18next';
import {
  BatteryCharging,
  Bell,
  Check,
  ChevronRight,
  ClipboardPaste,
  Copy,
  Hash,
  Keyboard,
  Laptop,
  Lock,
  QrCode,
  Share,
  ShieldCheck,
  Smartphone,
} from 'lucide-react-native';
import { AppButton, AppHost } from '@/components/ui';
import { useTheme } from '@/hooks/useTheme';
import type { OnboardingScreenProps } from './OnboardingScreen.types';
import { OnboardingBackButton } from './onboarding/OnboardingBackButton';
import { OnboardingHeroArt } from './onboarding/OnboardingHeroArt';
import { ONBOARDING_PLATFORM_CONFIG } from './onboarding/onboardingPlatformCopy';
import {
  deviceDisplayName,
  formatFingerprintLines,
  formatPairingCodeDisplay,
  useOnboardingFlow,
} from './onboarding/useOnboardingFlow';
import { useOnboardingScanner } from './onboarding/useOnboardingScanner';
import {
  usePlatformCapabilities,
  type CapabilityIconKind,
} from './onboarding/usePlatformCapabilities';

const SLIDES = ['welcome', 'sync', 'system'] as const;

function CapabilityIcon({
  kind,
  size = 18,
  color,
}: {
  kind: CapabilityIconKind;
  size?: number;
  color: string;
}) {
  switch (kind) {
    case 'paste':
      return <ClipboardPaste size={size} color={color} />;
    case 'keyboard':
      return <Keyboard size={size} color={color} />;
    case 'share':
      return <Share size={size} color={color} />;
    case 'bell':
      return <Bell size={size} color={color} />;
    case 'battery':
      return <BatteryCharging size={size} color={color} />;
  }
}

export function OnboardingScreen({ onComplete }: OnboardingScreenProps) {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const c = theme.colors;
  const pagerRef = useRef<PagerView>(null);
  const digitRefs = useRef<Array<TextInput | null>>([]);
  const segmented = ONBOARDING_PLATFORM_CONFIG.segmentedCards;

  const flow = useOnboardingFlow(onComplete);
  const scrollProgress = useRef(new Animated.Value(flow.slide)).current;
  const latestSlideRef = useRef(flow.slide);
  latestSlideRef.current = flow.slide;

  useEffect(() => {
    if (flow.step === 'intro') {
      scrollProgress.setValue(latestSlideRef.current);
    }
  }, [flow.step, scrollProgress]);
  const caps = usePlatformCapabilities(
    flow.step === 'capabilities' || flow.step === 'ready',
  );
  const scanner = useOnboardingScanner({
    isLive: flow.mode === 'live',
    onRead: (value) => flow.submitPairingInput(value),
    onFallbackToCode: () => {
      flow.setError(null);
      flow.setStep('pair-code');
    },
  });

  // Handle hardware back button cleanly (Android; safe no-op on iOS).
  useEffect(() => {
    const sub = BackHandler.addEventListener('hardwareBackPress', () => {
      if (flow.step === 'intro') {
        if (flow.slide > 0) {
          const prev = flow.slide - 1;
          flow.setSlide(prev);
          pagerRef.current?.setPage(prev);
          return true;
        }
        return false;
      }
      flow.goBack();
      return true;
    });
    return () => sub.remove();
  }, [flow]);

  const pageBg =
    flow.step === 'intro' ? c.background : segmented ? c.background : c.surface;
  const cardBg = segmented ? c.surfaceMid : c.surfaceLowest;
  const iconBg = (iosColor: string) => (segmented ? c.accentContainer : iosColor);
  const iconFg = () => (segmented ? (c.onAccentContainer as string) : '#FFFFFF');

  const rowContainerStyle = (index: number, total: number) => {
    if (!segmented) {
      return [
        s.iosRow,
        index > 0 && { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: c.separator },
      ];
    }
    const isFirst = index === 0;
    const isLast = index === total - 1;
    return [
      s.m3Row,
      {
        backgroundColor: c.surfaceMid,
        borderTopLeftRadius: isFirst ? 20 : 4,
        borderTopRightRadius: isFirst ? 20 : 4,
        borderBottomLeftRadius: isLast ? 20 : 4,
        borderBottomRightRadius: isLast ? 20 : 4,
      },
    ];
  };

  const renderTopBar = ({
    showBack = false,
    stepBadge,
    rightLabel,
    onRightPress,
    rightTestID,
    rightOpacity,
    rightHidden = false,
  }: {
    showBack?: boolean;
    stepBadge?: string;
    rightLabel?: string;
    onRightPress?: () => void;
    rightTestID?: string;
    rightOpacity?: Animated.AnimatedInterpolation<number>;
    rightHidden?: boolean;
  }) => (
    <View style={s.topBar}>
      <View style={s.topSide}>
        {showBack ? (
          <OnboardingBackButton
            onPress={flow.goBack}
            disabled={flow.busy || flow.finishing}
            label={t('common:action.back')}
          />
        ) : null}
      </View>

      {stepBadge ? (
        <View
          style={[
            s.stepPill,
            { backgroundColor: segmented ? c.surfaceHigh : c.fillQuaternary },
          ]}
        >
          <View style={[s.stepDot, { backgroundColor: c.accent }]} />
          <Text style={[s.stepPillText, { color: c.textSecondary }]}>{stepBadge}</Text>
        </View>
      ) : (
        <View />
      )}

      <View style={[s.topSide, s.topSideRight]}>
        {rightLabel && onRightPress ? (
          <Animated.View
            style={rightOpacity ? { opacity: rightOpacity } : undefined}
            pointerEvents={rightHidden ? 'none' : 'auto'}
            accessibilityElementsHidden={rightHidden}
            importantForAccessibility={rightHidden ? 'no-hide-descendants' : 'yes'}
          >
            <Pressable
              testID={rightTestID}
              onPress={onRightPress}
              disabled={rightHidden || flow.busy || flow.finishing}
              hitSlop={10}
              style={({ pressed }) => [s.skipBtn, pressed && { opacity: 0.6 }]}
            >
              <Text
                style={[
                  s.skipText,
                  { color: segmented ? c.accent : c.textSecondary },
                ]}
              >
                {rightLabel}
              </Text>
            </Pressable>
          </Animated.View>
        ) : null}
      </View>
    </View>
  );

  // =========================================================================
  // STEP 1: INTRO CAROUSEL (3 SLIDES)
  // =========================================================================
  const renderIntro = () => {
    const isLast = flow.slide === SLIDES.length - 1;
    const skipOpacity = scrollProgress.interpolate({
      inputRange: [1, 1.85],
      outputRange: [1, 0],
      extrapolate: 'clamp',
    });
    const secondaryOpacity = scrollProgress.interpolate({
      inputRange: [1.15, 2],
      outputRange: [0, 1],
      extrapolate: 'clamp',
    });
    const secondaryTranslateY = scrollProgress.interpolate({
      inputRange: [1.15, 2],
      outputRange: [8, 0],
      extrapolate: 'clamp',
    });

    return (
      <>
        {renderTopBar({
          rightLabel: t('onboarding:guide.skip'),
          onRightPress: () => void flow.finishOnboarding(),
          rightTestID: 'setup-start-clean',
          rightOpacity: skipOpacity,
          rightHidden: isLast,
        })}

        <PagerView
          ref={pagerRef}
          style={s.pager}
          initialPage={flow.slide}
          onPageScroll={({ nativeEvent }) => {
            scrollProgress.setValue(nativeEvent.position + nativeEvent.offset);
          }}
          onPageSelected={({ nativeEvent }) => {
            flow.setSlide(nativeEvent.position);
            scrollProgress.setValue(nativeEvent.position);
          }}
          scrollEnabled={!flow.busy && !flow.finishing}
        >
          {/* Slide 1: Welcome */}
          <View key="welcome" collapsable={false} style={s.slidePage}>
            <ScrollView
              contentContainerStyle={s.slideContent}
              showsVerticalScrollIndicator={false}
            >
              <OnboardingHeroArt kind="welcome" />
              <View style={s.slideCopy}>
                <Text accessibilityRole="header" style={[s.heroTitle, { color: c.textPrimary }]}>
                  {t('onboarding:guide.slides.welcomeTitle')}
                </Text>
                <Text style={[s.heroSlogan, { color: c.accent }]}>
                  {t('onboarding:guide.slides.welcomeSlogan')}
                </Text>
                <View style={s.trustRow}>
                  <View style={s.trustItem}>
                    <ShieldCheck size={14} color={c.success as string} />
                    <Text style={[s.trustText, { color: c.textSecondary }]}>
                      {t('onboarding:guide.slides.badgeNoAccount')}
                    </Text>
                  </View>
                  <View style={s.trustItem}>
                    <Lock size={14} color={c.success as string} />
                    <Text style={[s.trustText, { color: c.textSecondary }]}>
                      {t('onboarding:guide.slides.badgeE2ee')}
                    </Text>
                  </View>
                </View>
              </View>
            </ScrollView>
          </View>

          {/* Slide 2: Cross-Device Sync */}
          <View key="sync" collapsable={false} style={s.slidePage}>
            <ScrollView
              contentContainerStyle={s.slideContent}
              showsVerticalScrollIndicator={false}
            >
              <OnboardingHeroArt kind="sync" />
              <View style={s.slideCopy}>
                <Text accessibilityRole="header" style={[s.heroTitle, { color: c.textPrimary }]}>
                  {t('onboarding:guide.slides.syncTitle')}
                </Text>
                <Text style={[s.heroBody, { color: c.textSecondary }]}>
                  {t('onboarding:guide.slides.syncBody')}
                </Text>
              </View>
            </ScrollView>
          </View>

          {/* Slide 3: Platform Integration */}
          <View key="system" collapsable={false} style={s.slidePage}>
            <ScrollView
              contentContainerStyle={s.slideContent}
              showsVerticalScrollIndicator={false}
            >
              <OnboardingHeroArt kind="system" />
              <View style={s.slideCopy}>
                <Text accessibilityRole="header" style={[s.heroTitle, { color: c.textPrimary }]}>
                  {t(`onboarding:${ONBOARDING_PLATFORM_CONFIG.slide3TitleKey}`)}
                </Text>
                <Text style={[s.heroBody, { color: c.textSecondary }]}>
                  {t(`onboarding:${ONBOARDING_PLATFORM_CONFIG.slide3BodyKey}`)}
                </Text>
              </View>
            </ScrollView>
          </View>
        </PagerView>

        <View style={s.footer}>
          <View
            style={s.dots}
            accessible
            accessibilityRole="progressbar"
            accessibilityLabel={t('onboarding:intro.progress', {
              current: flow.slide + 1,
              total: SLIDES.length,
            })}
          >
            {SLIDES.map((kind, idx) => {
              const dotWidth = scrollProgress.interpolate({
                inputRange: [idx - 1, idx, idx + 1],
                outputRange: [7, 22, 7],
                extrapolate: 'clamp',
              });
              const dotOpacity = scrollProgress.interpolate({
                inputRange: [idx - 1, idx, idx + 1],
                outputRange: [0.24, 1, 0.24],
                extrapolate: 'clamp',
              });
              return (
                <Pressable
                  key={kind}
                  hitSlop={8}
                  onPress={() => {
                    flow.setSlide(idx);
                    pagerRef.current?.setPage(idx);
                  }}
                >
                  <Animated.View
                    style={[
                      s.dot,
                      {
                        width: dotWidth,
                        opacity: dotOpacity,
                        backgroundColor: c.accent,
                      },
                    ]}
                  />
                </Pressable>
              );
            })}
          </View>

          {flow.error ? (
            <Text accessibilityRole="alert" style={[s.errorText, { color: c.error }]}>
              {flow.error}
            </Text>
          ) : null}

          <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
            <AppButton
              testID={isLast ? 'onboarding-start-config' : 'onboarding-next'}
              title={t(isLast ? 'onboarding:guide.startConfig' : 'onboarding:guide.next')}
              variant="filled"
              fullWidth
              size="large"
              colors={{ containerColor: c.accent, contentColor: c.onAccent }}
              disabled={flow.busy || flow.finishing}
              onPress={
                isLast
                  ? () => void flow.beginSetup()
                  : () => {
                      const next = flow.slide + 1;
                      flow.setSlide(next);
                      pagerRef.current?.setPage(next);
                    }
              }
            />
          </AppHost>

          <Animated.View
            style={[
              s.secondarySlot,
              {
                opacity: secondaryOpacity,
                transform: [{ translateY: secondaryTranslateY }],
              },
            ]}
            pointerEvents={isLast ? 'auto' : 'none'}
            accessibilityElementsHidden={!isLast}
            importantForAccessibility={isLast ? 'yes' : 'no-hide-descendants'}
          >
            <AppHost style={s.secondaryHost} matchContents={{ vertical: true }}>
              <AppButton
                testID="onboarding-skip"
                title={t('onboarding:guide.enterHistoryFirst')}
                variant="text"
                fullWidth
                colors={{ contentColor: segmented ? c.accent : c.textSecondary }}
                disabled={!isLast || flow.busy || flow.finishing}
                onPress={() => void flow.finishOnboarding()}
              />
            </AppHost>
          </Animated.View>
        </View>
      </>
    );
  };

  // =========================================================================
  // STEP 2: DEVICE PAIRING (ENTRY)
  // =========================================================================
  const renderPairEntry = () => (
    <>
      {renderTopBar({
        showBack: true,
        stepBadge: t('onboarding:guide.stepBadge', {
          current: 1,
          total: 2,
          label: t('onboarding:guide.pair.stepLabel'),
        }),
        rightLabel: t('onboarding:guide.skip'),
        onRightPress: () => flow.setStep('capabilities'),
        rightTestID: 'onboarding-pair-skip-top',
      })}

      <ScrollView contentContainerStyle={s.scrollContent} showsVerticalScrollIndicator={false}>
        <View style={s.pageHead}>
          <Text accessibilityRole="header" style={[s.pageTitle, { color: c.textPrimary }]}>
            {t('onboarding:guide.pair.title')}
          </Text>
          <Text style={[s.pageSubtitle, { color: c.textSecondary }]}>
            {t('onboarding:guide.pair.subtitle')}
          </Text>
        </View>

        <View
          style={[
            s.pairHeroCard,
            {
              backgroundColor: cardBg,
              borderColor: segmented ? 'transparent' : c.separator,
            },
          ]}
        >
          <View style={s.pairHeroTop}>
            <View style={[s.qrBadge, { backgroundColor: c.accent }]}>
              <QrCode size={25} color={c.onAccent as string} />
            </View>
            <View style={s.flex1}>
              <Text style={[s.cardTitle, { color: c.textPrimary }]}>
                {t('onboarding:guide.pair.scanCardTitle')}
              </Text>
              <Text style={[s.cardSub, { color: c.textSecondary }]}>
                {t('onboarding:guide.pair.scanCardSub')}
              </Text>
            </View>
          </View>
          <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
            <AppButton
              testID="onboarding-pair-scan"
              title={t('onboarding:guide.pair.scanAction')}
              variant="filled"
              fullWidth
              size="large"
              colors={{ containerColor: c.accent, contentColor: c.onAccent }}
              disabled={flow.busy}
              onPress={scanner.openScanner}
            />
          </AppHost>
        </View>

        <View
          style={[
            s.groupWrap,
            !segmented && { backgroundColor: cardBg },
            segmented && { gap: 2 },
          ]}
        >
          <Pressable
            testID="onboarding-pair-code"
            onPress={() => {
              flow.setError(null);
              flow.setStep('pair-code');
            }}
            style={({ pressed }) => [
              rowContainerStyle(0, 2),
              pressed && { opacity: 0.72 },
            ]}
          >
            <View
              style={[
                segmented ? s.m3IconCircle : s.iosIconTile,
                { backgroundColor: iconBg('#007AFF') },
              ]}
            >
              <Hash size={18} color={iconFg()} />
            </View>
            <View style={s.flex1}>
              <Text style={[s.rowTitle, { color: c.textPrimary }]}>
                {t('onboarding:guide.pair.codeRowTitle')}
              </Text>
              <Text style={[s.rowSub, { color: c.textSecondary }]}>
                {t('onboarding:guide.pair.codeRowSub')}
              </Text>
            </View>
            <ChevronRight size={17} color={c.textTertiary as string} />
          </Pressable>

          <Pressable
            testID="onboarding-pair-my-code"
            onPress={() => void flow.toggleMyCode()}
            disabled={flow.busy}
            style={({ pressed }) => [
              rowContainerStyle(1, 2),
              pressed && { opacity: 0.72 },
            ]}
          >
            <View
              style={[
                segmented ? s.m3IconCircle : s.iosIconTile,
                { backgroundColor: iconBg('#34C759') },
              ]}
            >
              <Smartphone size={18} color={iconFg()} />
            </View>
            <View style={s.flex1}>
              <Text style={[s.rowTitle, { color: c.textPrimary }]}>
                {t('onboarding:guide.pair.myCodeRowTitle')}
              </Text>
              <Text style={[s.rowSub, { color: c.textSecondary }]}>
                {flow.showMyCode
                  ? t('onboarding:guide.pair.myCodeHide')
                  : t('onboarding:guide.pair.myCodeRowSub')}
              </Text>
            </View>
            <ChevronRight size={17} color={c.textTertiary as string} />
          </Pressable>
        </View>

        {flow.showMyCode && flow.pairingCode ? (
          <View
            style={[
              s.myCodeCard,
              { backgroundColor: segmented ? c.surfaceLow : c.fillQuaternary },
            ]}
          >
            <Text selectable style={[s.myCodeDigits, { color: c.textPrimary }]}>
              {formatPairingCodeDisplay(flow.pairingCode.code)}
            </Text>
            <View style={s.myCodeMeta}>
              {flow.myCodeCountdown ? (
                <Text style={[s.myCodeMetaText, { color: c.textSecondary }]}>
                  {flow.myCodeCountdown}
                </Text>
              ) : null}
              <Pressable
                onPress={() => void flow.copyMyCode()}
                style={({ pressed }) => [s.inlineLink, pressed && { opacity: 0.6 }]}
              >
                <Copy size={13} color={c.accent as string} />
                <Text style={[s.inlineLinkText, { color: c.accent }]}>
                  {flow.myCodeCopied
                    ? t('onboarding:guide.pair.myCodeCopied')
                    : t('onboarding:guide.pair.myCodeCopy')}
                </Text>
              </Pressable>
            </View>
          </View>
        ) : null}

        {flow.error ? (
          <Text accessibilityRole="alert" style={[s.errorBanner, { color: c.error }]}>
            {flow.error}
          </Text>
        ) : null}
      </ScrollView>

      <View style={s.footer}>
        <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
          <AppButton
            testID="onboarding-pair-later"
            title={t('onboarding:guide.pair.later')}
            variant="tonal"
            fullWidth
            size="large"
            colors={{ containerColor: c.accentContainer, contentColor: c.onAccentContainer }}
            onPress={() => flow.setStep('capabilities')}
          />
        </AppHost>
      </View>
    </>
  );

  // =========================================================================
  // STEP 2b: 6-DIGIT PAIRING CODE / TICKET INPUT
  // =========================================================================
  const handleDigitChange = (index: number, value: string) => {
    const trimmed = value.trim();
    if (trimmed.startsWith('unic-pair:')) {
      flow.setTicketMode(true);
      flow.setTicketInput(trimmed);
      flow.setError(null);
      return;
    }
    const clean = value.replace(/\D/g, '');
    if (clean.length > 1) {
      const next = [...flow.codeDigits];
      clean
        .slice(0, 6)
        .split('')
        .forEach((digit, offset) => {
          next[offset] = digit;
        });
      flow.setCodeDigits(next);
      flow.setError(null);
      const focusIdx = Math.min(5, clean.length);
      digitRefs.current[focusIdx]?.focus();
      return;
    }
    const next = [...flow.codeDigits];
    next[index] = clean.slice(-1);
    flow.setCodeDigits(next);
    flow.setError(null);
    if (next[index] && index < 5) {
      digitRefs.current[index + 1]?.focus();
    }
  };

  const renderPairCode = () => (
    <>
      {renderTopBar({
        showBack: true,
        rightLabel: t('onboarding:guide.skip'),
        onRightPress: () => flow.setStep('capabilities'),
      })}

      <ScrollView
        contentContainerStyle={s.scrollContent}
        keyboardShouldPersistTaps="handled"
        showsVerticalScrollIndicator={false}
      >
        <View style={s.pageHead}>
          <Text accessibilityRole="header" style={[s.pageTitle, { color: c.textPrimary }]}>
            {t('onboarding:guide.pair.codeTitle')}
          </Text>
          <Text style={[s.pageSubtitle, { color: c.textSecondary }]}>
            {t('onboarding:guide.pair.codeSubtitle')}
          </Text>
        </View>

        <View style={[s.codePanel, { backgroundColor: cardBg }]}>
          {!flow.ticketMode ? (
            <View style={s.otpGrid}>
              {flow.codeDigits.map((digit, idx) => (
                <TextInput
                  key={idx}
                  ref={(el) => {
                    digitRefs.current[idx] = el;
                  }}
                  testID={`onboarding-otp-${idx}`}
                  value={digit}
                  keyboardType="number-pad"
                  maxLength={6}
                  textAlign="center"
                  selectTextOnFocus
                  onChangeText={(val) => handleDigitChange(idx, val)}
                  onKeyPress={({ nativeEvent }) => {
                    if (nativeEvent.key === 'Backspace' && !digit && idx > 0) {
                      const next = [...flow.codeDigits];
                      next[idx - 1] = '';
                      flow.setCodeDigits(next);
                      digitRefs.current[idx - 1]?.focus();
                    }
                  }}
                  style={[
                    s.otpBox,
                    {
                      backgroundColor: segmented ? c.surfaceHighest : c.background,
                      borderColor: digit ? c.accent : c.separator,
                      color: c.textPrimary,
                    },
                  ]}
                />
              ))}
            </View>
          ) : (
            <TextInput
              testID="onboarding-ticket-input"
              value={flow.ticketInput}
              onChangeText={(val) => {
                flow.setTicketInput(val);
                flow.setError(null);
              }}
              placeholder={t('onboarding:guide.pair.ticketPlaceholder')}
              placeholderTextColor={c.textTertiary as string}
              autoCapitalize="none"
              autoCorrect={false}
              multiline
              style={[
                s.ticketInput,
                {
                  backgroundColor: segmented ? c.surfaceHighest : c.background,
                  borderColor: c.separator,
                  color: c.textPrimary,
                },
              ]}
            />
          )}

          <View style={s.codeHelperRow}>
            <Text style={[s.codeHelperText, { color: c.textSecondary }]}>
              {t('onboarding:guide.pair.codeHint')}
            </Text>
            <Pressable
              onPress={() => {
                flow.setTicketMode(!flow.ticketMode);
                flow.setError(null);
              }}
              hitSlop={8}
            >
              <Text style={[s.inlineLinkText, { color: c.accent }]}>
                {flow.ticketMode
                  ? t('onboarding:guide.pair.switchToCode')
                  : t('onboarding:guide.pair.pasteTicket')}
              </Text>
            </Pressable>
          </View>
        </View>

        {flow.error ? (
          <View style={[s.errBox, { backgroundColor: c.errorContainer }]}>
            <Text accessibilityRole="alert" style={[s.errBoxText, { color: c.onErrorContainer }]}>
              {flow.error}
            </Text>
          </View>
        ) : null}
      </ScrollView>

      <View style={s.footer}>
        <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
          <AppButton
            testID="onboarding-code-connect"
            title={t('onboarding:guide.pair.connect')}
            variant="filled"
            fullWidth
            size="large"
            colors={{ containerColor: c.accent, contentColor: c.onAccent }}
            disabled={!flow.isCodeReady || flow.busy}
            onPress={flow.submitCodeForm}
          />
        </AppHost>
        <AppHost style={s.secondaryHost} matchContents={{ vertical: true }}>
          <AppButton
            title={t('onboarding:guide.pair.switchToScan')}
            variant="text"
            fullWidth
            colors={{ contentColor: segmented ? c.accent : c.textSecondary }}
            onPress={scanner.openScanner}
          />
        </AppHost>
      </View>
    </>
  );

  // =========================================================================
  // STEP 2c: HANDSHAKE
  // =========================================================================
  const renderPairHandshake = () => {
    const phases = [
      t('onboarding:guide.pair.phaseResolving'),
      t('onboarding:guide.pair.phaseConnecting'),
      t('onboarding:guide.pair.phaseHandshaking'),
    ];
    return (
      <>
        {renderTopBar({
          showBack: true,
          stepBadge: t('onboarding:guide.stepBadge', {
            current: 1,
            total: 2,
            label: t('onboarding:guide.pair.connectingLabel'),
          }),
        })}

        <View style={s.centerStage}>
          <View style={[s.hsCard, { backgroundColor: cardBg }]}>
            <ActivityIndicator size="large" color={c.accent} />
            <Text style={[s.hsTitle, { color: c.textPrimary }]}>
              {t('onboarding:guide.pair.connecting')}
            </Text>
            <View style={s.hsList}>
              {phases.map((label, idx) => {
                const done = idx < flow.handshakeStageIndex;
                const active = idx === flow.handshakeStageIndex;
                return (
                  <View key={label} style={s.hsRow}>
                    <View style={s.hsIconSlot}>
                      {done ? (
                        <Check size={16} color={c.success as string} />
                      ) : active ? (
                        <ActivityIndicator size="small" color={c.accent} />
                      ) : (
                        <View style={[s.hsBullet, { backgroundColor: c.separator }]} />
                      )}
                    </View>
                    <Text
                      style={[
                        s.hsText,
                        {
                          color: done ? c.success : active ? c.textPrimary : c.textSecondary,
                          fontWeight: active ? '600' : '400',
                        },
                      ]}
                    >
                      {label}
                    </Text>
                  </View>
                );
              })}
            </View>
          </View>
        </View>

        <View style={s.footer}>
          <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
            <AppButton
              title={t('onboarding:guide.cancel')}
              variant="tonal"
              fullWidth
              size="large"
              colors={{ containerColor: c.accentContainer, contentColor: c.onAccentContainer }}
              onPress={flow.cancelHandshake}
            />
          </AppHost>
        </View>
      </>
    );
  };

  // =========================================================================
  // STEP 2d: MANDATORY FINGERPRINT VERIFICATION
  // =========================================================================
  const renderPairApprove = () => {
    const device = flow.pendingDevice;
    return (
      <>
        {renderTopBar({
          showBack: true,
          stepBadge: t('onboarding:guide.stepBadge', {
            current: 1,
            total: 2,
            label: t('onboarding:guide.pair.approveLabel'),
          }),
        })}

        <ScrollView contentContainerStyle={s.scrollContent} showsVerticalScrollIndicator={false}>
          <View style={s.pageHead}>
            <Text accessibilityRole="header" style={[s.pageTitle, { color: c.textPrimary }]}>
              {t('onboarding:guide.pair.approveTitle')}
            </Text>
            <Text style={[s.pageSubtitle, { color: c.textSecondary }]}>
              {t('onboarding:guide.pair.approveSubtitle')}
            </Text>
          </View>

          <View style={[s.fpCard, { backgroundColor: cardBg }]}>
            <View style={[s.fpPeer, { borderBottomColor: c.separator }]}>
              <View style={[s.fpPeerIcon, { backgroundColor: c.warningContainer }]}>
                <Laptop size={22} color={c.warning as string} />
              </View>
              <View style={s.flex1}>
                <Text style={[s.fpPeerName, { color: c.textPrimary }]} numberOfLines={1}>
                  {deviceDisplayName(device)}
                </Text>
                <Text style={[s.fpPeerId, { color: c.textSecondary }]} numberOfLines={1}>
                  {device?.id ?? ''}
                </Text>
              </View>
            </View>

            <Text style={[s.fpLabel, { color: c.textSecondary }]}>
              {t('onboarding:guide.pair.fingerprintLabel')}
            </Text>
            <View
              style={[
                s.fpBox,
                { backgroundColor: segmented ? c.surfaceLow : c.fillQuaternary },
              ]}
            >
              <Text
                testID="onboarding-approve-fingerprint"
                selectable
                style={[s.fpValue, { color: c.textPrimary }]}
              >
                {formatFingerprintLines(device?.fingerprint ?? '')}
              </Text>
            </View>
            <Text style={[s.fpHint, { color: c.textSecondary }]}>
              {t('onboarding:guide.pair.approveHint')}
            </Text>
          </View>

          {flow.error ? (
            <View style={[s.errBox, { backgroundColor: c.errorContainer }]}>
              <Text accessibilityRole="alert" style={[s.errBoxText, { color: c.onErrorContainer }]}>
                {flow.error}
              </Text>
            </View>
          ) : null}
        </ScrollView>

        <View style={s.footer}>
          <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
            <AppButton
              testID="onboarding-approve"
              title={t('onboarding:guide.pair.approve')}
              variant="filled"
              fullWidth
              size="large"
              colors={{ containerColor: c.accent, contentColor: c.onAccent }}
              disabled={flow.busy}
              onPress={() => void flow.approvePendingDevice()}
            />
          </AppHost>
          <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
            <AppButton
              testID="onboarding-reject"
              title={t('onboarding:guide.pair.reject')}
              variant="tonal"
              fullWidth
              size="large"
              colors={{ containerColor: c.errorContainer, contentColor: c.error }}
              disabled={flow.busy}
              onPress={() => void flow.rejectPendingDevice()}
            />
          </AppHost>
        </View>
      </>
    );
  };

  // =========================================================================
  // STEP 2e: PAIRED SUCCESS
  // =========================================================================
  const renderPairSuccess = () => (
    <>
      {renderTopBar({
        stepBadge: t('onboarding:guide.stepBadge', {
          current: 1,
          total: 2,
          label: t('onboarding:guide.pair.connectedLabel'),
        }),
      })}

      <View style={s.centerStage}>
        <View style={[s.checkCircle, { backgroundColor: c.successContainer }]}>
          <Check size={34} color={c.success as string} />
        </View>
        <Text accessibilityRole="header" style={[s.readyTitle, { color: c.textPrimary }]}>
          {t('onboarding:guide.pair.connectedTitle', {
            name: deviceDisplayName(flow.pairedDevice),
          })}
        </Text>
        <Text style={[s.readySub, { color: c.textSecondary }]}>
          {t('onboarding:guide.pair.connectedBody')}
        </Text>
      </View>

      <View style={s.footer}>
        <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
          <AppButton
            testID="onboarding-pair-next"
            title={t('onboarding:guide.next')}
            variant="filled"
            fullWidth
            size="large"
            colors={{ containerColor: c.accent, contentColor: c.onAccent }}
            onPress={() => flow.setStep('capabilities')}
          />
        </AppHost>
        <AppHost style={s.secondaryHost} matchContents={{ vertical: true }}>
          <AppButton
            title={t('onboarding:guide.pair.finishDirect')}
            variant="text"
            fullWidth
            colors={{ contentColor: segmented ? c.accent : c.textSecondary }}
            onPress={() => flow.setStep('ready')}
          />
        </AppHost>
      </View>
    </>
  );

  // =========================================================================
  // STEP 3: PLATFORM CAPABILITIES (SINGLE-GLANCE ROWS)
  // =========================================================================
  const pillColors = (tone: 'primary' | 'soft' | 'ok' | 'warn') => {
    switch (tone) {
      case 'primary':
        return { bg: c.accent, fg: c.onAccent };
      case 'ok':
        return { bg: c.successContainer, fg: c.onSuccessContainer };
      case 'warn':
        return { bg: c.warningContainer, fg: c.onWarningContainer };
      case 'soft':
      default:
        return { bg: c.accentContainer, fg: c.onAccentContainer };
    }
  };

  const renderCapabilities = () => (
    <>
      {renderTopBar({
        showBack: true,
        stepBadge: t('onboarding:guide.stepBadge', {
          current: 2,
          total: 2,
          label: caps.stepLabel,
        }),
        rightLabel: t('onboarding:guide.skip'),
        onRightPress: () => flow.setStep('ready'),
        rightTestID: 'onboarding-caps-skip',
      })}

      <ScrollView contentContainerStyle={s.scrollContent} showsVerticalScrollIndicator={false}>
        <View style={s.pageHead}>
          <Text accessibilityRole="header" style={[s.pageTitle, { color: c.textPrimary }]}>
            {caps.title}
          </Text>
          <Text style={[s.pageSubtitle, { color: c.textSecondary }]}>{caps.subtitle}</Text>
        </View>

        <View
          style={[
            s.groupWrap,
            !segmented && { backgroundColor: cardBg },
            segmented && { gap: 2 },
          ]}
        >
          {caps.rows.map((row, idx) => {
            const pc = pillColors(row.actionTone);
            return (
              <View key={row.id} style={rowContainerStyle(idx, caps.rows.length)}>
                <View
                  style={[
                    segmented ? s.m3IconCircle : s.iosIconTile,
                    { backgroundColor: iconBg(row.iosTileColor) },
                  ]}
                >
                  <CapabilityIcon kind={row.icon} color={iconFg()} />
                </View>
                <View style={s.flex1}>
                  <Text style={[s.rowTitle, { color: c.textPrimary }]} numberOfLines={1}>
                    {row.title}
                  </Text>
                  <Text style={[s.rowSub, { color: c.textSecondary }]} numberOfLines={2}>
                    {row.subtitle}
                  </Text>
                </View>
                <Pressable
                  testID={row.testID}
                  onPress={row.onPress}
                  style={({ pressed }) => [
                    s.capPillBtn,
                    { backgroundColor: pc.bg },
                    pressed && { opacity: 0.78 },
                  ]}
                >
                  <Text style={[s.capPillText, { color: pc.fg }]}>{row.actionLabel}</Text>
                </Pressable>
              </View>
            );
          })}
        </View>
      </ScrollView>

      <View style={s.footer}>
        <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
          <AppButton
            testID="onboarding-caps-done"
            title={t('onboarding:guide.done')}
            variant="filled"
            fullWidth
            size="large"
            colors={{ containerColor: c.accent, contentColor: c.onAccent }}
            onPress={() => flow.setStep('ready')}
          />
        </AppHost>
      </View>
    </>
  );

  // =========================================================================
  // STEP 4: READY SUMMARY
  // =========================================================================
  const renderReady = () => {
    const summaryRows = [
      {
        id: 'device',
        icon: flow.pairedDevice ? ('laptop' as const) : ('phone' as const),
        iosTileColor: flow.pairedDevice ? '#34C759' : '#8E8E93',
        title: flow.pairedDevice
          ? deviceDisplayName(flow.pairedDevice)
          : t('onboarding:guide.ready.deviceSync'),
        statusLabel: flow.pairedDevice
          ? t('onboarding:guide.ready.connected')
          : t('onboarding:guide.ready.pairLater'),
        isPositive: Boolean(flow.pairedDevice),
      },
      ...caps.readyItems,
    ];

    return (
      <>
        {renderTopBar({ showBack: true })}

        <ScrollView contentContainerStyle={s.scrollContent} showsVerticalScrollIndicator={false}>
          <View style={s.readyHero}>
            <View style={[s.checkCircle, { backgroundColor: c.successContainer }]}>
              <Check size={34} color={c.success as string} />
            </View>
            <Text accessibilityRole="header" style={[s.readyTitle, { color: c.textPrimary }]}>
              {t('onboarding:guide.ready.title')}
            </Text>
            <Text style={[s.readySub, { color: c.textSecondary }]}>
              {t('onboarding:guide.ready.subtitle')}
            </Text>
          </View>

          <View
            style={[
              s.groupWrap,
              !segmented && { backgroundColor: cardBg },
              segmented && { gap: 2 },
            ]}
          >
            {summaryRows.map((row, idx) => (
              <View key={row.id} style={rowContainerStyle(idx, summaryRows.length)}>
                <View
                  style={[
                    segmented ? s.m3IconCircle : s.iosIconTile,
                    { backgroundColor: iconBg(row.iosTileColor) },
                  ]}
                >
                  {row.icon === 'laptop' ? (
                    <Laptop size={18} color={iconFg()} />
                  ) : row.icon === 'phone' ? (
                    <Smartphone size={18} color={iconFg()} />
                  ) : (
                    <CapabilityIcon kind={row.icon} color={iconFg()} />
                  )}
                </View>
                <View style={s.flex1}>
                  <Text style={[s.rowTitle, { color: c.textPrimary }]} numberOfLines={1}>
                    {row.title}
                  </Text>
                </View>
                <View
                  style={[
                    s.statusTag,
                    {
                      backgroundColor: row.isPositive
                        ? c.successContainer
                        : segmented
                          ? c.surfaceHigh
                          : c.fillQuaternary,
                    },
                  ]}
                >
                  <Text
                    style={[
                      s.statusTagText,
                      { color: row.isPositive ? c.onSuccessContainer : c.textSecondary },
                    ]}
                  >
                    {row.statusLabel}
                  </Text>
                </View>
              </View>
            ))}
          </View>

          {flow.error ? (
            <Text accessibilityRole="alert" style={[s.errorBanner, { color: c.error }]}>
              {flow.error}
            </Text>
          ) : null}
        </ScrollView>

        <View style={s.footer}>
          <AppHost style={s.primaryHost} matchContents={{ vertical: true }}>
            <AppButton
              testID="onboarding-finish"
              title={t('onboarding:guide.startUsing')}
              variant="filled"
              fullWidth
              size="large"
              colors={{ containerColor: c.accent, contentColor: c.onAccent }}
              disabled={flow.finishing}
              onPress={() => void flow.finishOnboarding()}
            />
          </AppHost>
        </View>
      </>
    );
  };

  return (
    <SafeAreaView style={[s.root, { backgroundColor: pageBg }]} edges={['top', 'bottom']}>
      {flow.step === 'intro' && renderIntro()}
      {flow.step === 'pair' && renderPairEntry()}
      {flow.step === 'pair-code' && renderPairCode()}
      {flow.step === 'pair-handshake' && renderPairHandshake()}
      {flow.step === 'pair-approve' && renderPairApprove()}
      {flow.step === 'pair-success' && renderPairSuccess()}
      {flow.step === 'capabilities' && renderCapabilities()}
      {flow.step === 'ready' && renderReady()}

      {scanner.renderScannerOverlay()}
      {caps.renderOverlays()}
    </SafeAreaView>
  );
}

const s = StyleSheet.create({
  root: { flex: 1 },
  flex1: { flex: 1 },

  topBar: {
    minHeight: 52,
    paddingHorizontal: 16,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  topSide: { width: 72, alignItems: 'flex-start', justifyContent: 'center' },
  topSideRight: { alignItems: 'flex-end' },
  stepPill: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    paddingHorizontal: 11,
    paddingVertical: 5,
    borderRadius: 99,
  },
  stepDot: { width: 6, height: 6, borderRadius: 3 },
  stepPillText: { fontSize: 12, fontWeight: '600', fontVariant: ['tabular-nums'] },
  skipBtn: { paddingVertical: 8, paddingHorizontal: 6 },
  skipText: { fontSize: 15, fontWeight: '600' },

  pager: { flex: 1 },
  slidePage: { flex: 1 },
  slideContent: {
    flexGrow: 1,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 26,
    paddingVertical: 16,
    gap: 22,
  },
  slideCopy: { width: '100%', maxWidth: 340, alignItems: 'center' },
  heroTitle: { fontSize: 27, lineHeight: 35, fontWeight: '700', textAlign: 'center' },
  heroSlogan: { fontSize: 16, fontWeight: '600', textAlign: 'center', marginTop: 6 },
  heroBody: { fontSize: 15, lineHeight: 22, textAlign: 'center', marginTop: 10 },
  trustRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 16,
    marginTop: 18,
  },
  trustItem: { flexDirection: 'row', alignItems: 'center', gap: 5 },
  trustText: { fontSize: 12, fontWeight: '500' },

  dots: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 7,
    marginBottom: 6,
  },
  dot: { height: 7, borderRadius: 4 },

  scrollContent: {
    flexGrow: 1,
    paddingHorizontal: 18,
    paddingTop: 6,
    paddingBottom: 20,
    alignSelf: 'center',
    width: '100%',
    maxWidth: 480,
  },
  pageHead: { paddingHorizontal: 2, paddingBottom: 20 },
  pageTitle: { fontSize: 26, lineHeight: 33, fontWeight: '700' },
  pageSubtitle: { fontSize: 14.5, lineHeight: 21, marginTop: 6 },

  pairHeroCard: {
    borderRadius: 22,
    borderWidth: StyleSheet.hairlineWidth,
    padding: 18,
    gap: 14,
    marginBottom: 14,
  },
  pairHeroTop: { flexDirection: 'row', alignItems: 'center', gap: 14 },
  qrBadge: {
    width: 50,
    height: 50,
    borderRadius: 15,
    alignItems: 'center',
    justifyContent: 'center',
  },
  cardTitle: { fontSize: 17, fontWeight: '700' },
  cardSub: { fontSize: 13, lineHeight: 18, marginTop: 2 },

  groupWrap: {
    borderRadius: 20,
    overflow: 'hidden',
  },
  iosRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    paddingHorizontal: 15,
    paddingVertical: 14,
    minHeight: 66,
  },
  m3Row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 14,
    paddingHorizontal: 16,
    paddingVertical: 14,
    minHeight: 72,
  },
  iosIconTile: {
    width: 32,
    height: 32,
    borderRadius: 8,
    alignItems: 'center',
    justifyContent: 'center',
  },
  m3IconCircle: {
    width: 40,
    height: 40,
    borderRadius: 20,
    alignItems: 'center',
    justifyContent: 'center',
  },
  rowTitle: { fontSize: 15.5, lineHeight: 21, fontWeight: '600' },
  rowSub: { fontSize: 12.5, lineHeight: 17, marginTop: 2 },

  myCodeCard: {
    marginTop: 14,
    padding: 18,
    borderRadius: 18,
    alignItems: 'center',
    gap: 6,
  },
  myCodeDigits: {
    fontSize: 30,
    fontWeight: '800',
    letterSpacing: 4,
    fontVariant: ['tabular-nums'],
  },
  myCodeMeta: { flexDirection: 'row', alignItems: 'center', gap: 14 },
  myCodeMetaText: { fontSize: 12.5, fontVariant: ['tabular-nums'] },
  inlineLink: { flexDirection: 'row', alignItems: 'center', gap: 4, paddingVertical: 4 },
  inlineLinkText: { fontSize: 13, fontWeight: '600' },

  codePanel: {
    borderRadius: 20,
    padding: 18,
    gap: 14,
  },
  otpGrid: {
    flexDirection: 'row',
    gap: 8,
    justifyContent: 'space-between',
  },
  otpBox: {
    flex: 1,
    height: 54,
    borderRadius: 14,
    borderWidth: 1.5,
    fontSize: 22,
    fontWeight: '700',
    fontVariant: ['tabular-nums'],
  },
  ticketInput: {
    minHeight: 84,
    borderRadius: 14,
    borderWidth: 1,
    padding: 12,
    fontSize: 13.5,
    textAlignVertical: 'top',
  },
  codeHelperRow: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  codeHelperText: { fontSize: 12.5 },

  centerStage: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    paddingHorizontal: 24,
  },
  hsCard: {
    width: '100%',
    maxWidth: 360,
    borderRadius: 22,
    padding: 24,
    alignItems: 'center',
    gap: 14,
  },
  hsTitle: { fontSize: 19, fontWeight: '700' },
  hsList: { width: '100%', gap: 10, marginTop: 4 },
  hsRow: { flexDirection: 'row', alignItems: 'center', gap: 10 },
  hsIconSlot: { width: 20, height: 20, alignItems: 'center', justifyContent: 'center' },
  hsBullet: { width: 6, height: 6, borderRadius: 3 },
  hsText: { fontSize: 14 },

  fpCard: {
    borderRadius: 22,
    padding: 18,
    alignItems: 'center',
  },
  fpPeer: {
    width: '100%',
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    paddingBottom: 14,
    marginBottom: 14,
    borderBottomWidth: StyleSheet.hairlineWidth,
  },
  fpPeerIcon: {
    width: 44,
    height: 44,
    borderRadius: 13,
    alignItems: 'center',
    justifyContent: 'center',
  },
  fpPeerName: { fontSize: 16, fontWeight: '600' },
  fpPeerId: { fontSize: 12, marginTop: 2 },
  fpLabel: { fontSize: 12, fontWeight: '600', marginBottom: 8 },
  fpBox: {
    width: '100%',
    paddingVertical: 14,
    paddingHorizontal: 12,
    borderRadius: 14,
    alignItems: 'center',
  },
  fpValue: {
    fontSize: 17,
    lineHeight: 27,
    fontWeight: '700',
    letterSpacing: 1.5,
    textAlign: 'center',
    fontVariant: ['tabular-nums'],
  },
  fpHint: { fontSize: 12.5, textAlign: 'center', marginTop: 12 },

  capPillBtn: {
    paddingHorizontal: 14,
    paddingVertical: 7,
    borderRadius: 99,
  },
  capPillText: { fontSize: 13, fontWeight: '600' },

  readyHero: {
    alignItems: 'center',
    paddingTop: 22,
    paddingBottom: 24,
  },
  checkCircle: {
    width: 68,
    height: 68,
    borderRadius: 34,
    alignItems: 'center',
    justifyContent: 'center',
    marginBottom: 16,
  },
  readyTitle: { fontSize: 25, fontWeight: '700', textAlign: 'center' },
  readySub: { fontSize: 14.5, lineHeight: 21, textAlign: 'center', marginTop: 6 },
  statusTag: {
    paddingHorizontal: 10,
    paddingVertical: 4,
    borderRadius: 99,
  },
  statusTagText: { fontSize: 12, fontWeight: '600' },

  errBox: {
    marginTop: 14,
    padding: 13,
    borderRadius: 14,
  },
  errBoxText: { fontSize: 13, lineHeight: 18.5 },
  errorBanner: { fontSize: 13.5, textAlign: 'center', marginTop: 14 },
  errorText: { fontSize: 13.5, textAlign: 'center' },

  footer: {
    paddingHorizontal: 20,
    paddingTop: 10,
    paddingBottom: 12,
    alignSelf: 'center',
    width: '100%',
    maxWidth: 480,
    gap: 10,
  },
  primaryHost: { minHeight: 54, width: '100%' },
  secondarySlot: { height: 44, width: '100%', justifyContent: 'center' },
  secondaryHost: { minHeight: 44, width: '100%' },
});
