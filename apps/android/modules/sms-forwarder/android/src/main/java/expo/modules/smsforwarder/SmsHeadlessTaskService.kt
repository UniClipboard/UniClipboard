package expo.modules.smsforwarder

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Bundle
import expo.modules.nativeutil.NativeLogger
import androidx.core.app.NotificationCompat
import com.facebook.react.HeadlessJsTaskService
import com.facebook.react.bridge.Arguments
import com.facebook.react.jstasks.HeadlessJsTaskConfig

/**
 * Headless JS 任务服务 — 在后台（无 Activity / 无 UI）启动 React Native JS 运行时，
 * 执行短信验证码提取与上传。
 *
 * 由 [StaticSmsReceiver] 通过 `startForegroundService()` 启动。
 * 启动后立即调用 `startForeground()` 以满足 Android 8+ 要求。
 *
 * 上传成功后发送成功通知（30 秒后自动清除），随即停止前台服务。
 * 新 SMS 到来会替换通知并重新处理。
 */
class SmsHeadlessTaskService : HeadlessJsTaskService() {

    companion object {
        private const val TAG = "SmsHeadlessTask"
        const val CHANNEL_ID = "syncclipboard_sms_headless"
        const val CHANNEL_NAME = "短信验证码上传"
        const val NOTIFICATION_ID = 0x2022
        const val TASK_NAME = "SmsUploadTask"
        const val ACTION_DISMISS = "expo.modules.smsforwarder.ACTION_DISMISS_COUNTDOWN"
        /** 任务超时：60 秒（含重试时间） */
        private const val TASK_TIMEOUT_MS = 60_000L
        /** 成功通知自动清除时间（毫秒） */
        private const val SUCCESS_NOTIFICATION_TIMEOUT_MS = 30_000L

        @Volatile
        private var instance: SmsHeadlessTaskService? = null

        /**
         * JS 侧上传成功后设置的验证码。
         * 主路径：[startCountdown] 直接发送成功通知。
         * 兆底：[onHeadlessJsTaskFinish] 检查此变量，确保通知不会遗漏。
         */
        @Volatile
        var pendingSuccessCode: String? = null

        /**
         * 从任意位置更新 Headless 服务的前台通知文本。
         * JS 侧通过 SmsForwarderModule.updateSmsUploadNotification() 调用。
         */
        fun updateNotificationText(context: Context, text: String) {
            val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager
                ?: return
            ensureNotificationChannel(context, nm)
            nm.notify(NOTIFICATION_ID, buildStaticNotification(
                context, "SyncClipboard", text,
                dismissButtonLabel = "取消"
            ))
        }

        /**
         * 由 JS 侧调用：上传成功后发送成功通知并停止服务。
         * 主路径：直接通过服务实例发送通知。
         * 若服务实例不可用，则记录 pendingSuccessCode，
         * 由 [onHeadlessJsTaskFinish] 兆底处理。
         */
        fun startCountdown(code: String) {
            NativeLogger.d(TAG, "startCountdown called with code=$code")
            pendingSuccessCode = code
            // 主路径：直接通过服务实例发送成功通知
            instance?.let { service ->
                if (!service.isSuccessPosted) {
                    pendingSuccessCode = null
                    service.postSuccessNotification(code)
                }
            }
        }

        private fun ensureNotificationChannel(context: Context, nm: NotificationManager) {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val channel = NotificationChannel(
                    CHANNEL_ID,
                    CHANNEL_NAME,
                    NotificationManager.IMPORTANCE_LOW
                ).apply {
                    description = "短信验证码后台上传通知"
                    setShowBadge(false)
                    lockscreenVisibility = Notification.VISIBILITY_PUBLIC
                }
                nm.createNotificationChannel(channel)
            }
        }

        private fun getNotificationIcon(context: Context): Int {
            return context.resources.getIdentifier(
                "ic_notification", "drawable", context.packageName
            ).takeIf { it != 0 }
                ?: context.resources.getIdentifier(
                    "ic_launcher_foreground", "mipmap", context.packageName
                ).takeIf { it != 0 }
                ?: android.R.drawable.ic_menu_info_details
        }

