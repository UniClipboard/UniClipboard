import { StyleSheet, Text, View } from 'react-native';
import { useTranslation } from 'react-i18next';
import { Bell } from 'lucide-react-native';
import { UnicBrandLogo } from '@/components/UnicBrandLogo';
import { useTheme } from '@/hooks/useTheme';

export function OnboardingSystemArt() {
  const { t } = useTranslation('onboarding');
  const { theme } = useTheme();
  const c = theme.colors;

  return (
    <View
      style={[s.card, { backgroundColor: c.surfaceMid }]}
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
    >
      <View style={[s.row, { backgroundColor: c.surfaceLowest }]}>
        <View style={[s.badge, { backgroundColor: c.accentContainer }]}>
          <Bell size={14} color={c.onAccentContainer as string} />
        </View>
        <View style={s.body}>
          <Text style={[s.title, { color: c.textPrimary }]} numberOfLines={1}>
            {t('guide.slides.androidSyncTitle')}
          </Text>
          <Text style={[s.sub, { color: c.textSecondary }]} numberOfLines={1}>
            {t('guide.slides.androidSyncSub')}
          </Text>
        </View>
      </View>

      <View style={[s.row, { backgroundColor: c.surfaceLowest }]}>
        <UnicBrandLogo size={28} borderRadius={8} />
        <View style={s.body}>
          <Text style={[s.title, { color: c.textPrimary }]} numberOfLines={1}>
            {t('guide.slides.androidShareTitle')}
          </Text>
          <Text style={[s.sub, { color: c.textSecondary }]} numberOfLines={1}>
            {t('guide.slides.androidShareSub')}
          </Text>
        </View>
      </View>
    </View>
  );
}

const s = StyleSheet.create({
  card: {
    width: '100%',
    maxWidth: 300,
    borderRadius: 24,
    padding: 14,
    gap: 10,
  },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 11,
    paddingVertical: 12,
    paddingHorizontal: 13,
    borderRadius: 16,
  },
  badge: {
    width: 28,
    height: 28,
    borderRadius: 14,
    alignItems: 'center',
    justifyContent: 'center',
  },
  body: { flex: 1 },
  title: { fontSize: 12.5, fontWeight: '600' },
  sub: { fontSize: 10.5, marginTop: 2 },
});
