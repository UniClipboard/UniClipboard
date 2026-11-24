import type { ShellGroup } from "@/shell/settings";
export type SettingsPageProps = {
  title: string;
  groups: ShellGroup[];
  onBack?: () => void;
};
