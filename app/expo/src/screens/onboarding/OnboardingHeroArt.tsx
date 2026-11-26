import { useEffect, useRef } from 'react';
import { Animated, Easing, StyleSheet, Text, View } from 'react-native';
import { useTranslation } from 'react-i18next';
import { File, Image as ImageIcon, Link, Lock, Type } from 'lucide-react-native';
import { UnicBrandLogo } from '@/components/UnicBrandLogo';
import { useTheme } from '@/hooks/useTheme';
import { OnboardingSystemArt } from './OnboardingSystemArt';

export function OnboardingHeroArt({ kind }: { kind: 'welcome' | 'sync' | 'system' }) {
  if (kind === 'welcome') return <WelcomeArt />;
  if (kind === 'sync') return <SyncFlowArt />;
  return <OnboardingSystemArt />;
}

function WelcomeArt() {
  const { t } = useTranslation('onboarding');
  const { theme } = useTheme();
  const c = theme.colors;
  const floatA = useRef(new Animated.Value(0)).current;
  const floatB = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    const loopA = Animated.loop(
      Animated.sequence([
        Animated.timing(floatA, {
          toValue: -6,
          duration: 2100,
          easing: Easing.inOut(Easing.sin),
          useNativeDriver: true,
        }),
        Animated.timing(floatA, {
          toValue: 0,
          duration: 2100,
          easing: Easing.inOut(Easing.sin),
          useNativeDriver: true,
        }),
      ]),
    );
    const loopB = Animated.loop(
      Animated.sequence([
        Animated.timing(floatB, {
          toValue: 6,
          duration: 2400,
          easing: Easing.inOut(Easing.sin),
          useNativeDriver: true,
        }),
        Animated.timing(floatB, {
          toValue: 0,
          duration: 2400,
          easing: Easing.inOut(Easing.sin),
          useNativeDriver: true,
        }),
      ]),
    );
    loopA.start();
    loopB.start();
    return () => {
      loopA.stop();
      loopB.stop();
    };
  }, [floatA, floatB]);

  const cardBg = theme.isDark ? c.surfaceMid : c.surfaceLowest;
  const cardBorder = c.separator;

  return (
    <View style={s.stage} accessibilityElementsHidden importantForAccessibility="no-hide-descendants">
      <View
        style={[
          s.orb,
          { backgroundColor: c.accent, opacity: theme.isDark ? 0.1 : 0.06 },
        ]}
      />
      <UnicBrandLogo size={78} borderRadius={22} />

      <Animated.View
        style={[
          s.floatCard,
          s.fc1,
          { backgroundColor: cardBg, borderColor: cardBorder, transform: [{ translateY: floatA }] },
        ]}
      >
        <View style={[s.fcIcon, { backgroundColor: 'rgba(0,122,255,0.14)' }]}>
          <Type size={13} color="#007AFF" />
        </View>
        <View>
          <Text style={[s.fcTitle, { color: c.textPrimary }]} numberOfLines={1}>
            {t('guide.slides.cardTextTitle')}
          </Text>
          <Text style={[s.fcSub, { color: c.textSecondary }]} numberOfLines={1}>
            {t('guide.slides.cardTextKind')}
          </Text>
        </View>
      </Animated.View>

      <Animated.View
        style={[
          s.floatCard,
          s.fc2,
          { backgroundColor: cardBg, borderColor: cardBorder, transform: [{ translateY: floatB }] },
        ]}
      >
        <View style={[s.fcIcon, { backgroundColor: 'rgba(2,132,199,0.15)' }]}>
          <Link size={13} color="#0284C7" />
        </View>
        <View>
          <Text style={[s.fcTitle, { color: c.textPrimary }]} numberOfLines={1}>
            github.com/unic
          </Text>
          <Text style={[s.fcSub, { color: c.textSecondary }]} numberOfLines={1}>
            {t('guide.slides.cardLinkKind')}
          </Text>
        </View>
      </Animated.View>

      <Animated.View
        style={[
          s.floatCard,
          s.fc3,
          { backgroundColor: cardBg, borderColor: cardBorder, transform: [{ translateY: floatB }] },
        ]}
      >
        <View style={[s.fcIcon, { backgroundColor: 'rgba(52,199,89,0.15)' }]}>
          <ImageIcon size={13} color="#248A3D" />
        </View>
        <View>
          <Text style={[s.fcTitle, { color: c.textPrimary }]} numberOfLines={1}>
            {t('guide.slides.cardImageTitle')}
          </Text>
          <Text style={[s.fcSub, { color: c.textSecondary }]} numberOfLines={1}>
            {t('guide.slides.cardImageKind')}
          </Text>
        </View>
      </Animated.View>

      <Animated.View
        style={[
          s.floatCard,
          s.fc4,
          { backgroundColor: cardBg, borderColor: cardBorder, transform: [{ translateY: floatA }] },
        ]}
      >
        <View style={[s.fcIcon, { backgroundColor: 'rgba(255,149,0,0.15)' }]}>
          <File size={13} color="#D97706" />
        </View>
        <View>
          <Text style={[s.fcTitle, { color: c.textPrimary }]} numberOfLines={1}>
            {t('guide.slides.cardFileTitle')}
          </Text>
          <Text style={[s.fcSub, { color: c.textSecondary }]} numberOfLines={1}>
            {t('guide.slides.cardFileKind')}
          </Text>
        </View>
      </Animated.View>
    </View>
  );
}

