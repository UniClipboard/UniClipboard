import { useCallback } from 'react';
import { useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import type { RootStackParamList } from '@/navigation/AppNavigator.types';
import { OnboardingScreen } from './OnboardingScreen';

export function OnboardingPreviewScreen() {
  const navigation =
    useNavigation<NativeStackNavigationProp<RootStackParamList, 'OnboardingPreview'>>();
  const closePreview = useCallback(() => navigation.goBack(), [navigation]);
  return <OnboardingScreen onComplete={closePreview} />;
}
