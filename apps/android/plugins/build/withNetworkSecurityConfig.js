"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
const config_plugins_1 = require("expo/config-plugins");
const fs = __importStar(require("fs"));
const path = __importStar(require("path"));
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
function addNetworkSecurityConfig(androidManifest) {
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
const withNetworkSecurityConfig = (config) => {
    config = (0, config_plugins_1.withDangerousMod)(config, [
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
    config = (0, config_plugins_1.withAndroidManifest)(config, (config) => {
        config.modResults = addNetworkSecurityConfig(config.modResults);
        return config;
    });
    return config;
};
exports.default = (0, config_plugins_1.createRunOncePlugin)(withNetworkSecurityConfig, 'withNetworkSecurityConfig', '1.0.0');
