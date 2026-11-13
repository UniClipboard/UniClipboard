import { Image, StyleSheet, View } from 'react-native';

const illustrations = {
  history: require('../../../assets/onboarding-history.png'),
  sync: require('../../../assets/onboarding-device-sync.png'),
  settings: require('../../../assets/onboarding-scan-pair.png'),
};

export function OnboardingArtwork({ kind }: { kind: keyof typeof illustrations }) {
  return (
    <View
      style={s.root}
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
    >
      <Image
        source={illustrations[kind]}
        style={s.illustration}
        resizeMode="contain"
        accessibilityIgnoresInvertColors
      />
    </View>
  );
}

const s = StyleSheet.create({
  root: { width: '100%', height: '100%', alignItems: 'center', justifyContent: 'center' },
  illustration: { width: '100%', height: '100%' },
});
