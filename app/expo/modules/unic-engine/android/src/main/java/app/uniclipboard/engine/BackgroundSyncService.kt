package app.uniclipboard.engine

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.content.res.Configuration
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat

/**
 * Long-running Android foreground service keeping the in-process Go Engine and background
 * clipboard monitors alive across task removal, process restarts, and screen-off periods.
 *
 * Key long-running guarantees:
 * - Uses `FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE` (with `CHANGE_WIFI_MULTICAST_STATE`),
 *   which has NO 6-hour/24-hour Android 15+ timeout cap (unlike `dataSync`).
 * - Declared `android:stopWithTask="false"` and keeps running in `onTaskRemoved`.
 * - Returns `START_STICKY` and natively bootstraps [UnicEngineHost] on system restart without
 *   needing the JS runtime to be alive; falls back to [RestartNotifier] if the OS refuses FGS.
 * - Retains [UnicEngineHost] so UI Activity / JS teardown never shuts down the Go Engine while
 *   background sync is active.
 * - Acquires a `WifiManager.MulticastLock` while active so local Tailcat/mDNS peer discovery
 *   continues working in low-power Wi-Fi states.
 */
class BackgroundSyncService : Service() {
  private var holdsEngine = false
  private var multicastLock: WifiManager.MulticastLock? = null

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onCreate() {
    super.onCreate()
    createNotificationChannels()
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val action = intent?.action
    when (action) {
      ACTION_STOP -> {
        stoppedByUser = true
        if (!running) {
          enterForeground(createNotification(getString(R.string.unic_background_sync_stopping)))
        }
        BackgroundSyncPrefs.setEnabled(this, false)
        BackgroundSyncPrefs.tempDisabled = false
        ServiceStopRecord.write(this, ServiceStopRecord.USER_STOP)
        stopForeground(STOP_FOREGROUND_REMOVE)
        updateRunning(false)
        UnicEngineHost.notifyBackgroundSyncStateChanged(this, "user_stop")
        stopSelf()
        return START_NOT_STICKY
      }

      ACTION_TEMP_STOP -> {
        stoppedByUser = true
        if (!running) {
          enterForeground(createNotification(getString(R.string.unic_background_sync_stopping)))
        }
        BackgroundSyncPrefs.tempDisabled = true
        ServiceStopRecord.write(this, ServiceStopRecord.USER_PAUSE)
        stopForeground(STOP_FOREGROUND_REMOVE)
        updateRunning(false)
        UnicEngineHost.notifyBackgroundSyncStateChanged(this, "user_pause")
        stopSelf()
        return START_NOT_STICKY
      }

      ACTION_START, ACTION_HEADLESS_RECOVERY, null -> {
        val isSystemRestart = intent == null || action == ACTION_HEADLESS_RECOVERY
        if (isSystemRestart) {
          if (!BackgroundSyncPrefs.isEffectivelyEnabled(this) ||
            !BackgroundSyncPrefs.foregroundNotification(this) ||
            ServiceStopRecord.lastReason(this) in ServiceStopRecord.USER_CHOSEN
          ) {
            stoppedByUser = true
            stopSelf()
            return START_NOT_STICKY
          }
        }

        if (!enterForeground(createNotification())) {
          if (isSystemRestart) {
            RestartNotifier.show(this)
          }
          return START_NOT_STICKY
        }

        stoppedByUser = false
        RestartNotifier.cancel(this)
        ServiceStopRecord.clear(this)
        holdEngineAndLocks()
        updateRunning(true)

        if (isSystemRestart) {
          UnicEngineHost.ensureHeadlessEngineStarted(applicationContext)
        }
        return START_STICKY
      }

      else -> {
        if (!enterForeground(createNotification())) return START_NOT_STICKY
        holdEngineAndLocks()
        updateRunning(true)
        return START_STICKY
      }
    }
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    createNotificationChannels()
    if (running) {
      val nm = getSystemService(NotificationManager::class.java)
      runCatching { nm?.notify(NOTIFICATION_ID, createNotification()) }
    }
  }

  override fun onTaskRemoved(rootIntent: Intent?) {
    super.onTaskRemoved(rootIntent)
    // Intentionally keep the foreground service & Go Engine running when the user swipes the
    // app from Recents. Notification actions ("Pause" / "Stop") or Settings remain the explicit
    // way to end background sync.
    Log.i(TAG, "Task removed; keeping background sync service alive")
  }

  override fun onTimeout(startId: Int, fgsType: Int) {
    Log.w(TAG, "Foreground service timeout callback (type=$fgsType); stopping gracefully")
    stoppedByUser = true
    ServiceStopRecord.write(this, ServiceStopRecord.SYSTEM_TIMEOUT)
    RestartNotifier.show(this, R.string.unic_background_sync_timeout_content)
    stopForeground(STOP_FOREGROUND_REMOVE)
    updateRunning(false)
    stopSelf()
  }

  override fun onDestroy() {
    val wasRunning = running
    updateRunning(false)
    if (!stoppedByUser && wasRunning) {
      Log.w(TAG, "Service destroyed unexpectedly; posting restart notification")
      ServiceStopRecord.write(this, ServiceStopRecord.SYSTEM_RESTART)
      RestartNotifier.show(this)
    }
    releaseEngineAndLocks()
    stoppedByUser = false
    super.onDestroy()
  }

