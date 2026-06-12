import {
  AndroidConfig,
  ConfigPlugin,
  withAndroidManifest,
  withDangerousMod,
  createRunOncePlugin,
} from 'expo/config-plugins';
import * as fs from 'fs';
import * as path from 'path';

const NETWORK_SECURITY_CONFIG = `<?xml version="1.0" encoding="utf-8"?>
<network-security-config>
    <base-config cleartextTrafficPermitted="true">
        <trust-anchors>
            <certificates src="system" />
            <certificates src="user" />
        </trust-anchors>
    </base-config>
</network-security-config>
`;

function addNetworkSecurityConfig(
  androidManifest: AndroidConfig.Manifest.AndroidManifest
): AndroidConfig.Manifest.AndroidManifest {
  const { manifest } = androidManifest;

  if (!Array.isArray(manifest.application)) {
    console.warn('withNetworkSecurityConfig: No application array in manifest?');
    return androidManifest;
  }

  const application = manifest.application[0];
  application.$ = application.$ || {};
  application.$['android:networkSecurityConfig'] = '@xml/network_security_config';

  return androidManifest;
}

const withNetworkSecurityConfig: ConfigPlugin = (config) => {
  config = withDangerousMod(config, [
    'android',
    async (config) => {
      const resPath = path.join(config.modRequest.platformProjectRoot, 'app/src/main/res/xml');

      if (!fs.existsSync(resPath)) {
        fs.mkdirSync(resPath, { recursive: true });
      }

      const configPath = path.join(resPath, 'network_security_config.xml');
      fs.writeFileSync(configPath, NETWORK_SECURITY_CONFIG, 'utf-8');

      return config;
    },
  ]);

  config = withAndroidManifest(config, (config) => {
    config.modResults = addNetworkSecurityConfig(config.modResults);
    return config;
  });

  return config;
};

export default createRunOncePlugin(withNetworkSecurityConfig, 'withNetworkSecurityConfig', '1.0.0');