function SyncFlowArt() {
  const { t } = useTranslation('onboarding');
  const { theme } = useTheme();
  const c = theme.colors;
  const pulse = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    const loop = Animated.loop(
      Animated.timing(pulse, {
        toValue: 1,
        duration: 2200,
        easing: Easing.inOut(Easing.ease),
        useNativeDriver: true,
      }),
    );
    loop.start();
    return () => loop.stop();
  }, [pulse]);

  const cardBg = theme.isDark ? c.surfaceMid : c.surfaceLowest;
  const softBg = theme.isDark ? c.surfaceHigh : c.fillQuaternary;

  return (
    <View style={s.syncScene} accessibilityElementsHidden importantForAccessibility="no-hide-descendants">
      {/* Desktop Mock */}
      <View style={s.macWrap}>
        <View style={[s.macBody, { backgroundColor: cardBg, borderColor: c.separator }]}>
          <View style={s.macDots}>
            <View style={[s.macDot, { backgroundColor: c.separator }]} />
            <View style={[s.macDot, { backgroundColor: c.separator }]} />
            <View style={[s.macDot, { backgroundColor: c.separator }]} />
          </View>
          <View style={[s.skelLine, { width: '74%', backgroundColor: softBg }]} />
          <View style={[s.skelLine, { width: '50%', backgroundColor: softBg }]} />
          <View style={[s.macClip, { backgroundColor: c.accentContainer }]}>
            <Text style={[s.macClipText, { color: c.onAccentContainer }]} numberOfLines={1}>
              ⌘C · git push
            </Text>
          </View>
        </View>
        <View style={[s.macBase, { backgroundColor: c.separator }]} />
      </View>

      {/* Bridge */}
      <View style={s.bridge}>
        <View style={[s.e2eeBadge, { backgroundColor: cardBg, borderColor: c.separator }]}>
          <Lock size={10} color={c.success as string} />
          <Text style={[s.e2eeText, { color: c.success }]}>E2EE</Text>
        </View>
        <View style={[s.bridgeLine, { borderColor: c.accent }]} />
        <Animated.View
          style={[
            s.packet,
            {
              backgroundColor: c.accent,
              opacity: pulse.interpolate({
                inputRange: [0, 0.15, 0.85, 1],
                outputRange: [0, 1, 1, 0],
              }),
              transform: [
                {
                  translateX: pulse.interpolate({
                    inputRange: [0, 1],
                    outputRange: [-22, 22],
                  }),
                },
              ],
            },
          ]}
        />
      </View>

      {/* Phone Mock */}
      <View style={[s.phoneMock, { backgroundColor: cardBg, borderColor: c.separator }]}>
        <View style={[s.phoneNotch, { backgroundColor: c.separator }]} />
        <View style={[s.phoneToast, { backgroundColor: c.successContainer }]}>
          <Text style={[s.phoneToastText, { color: c.onSuccessContainer }]} numberOfLines={2}>
            {t('guide.slides.syncReceived')}
          </Text>
        </View>
        <View style={[s.phoneSkel, { backgroundColor: softBg }]} />
        <View style={[s.phoneSkel, { backgroundColor: softBg, opacity: 0.6 }]} />
      </View>
    </View>
  );
}

