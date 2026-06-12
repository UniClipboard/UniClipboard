package expo.modules.foregroundservice

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import expo.modules.nativeutil.NativeLogger
import androidx.core.app.NotificationCompat

class SyncForegroundService : Service() {

    companion object {
        private const val TAG = "SyncForegroundService"
        const val CHANNEL_ID = "syncclipboard_foreground"
        const val CHANNEL_NAME = "后台任务"
        const val NOTIFY_ID = 0x2020
        const val ACTION_START = "START"
        const val ACTION_STOP = "STOP"
        const val ACTION_TEMP_STOP = "TEMP_STOP"
        const val ACTION_UPDATE = "UPDATE"
        const val EXTRA_CONTENT = "content"
        private const val RESTART_NOTIFY_ID = 0x2021
        private const val RESTART_CHANNEL_ID = "syncclipboard_restart"
        private const val RESTART_CHANNEL_NAME = "服务重启提醒"

        var isRunning = false
            private set

        /** 标记是否为用户主动停止（ACTION_STOP / ACTION_TEMP_STOP / JS 侧 stopService），
         *  区分系统杀掉与用户操作，onDestroy 时据此决定是否发通知 */
        internal var stoppedByUser = false
    }

    private var notificationManager: NotificationManager? = null

    override fun onCreate() {
        super.onCreate()
        NativeLogger.d(TAG, "Service onCreate")
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        NativeLogger.d(TAG, "onStartCommand action=${intent?.action} flags=$flags startId=$startId")
        when (intent?.action) {
            ACTION_START, null -> {
                NativeLogger.d(TAG, "Starting foreground, intent action=${intent?.action}")

                // 系统 START_STICKY 重启时：
                //   - intent 为 null：系统直接重启
                //   - intent.action == ACTION_START 但 jsInitiatedService == false：
                //     系统重投了上次的 ACTION_START intent，JS 并未实际运行
                // 以上两种情况：JS 不存在，不启动前台服务，仅发重启引导通知
                if (intent == null || !ForegroundServiceModule.isJsRuntimeAlive()) {
                    NativeLogger.w(TAG, "Service restarted by system (intent=${intent?.action}, jsAlive=${ForegroundServiceModule.isJsRuntimeAlive()}), JS not running, showing restart notification")
                    showRestartNotification()
                    stoppedByUser = true  // 防止 onDestroy 再次发重启通知
                    stopSelf()
                    return START_NOT_STICKY
                }

                val notification = createNotification("后台任务运行中")
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                    startForeground(NOTIFY_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
                } else {
                    startForeground(NOTIFY_ID, notification)
                }
                NativeLogger.d(TAG, "startForeground called successfully")
                isRunning = true
            }
            ACTION_STOP -> {
                NativeLogger.d(TAG, "Stopping foreground service (permanent)")
                stoppedByUser = true
                if (!isRunning) {
                    val notification = createNotification("正在停止...")
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                        startForeground(NOTIFY_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
                    } else {
                        startForeground(NOTIFY_ID, notification)
                    }
                }
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                isRunning = false
                // Send event to JS to disable background tasks permanently
                ForegroundServiceModule.sendStopEvent()
            }
            ACTION_TEMP_STOP -> {
                NativeLogger.d(TAG, "Stopping foreground service (temporary)")
                stoppedByUser = true
                if (!isRunning) {
                    val notification = createNotification("正在停止...")
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                        startForeground(NOTIFY_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
                    } else {
                        startForeground(NOTIFY_ID, notification)
                    }
                }
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                isRunning = false
                // Send temp stop event to JS (no settings change, service restarts next time)
                ForegroundServiceModule.sendTempStopEvent()
            }
            ACTION_UPDATE -> {
                val content = intent.getStringExtra(EXTRA_CONTENT) ?: "后台任务运行中"
                updateNotification(content)
            }
            else -> {
                // Unknown action - still need to call startForeground to prevent crash
                val notification = createNotification("后台任务运行中")
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                    startForeground(NOTIFY_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
                } else {
                    startForeground(NOTIFY_ID, notification)
                }
                isRunning = true
            }
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    /**
     * Android 14+ 回调：dataSync 类型前台服务 6小时/24小时配额耗尽时由系统调用。
     * 若不在此回调中及时停止，系统会强制 ANR 终止进程（不经过 onDestroy 优雅路径）。
     * 处理同用户临时停止：通知 JS 侧重新调度，下次 App 进入前台时重启服务。
     */
    @androidx.annotation.RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
    override fun onTimeout(startId: Int) {
        NativeLogger.w(TAG, "dataSync foreground service timed out (6h/24h quota exhausted), stopping gracefully")
        stoppedByUser = true   // 防止 onDestroy 重复发通知
        showRestartNotification(contentText = "系统限制后台任务在主界面关闭后24小时内最多运行6小时，点击重新启动")
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
        isRunning = false
        // 不调用 sendTempStopEvent()：此为系统强制超时，非用户主动停止。
        // 保持 JS 侧 isTempDisabledBackgroundTasks=false，
        // 用户打开 App 后 start() 可自动重试启动服务。
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        super.onTaskRemoved(rootIntent)
        NativeLogger.d(TAG, "onTaskRemoved: user swiped app from recents, stopping service")
        stoppedByUser = true
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
        isRunning = false
        ForegroundServiceModule.sendTempStopEvent()
    }

    override fun onDestroy() {
        NativeLogger.d(TAG, "onDestroy called, stoppedByUser=$stoppedByUser, isRunning=$isRunning")
        val wasRunning = isRunning
        isRunning = false
        // 非用户主动停止且之前确实在运行 → 可能被系统杀死，发通知引导重启
        if (!stoppedByUser && wasRunning) {
            NativeLogger.w(TAG, "Service destroyed unexpectedly, showing restart notification")
            showRestartNotification()
        }
        stoppedByUser = false
        super.onDestroy()
    }

    private fun createNotificationChannel() {
        notificationManager = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                CHANNEL_NAME,
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "SyncClipboard 后台同步服务"
                setShowBadge(false)
            }
            notificationManager?.createNotificationChannel(channel)
        }
    }