  private fun enterForeground(notification: Notification): Boolean {
    return try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        startForeground(
          NOTIFICATION_ID,
          notification,
          ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
        )
      } else {
        startForeground(NOTIFICATION_ID, notification)
      }
      true
    } catch (error: RuntimeException) {
      Log.w(TAG, "startForeground refused: ${error.javaClass.simpleName}")
      stoppedByUser = true
      updateRunning(false)
      ServiceStopRecord.write(this, ServiceStopRecord.START_DENIED)
      stopSelf()
      false
    }
  }

  private fun holdEngineAndLocks() {
    if (holdsEngine) return
    holdsEngine = true
    UnicEngineHost.retainForService(applicationContext)
    acquireMulticastLock()
  }

  private fun releaseEngineAndLocks() {
    if (!holdsEngine) return
    holdsEngine = false
    releaseMulticastLock()
    UnicEngineHost.releaseForService(applicationContext)
  }

  private fun acquireMulticastLock() {
    if (multicastLock?.isHeld == true) return
    multicastLock = runCatching {
      val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
      wifi.createMulticastLock("uniclipboard-mdns").apply {
        setReferenceCounted(false)
        acquire()
      }
    }.onFailure {
      Log.w(TAG, "Multicast lock unavailable: ${it.javaClass.simpleName}")
    }.getOrNull()
  }

  private fun releaseMulticastLock() {
    runCatching { multicastLock?.takeIf { it.isHeld }?.release() }
    multicastLock = null
  }

  private fun createNotificationChannels() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    val nm = getSystemService(NotificationManager::class.java) ?: return
    val foregroundChannel = NotificationChannel(
      CHANNEL_ID,
      getString(R.string.unic_background_sync),
      NotificationManager.IMPORTANCE_LOW,
    ).apply {
      description = getString(R.string.unic_background_sync_channel_desc)
      setShowBadge(false)
    }
    val restartChannel = NotificationChannel(
      RESTART_CHANNEL_ID,
      getString(R.string.unic_background_sync_restart_channel),
      NotificationManager.IMPORTANCE_HIGH,
    ).apply {
      description = getString(R.string.unic_background_sync_restart_channel_desc)
    }
    nm.createNotificationChannels(listOf(foregroundChannel, restartChannel))
  }

  private fun createNotification(contentOverride: String? = null): Notification {
    val launchIntent = packageManager.getLaunchIntentForPackage(packageName)
    val contentIntent = if (launchIntent != null) {
      PendingIntent.getActivity(
        this,
        0,
        launchIntent,
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
      )
    } else {
      null
    }

    val pausePending = PendingIntent.getService(
      this,
      2,
      Intent(this, BackgroundSyncService::class.java).apply { action = ACTION_TEMP_STOP },
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )
    val stopPending = PendingIntent.getService(
      this,
      1,
      Intent(this, BackgroundSyncService::class.java).apply { action = ACTION_STOP },
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    val text = contentOverride ?: getString(R.string.unic_background_sync_running)
    return NotificationCompat.Builder(this, CHANNEL_ID)
      .setSmallIcon(android.R.drawable.stat_notify_sync)
      .setContentTitle(getString(R.string.unic_background_sync))
      .setContentText(text)
      .setOngoing(true)
      .setSilent(true)
      .setOnlyAlertOnce(true)
      .apply {
        if (contentIntent != null) setContentIntent(contentIntent)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
          setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
        }
      }
      .addAction(0, getString(R.string.unic_background_sync_action_pause), pausePending)
      .addAction(0, getString(R.string.unic_background_sync_action_stop), stopPending)
      .build()
  }

  private fun updateRunning(value: Boolean) {
    if (running == value) return
    running = value
    UnicEngineHost.onServiceRunningChanged(applicationContext, value)
  }

  companion object {
    private const val TAG = "UnicBgSyncService"
    const val CHANNEL_ID = "unic-background-sync"
    const val RESTART_CHANNEL_ID = "unic-background-sync-restart"
    private const val NOTIFICATION_ID = 8132
    const val ACTION_START = "app.uniclipboard.engine.action.START"
    const val ACTION_STOP = "app.uniclipboard.engine.action.STOP"
    const val ACTION_TEMP_STOP = "app.uniclipboard.engine.action.TEMP_STOP"
    const val ACTION_HEADLESS_RECOVERY = "app.uniclipboard.engine.action.HEADLESS_RECOVERY"

    @Volatile
    var running: Boolean = false
      private set

    @Volatile
    internal var stoppedByUser: Boolean = false

    fun enabled(context: Context): Boolean = BackgroundSyncPrefs.enabled(context)

    fun setEnabled(context: Context, enabled: Boolean) {
      BackgroundSyncPrefs.setEnabled(context, enabled)
    }

    fun start(context: Context, headlessRecovery: Boolean = false) {
      if (running && !stoppedByUser) return
      stoppedByUser = false
      val intent = Intent(context.applicationContext, BackgroundSyncService::class.java).apply {
        action = if (headlessRecovery) ACTION_HEADLESS_RECOVERY else ACTION_START
      }
      ContextCompat.startForegroundService(context.applicationContext, intent)
    }

    fun stop(context: Context, reason: String = ServiceStopRecord.POLICY) {
      stoppedByUser = true
      ServiceStopRecord.write(context, reason)
      context.applicationContext.stopService(
        Intent(context.applicationContext, BackgroundSyncService::class.java)
      )
    }
  }
}
