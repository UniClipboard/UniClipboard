import { showUnavailable } from "@/shell/notice";
export function useOnboardingConnection() {
  return {
    stage: "intro" as "intro" | "confirm" | "success",
    busy: false,
    error: null as string | null,
    intent: null as {
      name: string;
      urls: string[];
      username: string;
      password: string;
    } | null,
    scan: async () => showUnavailable(),
    connect: async () => showUnavailable(),
    close: () => {},
  };
}
