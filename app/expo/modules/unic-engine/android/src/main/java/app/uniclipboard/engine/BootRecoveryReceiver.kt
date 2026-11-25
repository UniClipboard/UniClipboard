package app.uniclipboard.engine

import android.Manifest
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log

/**
 * Recovers background sync after device reboot (`BOOT_COMPLETED`) or app update
 * (`MY_PACKAGE_REPLACED`).
 *
 * Because the Go Engine and all three background clipboard monitors run natively in-process
 * without requiring the JS runtime, this receiver first attempts to start `BackgroundSyncService`
 * directly (`connectedDevice` FGS is eligible for background recovery on supported versions).
 * If the OS refuses FGS startup from the broadcast (e.g., OEM restriction or Android 15+ policy),
 * it falls back to posting a one-tap resume notification via [RestartNotifier].
 */
class BootRecoveryReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    val action = intent.action
    if (action != Intent.ACTION_BOOT_COMPLETED && action != Intent.ACTION_MY_PACKAGE_REPLACED) {
      return
    }
    if (!BackgroundSyncPrefs.enabled(context)) return
    if (ServiceStopRecord.lastReason(context) in ServiceStopRecord.USER_CHOSEN) return
    if (!BackgroundSyncPrefs.foregroundNotification(context)) return

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
    ) {
      return
    }

    val started = try {
      BackgroundSyncService.start(context, headlessRecovery = true)
      true
    } catch (error: RuntimeException) {
      Log.w(TAG, "Direct recovery start refused on $action: ${error.javaClass.simpleName}")
      ServiceStopRecord.write(context, ServiceStopRecord.START_DENIED)
      false
    }

    if (!started) {
      RestartNotifier.show(context)
    }
  }

  private companion object {
    const val TAG = "UnicBootRecovery"
  }
}
