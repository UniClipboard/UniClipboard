package expo.modules.smsforwarder

import android.content.ComponentName
import android.content.pm.PackageManager
import android.net.Uri
import expo.modules.nativeutil.NativeLogger
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

/**
 * Expo Module — 提供短信相关的辅助功能。
 *
 * 短信验证码的接收与上传已完全由 Headless JS 任务（[SmsHeadlessTaskService]）处理，
 * 不再需要 JS 侧的动态 BroadcastReceiver 和事件监听。
 *
 * 本模块保留：
 * - readRecentSms：读取最近短信（供设置页调试）
 * - setStaticReceiverEnabled / isStaticReceiverEnabled：启用/禁用静态短信接收器
 */
class SmsForwarderModule : Module() {

    companion object {
        private const val TAG = "SmsForwarderModule"
    }

    override fun definition() = ModuleDefinition {
        Name("SmsForwarderModule")

        Function("readRecentSms") { count: Int ->
            val context = appContext.reactContext ?: return@Function emptyList<Map<String, String>>()
            val messages = mutableListOf<Map<String, String>>()

            val cursor = context.contentResolver.query(
                Uri.parse("content://sms"),
                arrayOf("address", "body", "date"),
                null,
                null,
                "date DESC"
            )

            cursor?.use {
                val addressIdx = it.getColumnIndexOrThrow("address")
                val bodyIdx = it.getColumnIndexOrThrow("body")
                var read = 0
                while (it.moveToNext() && read < count) {
                    messages.add(mapOf(
                        "from" to (it.getString(addressIdx) ?: ""),
                        "body" to (it.getString(bodyIdx) ?: "")
                    ))
                    read++
                }
            }

            messages
        }

        Function("setStaticReceiverEnabled") { enabled: Boolean ->
            val context = appContext.reactContext ?: return@Function false
            val component = ComponentName(context, StaticSmsReceiver::class.java)
            val newState = if (enabled) {
                PackageManager.COMPONENT_ENABLED_STATE_ENABLED
            } else {
                PackageManager.COMPONENT_ENABLED_STATE_DISABLED
            }
            context.packageManager.setComponentEnabledSetting(
                component,
                newState,
                PackageManager.DONT_KILL_APP
            )
            NativeLogger.d(TAG, "StaticSmsReceiver enabled=$enabled")
            true
        }

        Function("isStaticReceiverEnabled") {
            val context = appContext.reactContext ?: return@Function false
            val component = ComponentName(context, StaticSmsReceiver::class.java)
            val state = context.packageManager.getComponentEnabledSetting(component)
            state != PackageManager.COMPONENT_ENABLED_STATE_DISABLED
        }

        Function("updateSmsUploadNotification") { text: String ->
            val context = appContext.reactContext ?: return@Function false
            SmsHeadlessTaskService.updateNotificationText(context, text)
            true
        }

        Function("startSmsUploadCountdown") { code: String ->
            SmsHeadlessTaskService.startCountdown(code)
            true
        }

        Function("extractVerificationCode") { body: String ->
            VerificationCodeExtractor.extract(body)
        }
    }
}
