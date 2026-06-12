import { ConfigPlugin, withDangerousMod, createRunOncePlugin } from 'expo/config-plugins';
import * as fs from 'fs';
import * as path from 'path';

const NIGHT_COLORS = `<resources>
  <color name="splashscreen_background">#000000</color>
</resources>
`;

const withDarkSplashScreen: ConfigPlugin = (config) => {
  config = withDangerousMod(config, [
    'android',
    async (config) => {
      const nightValuesDir = path.join(
        config.modRequest.platformProjectRoot,
        'app/src/main/res/values-night'
      );

      if (!fs.existsSync(nightValuesDir)) {
        fs.mkdirSync(nightValuesDir, { recursive: true });
      }

      const colorsPath = path.join(nightValuesDir, 'colors.xml');
      fs.writeFileSync(colorsPath, NIGHT_COLORS, 'utf-8');

      return config;
    },
  ]);

  return config;
};

export default createRunOncePlugin(withDarkSplashScreen, 'withDarkSplashScreen', '1.0.0');
