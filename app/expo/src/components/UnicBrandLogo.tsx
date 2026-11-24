import { StyleSheet, View, type StyleProp, type ViewStyle } from "react-native";
import Svg, { Path, Rect } from "react-native-svg";
import { useTheme } from "@/hooks/useTheme";

const UNIC_LOGO_PATH =
  "M313 274H425V336H482V397H537V460H717V397H772V336H829V274H941V590H1006V663H941V725H1067V785H941V843H1006V909H941V874H878V952H376V874H313V909H247V843H313V785H186V725H313V663H247V590H313ZM392 460H457V523H392ZM797 460H862V523H797ZM454 634H521V742H454ZM733 634H801V742H733ZM565 779H689V829H653V875H601V829H565Z";

export function UnicBrandLogo({
  size = 80,
  borderRadius = 20,
  style,
}: {
  size?: number;
  borderRadius?: number;
  style?: StyleProp<ViewStyle>;
}) {
  const { theme } = useTheme();
  const isDark = theme.isDark;

  return (
    <View
      accessible
      accessibilityRole="image"
      accessibilityLabel="UniClipboard"
      style={[
        styles.frame,
        {
          width: size,
          height: size,
          borderRadius,
          borderColor: isDark ? "rgba(255,255,255,0.16)" : "rgba(0,0,0,0.08)",
          shadowOpacity: isDark ? 0.42 : 0.16,
        },
        style,
      ]}
    >
      <Svg width="100%" height="100%" viewBox="0 0 1254 1254">
        <Rect width="1254" height="1254" fill="#000000" />
        <Path fill="#FFFFFF" fillRule="evenodd" d={UNIC_LOGO_PATH} />
      </Svg>
    </View>
  );
}

const styles = StyleSheet.create({
  frame: {
    backgroundColor: "#000000",
    overflow: "hidden",
    borderWidth: 1,
    shadowColor: "#000000",
    shadowOffset: { width: 0, height: 6 },
    shadowRadius: 14,
    elevation: 4,
  },
});
