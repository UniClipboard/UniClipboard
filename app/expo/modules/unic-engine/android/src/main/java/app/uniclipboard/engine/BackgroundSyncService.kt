package app.uniclipboard.engine

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.content.ContextCompat

class BackgroundSyncService : Service() {
  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val notificationManager = getSystemService(NotificationManager::class.java)
    val builder = if (Build.VERSION.SDK_INT >= 26) {
      notificationManager.createNotificationChannel(NotificationChannel(
        CHANNEL, getString(R.string.unic_background_sync), NotificationManager.IMPORTANCE_LOW
      ))
      Notification.Builder(this, CHANNEL)
    } else {
      @Suppress("DEPRECATION")
      Notification.Builder(this)
    }
    val launch = packageManager.getLaunchIntentForPackage(packageName)
    if (launch != null) {
      builder.setContentIntent(PendingIntent.getActivity(this, 0, launch, PendingIntent.FLAG_IMMUTABLE))
    }
    val notification = builder
      .setSmallIcon(android.R.drawable.stat_notify_sync)
      .setContentTitle(getString(R.string.unic_background_sync))
      .setOngoing(true)
      .setOnlyAlertOnce(true)
      .build()
    try {
      if (Build.VERSION.SDK_INT >= 29) {
        startForeground(NOTIFICATION, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
      } else {
        startForeground(NOTIFICATION, notification)
      }
      updateRunning(true)
    } catch (_: SecurityException) {
      stopSelf()
    }
    return START_NOT_STICKY
  }

  override fun onTimeout(startId: Int, fgsType: Int) {
    updateRunning(false)
    stopSelf()
  }

  override fun onDestroy() {
    updateRunning(false)
    super.onDestroy()
  }

  private fun updateRunning(value: Boolean) {
    if (running == value) return
    running = value
    onRunningChanged?.invoke(value)
  }

  companion object {
    private const val CHANNEL = "unic-background-sync"
    private const val NOTIFICATION = 8132
    private const val SETTINGS = "unic.background-sync"
    private const val ENABLED = "enabled"

    @Volatile var running = false
      private set
    var onRunningChanged: ((Boolean) -> Unit)? = null

    fun enabled(context: Context): Boolean =
      context.getSharedPreferences(SETTINGS, Context.MODE_PRIVATE).getBoolean(ENABLED, false)

    fun setEnabled(context: Context, enabled: Boolean) {
      context.getSharedPreferences(SETTINGS, Context.MODE_PRIVATE).edit().putBoolean(ENABLED, enabled).apply()
    }

    fun start(context: Context) {
      if (!running) ContextCompat.startForegroundService(context, Intent(context, BackgroundSyncService::class.java))
    }

    fun stop(context: Context) {
      context.stopService(Intent(context, BackgroundSyncService::class.java))
    }
  }
}
