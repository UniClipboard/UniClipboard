export interface OnboardingScreenProps {
  /** Saves completion for both skipping and finishing the product introduction. */
  onComplete: () => void | Promise<void>;
}
