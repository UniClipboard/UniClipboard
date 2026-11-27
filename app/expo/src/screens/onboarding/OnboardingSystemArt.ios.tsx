import { StyleSheet, Text, View } from 'react-native';
import { useTranslation } from 'react-i18next';
import { ChevronRight } from 'lucide-react-native';
import { UnicBrandLogo } from '@/components/UnicBrandLogo';
import { useTheme } from '@/hooks/useTheme';

export function OnboardingSystemArt() {
  const { t } = useTranslation('onboarding');
  const { theme } = useTheme();
  const c = theme.colors;
  const cardBg = theme.isDark ? c.surfaceMid : c.surfaceLowest;
  const softBg = theme.isDark ? c.surfaceHigh : c.fillQuaternary;

  return (
    <View
      style={[s.card, { backgroundColor: cardBg, borderColor: c.separator }]}
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
    >
      <View style={s.head}>
        <Text style={[s.headLabel, { color: c.textSecondary }]}>
          {t('guide.slides.iosKeyboardLabel')}
        </Text>
        <View style={[s.pill, { backgroundColor: c.successContainer }]}>
          <Text style={[s.pillText, { color: c.onSuccessContainer }]}>
            {t('guide.slides.iosKeyboardPill')}
          </Text>
        </View>
      </View>

      <View style={[s.miniKb, { backgroundColor: softBg }]}>
        <View style={[s.rail, { backgroundColor: cardBg }]}>
          <View style={[s.railDot, { backgroundColor: c.accent }]} />
          <View style={[s.railDot, { backgroundColor: c.separator }]} />
          <View style={[s.railDot, { backgroundColor: c.separator }]} />
        </View>
        <View style={s.kbCards}>
          <View style={[s.kbCard, { backgroundColor: cardBg, borderColor: c.separator }]}>
            <Text style={s.otp}>482 917</Text>
            <Text style={[s.meta, { color: c.textSecondary }]} numberOfLines={1}>
              MacBook Pro
            </Text>
          </View>
          <View style={[s.kbCard, { backgroundColor: cardBg, borderColor: c.separator }]}>
            <Text style={[s.link, { color: c.textPrimary }]} numberOfLines={1}>
              uniclipboard.app
            </Text>
          </View>
        </View>
      </View>

      <View style={[s.shareStrip, { backgroundColor: softBg }]}>
        <UnicBrandLogo size={26} borderRadius={7} />
        <View style={s.shareBody}>
          <Text style={[s.shareTitle, { color: c.textPrimary }]} numberOfLines={1}>
            {t('guide.slides.iosShareTitle')}
          </Text>
          <Text style={[s.shareSub, { color: c.textSecondary }]} numberOfLines={1}>
            {t('guide.slides.iosShareSub')}
          </Text>
        </View>
        <ChevronRight size={15} color={c.textTertiary as string} />
      </View>
    </View>
  );
}

const s = StyleSheet.create({
  card: {
    width: '100%',
    maxWidth: 300,
    borderRadius: 22,
    borderWidth: StyleSheet.hairlineWidth,
    padding: 14,
    gap: 10,
    shadowColor: '#000',
    shadowOffset: { width: 0, height: 10 },
    shadowOpacity: 0.1,
    shadowRadius: 20,
    elevation: 3,
  },
  head: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
  },
  headLabel: { fontSize: 11.5, fontWeight: '600' },
  pill: { paddingHorizontal: 8, paddingVertical: 2, borderRadius: 99 },
  pillText: { fontSize: 10, fontWeight: '600' },
  miniKb: {
    flexDirection: 'row',
    gap: 8,
    padding: 9,
    borderRadius: 15,
  },
  rail: {
    width: 26,
    borderRadius: 9,
    paddingVertical: 6,
    alignItems: 'center',
    justifyContent: 'center',
    gap: 5,
  },
  railDot: { width: 11, height: 11, borderRadius: 3.5 },
  kbCards: { flex: 1, gap: 6 },
  kbCard: {
    paddingVertical: 7,
    paddingHorizontal: 9,
    borderRadius: 10,
    borderWidth: StyleSheet.hairlineWidth,
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 6,
  },
  otp: { fontSize: 12, fontWeight: '700', letterSpacing: 1, color: '#9333EA' },
  meta: { fontSize: 10 },
  link: { fontSize: 11.5, fontWeight: '600' },
  shareStrip: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 9,
    paddingVertical: 8,
    paddingHorizontal: 10,
    borderRadius: 13,
  },
  shareBody: { flex: 1 },
  shareTitle: { fontSize: 12, fontWeight: '600' },
  shareSub: { fontSize: 10.5, marginTop: 1 },
});