    private fun createNotification(content: String): Notification {
        // PendingIntent to open the app when notification is tapped
        val launchIntent = packageManager.getLaunchIntentForPackage(packageName)
        val pendingLaunchIntent = PendingIntent.getActivity(
            this, 0, launchIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        // Temp stop action
        val tempStopIntent = Intent(this, SyncForegroundService::class.java).apply {
            action = ACTION_TEMP_STOP
        }
        val tempStopPendingIntent = PendingIntent.getService(
            this, 2, tempStopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        // Stop action
        val stopIntent = Intent(this, SyncForegroundService::class.java).apply {
            action = ACTION_STOP
        }
        val stopPendingIntent = PendingIntent.getService(
            this, 1, stopIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val iconResId = applicationContext.resources.getIdentifier(
            "ic_notification", "drawable", packageName
        ).takeIf { it != 0 }
            ?: applicationContext.resources.getIdentifier(
                "ic_launcher_foreground", "mipmap", packageName
            ).takeIf { it != 0 }
            ?: android.R.drawable.ic_menu_info_details

        NativeLogger.d(TAG, "Notification icon resId=$iconResId")

        // 内容以 \n 分割为标题和正文
        val lines = content.split("\n", limit = 2)
        val title = lines[0]
        val body = if (lines.size > 1) lines[1] else ""

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle(title)
            .setContentText(body)
            .setSmallIcon(iconResId)
            .setContentIntent(pendingLaunchIntent)
            .setOngoing(true)
            .setSilent(true)
            .addAction(0, "临时停止", tempStopPendingIntent)
            .addAction(0, "永久停止", stopPendingIntent)
            .setStyle(NotificationCompat.BigTextStyle()
                .setBigContentTitle(title)
                .bigText(body)
            )
            .build()
    }

    private fun updateNotification(content: String) {
        val notification = createNotification(content)
        notificationManager?.notify(NOTIFY_ID, notification)
    }

    private fun showRestartNotification(contentText: String = "点击恢复后台服务") {
        val nm = getSystemService(NOTIFICATION_SERVICE) as? NotificationManager ?: return

        // 创建独立的通知渠道（重要性设为 HIGH 以弹出 heads-up）
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                RESTART_CHANNEL_ID,
                RESTART_CHANNEL_NAME,
                NotificationManager.IMPORTANCE_HIGH
            ).apply {
                description = "后台服务被系统终止后重启提醒"
            }
            nm.createNotificationChannel(channel)
        }

        // 启动 ServiceRestartActivity（自动恢复服务后退出）
        val restartIntent = Intent().apply {
            setClassName(packageName, "com.jericx.syncclipboardmobile.servicerestart.ServiceRestartActivity")
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        }
        val pendingIntent = PendingIntent.getActivity(
            this, 0, restartIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        val iconResId = applicationContext.resources.getIdentifier(
            "ic_notification", "drawable", packageName
        ).takeIf { it != 0 }
            ?: applicationContext.resources.getIdentifier(
                "ic_launcher_foreground", "mipmap", packageName
            ).takeIf { it != 0 }
            ?: android.R.drawable.ic_menu_info_details

        val notification = NotificationCompat.Builder(this, RESTART_CHANNEL_ID)
            .setContentTitle("后台服务已停止")
            .setContentText(contentText)
            .setSmallIcon(iconResId)
            .setContentIntent(pendingIntent)
            .setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .build()

        nm.notify(RESTART_NOTIFY_ID, notification)
    }
}
