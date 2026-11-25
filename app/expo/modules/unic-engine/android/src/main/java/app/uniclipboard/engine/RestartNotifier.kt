package app.uniclipboard.engine

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.annotation.StringRes
import androidx.core.app.NotificationCompat

/**
 * Posts a high-priority "tap to resume background sync" notification when Android refuses a
 * background FGS restart or terminates the foreground service unexpectedly.
 */
internal object RestartNotifier {
  private const val NOTIFY_ID = 8133

  fun show(
    context: Context,
    @StringRes contentResId: Int = R.string.unic_background_sync_restart_content,
  ) {
    val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager ?: return
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      nm.createNotificationChannel(
        NotificationChannel(
          BackgroundSyncService.RESTART_CHANNEL_ID,
          context.getString(R.string.unic_background_sync_restart_channel),
          NotificationManager.IMPORTANCE_HIGH,
        ).apply {
          description = context.getString(R.string.unic_background_sync_restart_channel_desc)
        }
      )
    }

    val launchIntent = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
      addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
    } ?: return

    val pendingIntent = PendingIntent.getActivity(
      context,
      10,
      launchIntent,
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    val notification = NotificationCompat.Builder(context, BackgroundSyncService.RESTART_CHANNEL_ID)
      .setContentTitle(context.getString(R.string.unic_background_sync_restart_title))
      .setContentText(context.getString(contentResId))
      .setSmallIcon(android.R.drawable.stat_notify_sync)
      .setContentIntent(pendingIntent)
      .setAutoCancel(true)
      .setPriority(NotificationCompat.PRIORITY_HIGH)
      .build()

    runCatching { nm.notify(NOTIFY_ID, notification) }
  }

  fun cancel(context: Context) {
    val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as? NotificationManager ?: return
    runCatching { nm.cancel(NOTIFY_ID) }
  }
}
