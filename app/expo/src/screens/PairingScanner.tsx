import { useRef } from "react";
import { CameraView, useCameraPermissions } from "expo-camera";
import { Linking, Text } from "react-native";
import { useTranslation } from "react-i18next";
import { AppButton, AppHost } from "@/components/ui";
import { useTheme } from "@/hooks/useTheme";

export function PairingScanner({ onRead }: { onRead: (value: string) => void }) {
  const { t } = useTranslation("settingsSync");
  const { theme } = useTheme();
  const [permission, requestPermission] = useCameraPermissions();
  const read = useRef(false);

  if (permission?.granted) {
    return (
      <CameraView
        style={{ height: 280, borderRadius: 12, overflow: "hidden" }}
        barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
        onBarcodeScanned={({ data }) => {
          if (read.current) return;
          read.current = true;
          onRead(data);
        }}
      />
    );
  }

  return (
    <>
      <Text style={{ color: theme.colors.textPrimary }}>{t("pairing.cameraPermission")}</Text>
      {permission && (
        <AppHost matchContents={{ vertical: true }}>
          <AppButton
            title={t(permission.canAskAgain ? "pairing.allowCamera" : "pairing.openSettings")}
            onPress={() => void (permission.canAskAgain ? requestPermission() : Linking.openSettings())}
          />
        </AppHost>
      )}
    </>
  );
}