        private fun buildStaticNotification(
            context: Context,
            title: String,
            text: String,
            dismissButtonLabel: String? = null
        ): Notification {
            val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
            ensureNotificationChannel(context, nm)
            val builder = NotificationCompat.Builder(context, CHANNEL_ID)
                .setContentTitle(title)
                .setContentText(text)
                .setSmallIcon(getNotificationIcon(context))
                .setPriority(NotificationCompat.PRIORITY_LOW)
                .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
                .setOngoing(true)
            if (dismissButtonLabel != null) {
                val dismissIntent = Intent(context, SmsHeadlessTaskService::class.java).apply {
                    action = ACTION_DISMISS
                }
                val pendingIntent = PendingIntent.getForegroundService(
                    context, 0, dismissIntent,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
                )
                builder.addAction(0, dismissButtonLabel, pendingIntent)
            }
            return builder.build()
        }
    }

    private var isSuccessPosted = false

    override fun onCreate() {
        NativeLogger.d(TAG, "[A] onCreate entered (before super.onCreate) — process=${android.os.Process.myPid()}")
        try {
            super.onCreate()
            NativeLogger.d(TAG, "[B] super.onCreate() returned successfully")
        } catch (e: Exception) {
            NativeLogger.e(TAG, "[B] super.onCreate() threw ${e.javaClass.simpleName}: ${e.message}", e)
            throw e
        }
        instance = this
        pendingSuccessCode = null
        NativeLogger.d(TAG, "[C] Building foreground notification")
        val notification = buildStaticNotification(this, "SyncClipboard", "正在处理短信验证码…")
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                // shortService: Android 14+ 引入，无 dataSync 的 6小时/24小时累计配额限制，
                // 最长 3 分钟（任务超时 60s，完全满足）。
                // dataSync 在主剪贴板同步服务长期运行后配额耗尽，会导致此处抛出
                // ForegroundServiceStartNotAllowedException，改用 shortService 规避该问题。
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SHORT_SERVICE)
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
            NativeLogger.d(TAG, "[D] startForeground() completed")
        } catch (e: Exception) {
            NativeLogger.e(TAG, "[D] startForeground() threw ${e.javaClass.simpleName}: ${e.message}", e)
            throw e
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        NativeLogger.d(TAG, "[E] onStartCommand entered, action=${intent?.action}, startId=$startId")
        // "取消"按钮触发
        if (intent?.action == ACTION_DISMISS) {
            NativeLogger.d(TAG, "Dismiss action received, stopping service")
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }
        // 新 SMS 到来 → 重置成功状态
        if (isSuccessPosted) {
            isSuccessPosted = false
            NativeLogger.d(TAG, "New SMS arrived, resetting success state")
        }
        return try {
            super.onStartCommand(intent, flags, startId)
        } catch (e: Exception) {
            // 进程冷启动时 React Native JS 运行时可能无法初始化，
            // 基类 HeadlessJsTaskService.startTask() 没有异常保护，会直接崩溃。
            NativeLogger.e(TAG, "HeadlessJsTaskService failed to start React context: " +
                    "${e.javaClass.simpleName}: ${e.message}", e)
            updateNotificationText(this,
                "验证码上传失败：JS 运行时未就绪\n${e.javaClass.simpleName}: ${e.message}")
            // 5 秒后自动关闭通知并停止服务
            android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }, 5000)
            START_NOT_STICKY
        }
    }

    override fun getTaskConfig(intent: Intent?): HeadlessJsTaskConfig? {
        val extras: Bundle = intent?.extras ?: return null
        val from = extras.getString("from", "")
        val body = extras.getString("body", "")
        if (body.isNullOrEmpty()) {
            NativeLogger.w(TAG, "getTaskConfig: body is null or empty, skipping task (from=$from)")
            return null
        }

        NativeLogger.d(TAG, "Creating headless task config for SMS from=$from")

        return HeadlessJsTaskConfig(
            TASK_NAME,
            Arguments.fromBundle(extras),
            TASK_TIMEOUT_MS,
            true  // allowedInForeground: 即使 app 在前台也可以执行
        )
    }

    override fun onHeadlessJsTaskFinish(taskId: Int) {
        if (isSuccessPosted) {
            // 主路径已发送成功通知，不需要额外操作
            NativeLogger.d(TAG, "Headless task finished, success notification already posted")
            return
        }
        // 兆底：检查 pendingSuccessCode（处理桥接异步调用延迟的情况）
        val code = pendingSuccessCode
        if (code != null) {
            pendingSuccessCode = null
            NativeLogger.d(TAG, "Headless task finished, fallback: posting success notification for code=$code")
            postSuccessNotification(code)
        } else {
            NativeLogger.d(TAG, "Headless task finished, no success, stopping service")
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
    }

    override fun onDestroy() {
        instance = null
        super.onDestroy()
    }

    private fun postSuccessNotification(code: String) {
        NativeLogger.d(TAG, "Posting success notification for code=$code, stopping service")
        isSuccessPosted = true
        val nm = getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager ?: return
        ensureNotificationChannel(this, nm)

        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("已上传验证码：$code")
            .setContentText("30秒后自动关闭")
            .setSmallIcon(getNotificationIcon(this))
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setOngoing(false)
            .setAutoCancel(true)
            .setTimeoutAfter(SUCCESS_NOTIFICATION_TIMEOUT_MS)
            .build()

        stopForeground(STOP_FOREGROUND_DETACH)
        nm.notify(NOTIFICATION_ID, notification)
        stopSelf()
    }
}
