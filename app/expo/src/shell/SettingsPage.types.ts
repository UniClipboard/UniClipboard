import type { ShellGroup } from "./settings";
export type SettingsPageProps = {
  title: string;
  groups: ShellGroup[];
  onBack?: () => void;
};
