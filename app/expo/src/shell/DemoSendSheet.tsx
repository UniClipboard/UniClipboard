import { Text, Pressable, View } from "react-native";
import { useTranslation } from "react-i18next";
import { AppBottomSheet } from "@/components/ui";
import { useTheme } from "@/hooks/useTheme";
import { showUnavailable } from "./notice";

export function DemoSendSheet({
  visible,
  onClose,
}: {
  visible: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const { theme } = useTheme();
  return (
    <AppBottomSheet visible={visible} onDismiss={onClose}>
      <View style={{ padding: 16, gap: 16 }}>
        <Text
          accessibilityRole="header"
          style={{
            color: theme.colors.textPrimary,
            fontSize: 22,
            fontWeight: "600",
          }}
        >
          {t("home:detail.sendTo")}
        </Text>
        <Text style={{ color: theme.colors.textSecondary }}>
          {t("shell:unavailable")}
        </Text>
        {["Demo Mac", "Demo Phone"].map((name) => (
          <Pressable
            key={name}
            testID={`send-${name}`}
            onPress={showUnavailable}
            style={{
              padding: 20,
              borderRadius: 16,
              backgroundColor: theme.colors.surface,
            }}
          >
            <Text style={{ color: theme.colors.textPrimary }}>{name}</Text>
            <Text style={{ color: theme.colors.textSecondary }}>
              {t("shell:disconnected")}
            </Text>
          </Pressable>
        ))}
        <Pressable
          testID="send-close"
          onPress={onClose}
          style={{ padding: 16 }}
        >
          <Text style={{ color: theme.colors.accent }}>
            {t("common:action.close")}
          </Text>
        </Pressable>
      </View>
    </AppBottomSheet>
  );
}