const s = StyleSheet.create({
  stage: {
    width: '100%',
    maxWidth: 324,
    height: 248,
    alignItems: 'center',
    justifyContent: 'center',
  },
  orb: {
    position: 'absolute',
    width: 216,
    height: 216,
    borderRadius: 108,
  },
  floatCard: {
    position: 'absolute',
    flexDirection: 'row',
    alignItems: 'center',
    gap: 8,
    paddingVertical: 8,
    paddingHorizontal: 11,
    borderRadius: 14,
    borderWidth: StyleSheet.hairlineWidth,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 6 },
    shadowOpacity: 0.1,
    shadowRadius: 14,
    elevation: 3,
  },
  fc1: { top: 10, left: 6 },
  fc2: { top: 20, right: 4 },
  fc3: { bottom: 16, left: 10 },
  fc4: { bottom: 8, right: 8 },
  fcIcon: {
    width: 26,
    height: 26,
    borderRadius: 8,
    alignItems: 'center',
    justifyContent: 'center',
  },
  fcTitle: { fontSize: 12, fontWeight: '600' },
  fcSub: { fontSize: 10, marginTop: 1 },

  syncScene: {
    width: '100%',
    maxWidth: 324,
    height: 224,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingHorizontal: 8,
  },
  macWrap: { alignItems: 'center' },
  macBody: {
    width: 136,
    height: 98,
    borderRadius: 13,
    borderWidth: StyleSheet.hairlineWidth,
    padding: 10,
    gap: 6,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 8 },
    shadowOpacity: 0.1,
    shadowRadius: 16,
    elevation: 3,
  },
  macBase: {
    width: 156,
    height: 6,
    borderBottomLeftRadius: 6,
    borderBottomRightRadius: 6,
    marginTop: 2,
    opacity: 0.55,
  },
  macDots: { flexDirection: 'row', gap: 4 },
  macDot: { width: 5, height: 5, borderRadius: 2.5 },
  skelLine: { height: 7, borderRadius: 4 },
  macClip: {
    marginTop: 'auto',
    paddingVertical: 5,
    paddingHorizontal: 7,
    borderRadius: 8,
  },
  macClipText: { fontSize: 10, fontWeight: '600' },

  bridge: {
    flex: 1,
    height: 64,
    alignItems: 'center',
    justifyContent: 'center',
  },
  e2eeBadge: {
    position: 'absolute',
    top: 2,
    flexDirection: 'row',
    alignItems: 'center',
    gap: 3,
    paddingHorizontal: 7,
    paddingVertical: 3,
    borderRadius: 99,
    borderWidth: StyleSheet.hairlineWidth,
  },
  e2eeText: { fontSize: 10, fontWeight: '700' },
  bridgeLine: {
    width: '100%',
    borderTopWidth: 1.5,
    borderStyle: 'dashed',
    opacity: 0.4,
  },
  packet: {
    position: 'absolute',
    width: 9,
    height: 9,
    borderRadius: 4.5,
  },

  phoneMock: {
    width: 96,
    height: 158,
    borderRadius: 20,
    borderWidth: StyleSheet.hairlineWidth,
    padding: 9,
    gap: 7,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 8 },
    shadowOpacity: 0.1,
    shadowRadius: 16,
    elevation: 3,
  },
  phoneNotch: {
    width: 30,
    height: 5,
    borderRadius: 99,
    alignSelf: 'center',
    marginBottom: 2,
  },
  phoneToast: {
    paddingVertical: 6,
    paddingHorizontal: 7,
    borderRadius: 10,
  },
  phoneToastText: { fontSize: 9.5, fontWeight: '600', lineHeight: 13 },
  phoneSkel: { height: 22, borderRadius: 8 },
});
