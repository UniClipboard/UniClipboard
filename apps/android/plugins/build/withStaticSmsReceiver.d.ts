import { ConfigPlugin } from 'expo/config-plugins';
/**
 * 在 AndroidManifest.xml 中注册静态 SMS BroadcastReceiver。
 * 静态 Receiver 即使 app 被杀或 Doze 模式下仍能接收短信（SMS_RECEIVED 属于隐式广播豁免列表）。
 */
declare const withStaticSmsReceiver: ConfigPlugin;
export default withStaticSmsReceiver;
