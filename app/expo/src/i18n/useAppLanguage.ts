import { useTranslation } from "react-i18next";
import { useShellStore } from "@/shell/store";
import i18n from "./index";
import { type LanguagePreference, resolvePreference } from "./languages";
export function applyLanguagePreference(
  pref: LanguagePreference | string | null | undefined,
): void {
  void i18n.changeLanguage(resolvePreference(pref));
}
export function useAppLanguage() {
  const { i18n: instance } = useTranslation();
  const preference = useShellStore((s) => s.language) as LanguagePreference;
  const setLanguage = async (language: LanguagePreference) => {
    useShellStore.getState().setPreference({ language });
    await instance.changeLanguage(resolvePreference(language));
  };
  return { preference, resolvedLanguage: instance.language, setLanguage };
}
