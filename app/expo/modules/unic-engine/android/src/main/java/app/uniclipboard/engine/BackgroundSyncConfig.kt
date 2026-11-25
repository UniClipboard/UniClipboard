package app.uniclipboard.engine

import android.content.Context
import android.net.Uri
import java.io.File

internal enum class ClipboardAccessMethod(val wireValue: String) {
  OVERLAY_POLLING("overlay-polling"),
  OVERLAY_EVENT("overlay-event"),
  SHIZUKU("shizuku");

  companion object {
    fun fromWire(value: String?): ClipboardAccessMethod =
      entries.firstOrNull { it.wireValue == value } ?: OVERLAY_POLLING
  }
}

internal enum class BackgroundSyncNetwork(val wireValue: String) {
  ANY("any"),
  WIFI("wifi");

  companion object {
    fun fromWire(value: String?): BackgroundSyncNetwork =
      entries.firstOrNull { it.wireValue == value } ?: ANY
  }
}

internal sealed class AndroidClipSnapshot {
  object Empty : AndroidClipSnapshot()
  data class Text(val text: String, val html: String) : AndroidClipSnapshot()
  data class ImageUri(val uri: Uri, val mime: String) : AndroidClipSnapshot()
  data class StagedImageFile(val file: File, val mime: String) : AndroidClipSnapshot()
}

internal object BackgroundSyncPrefs {
  private const val FILE = "unic.background-sync"
  private const val KEY_ENABLED = "enabled"
  private const val KEY_FOREGROUND_NOTIFICATION = "foreground_notification"
  private const val KEY_BACKGROUND_DOWNLOAD = "background_download"
  private const val KEY_BACKGROUND_UPLOAD = "background_upload"
  private const val KEY_NETWORK = "network"
  private const val KEY_CLIPBOARD_METHOD = "clipboard_access_method"
  private const val KEY_AUTO_APPLY_RECEIVED = "auto_apply_received"
  private const val KEY_AUTOMATIC_CAPTURE = "automatic_capture"
  private const val KEY_DEVICE_NAME = "device_name"

  /**
   * Process-scoped pause flag triggered by the notification's "Pause" action.
   * Resets automatically when the app process cold-starts or when the user resumes.
   */
  @Volatile
  var tempDisabled: Boolean = false

  private fun prefs(context: Context) =
    context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

  fun enabled(context: Context): Boolean = prefs(context).getBoolean(KEY_ENABLED, false)

  fun setEnabled(context: Context, value: Boolean) {
    prefs(context).edit().putBoolean(KEY_ENABLED, value).apply()
  }

  fun foregroundNotification(context: Context): Boolean =
    prefs(context).getBoolean(KEY_FOREGROUND_NOTIFICATION, true)

  fun setForegroundNotification(context: Context, value: Boolean) {
    prefs(context).edit().putBoolean(KEY_FOREGROUND_NOTIFICATION, value).apply()
  }

  fun backgroundDownload(context: Context): Boolean =
    prefs(context).getBoolean(KEY_BACKGROUND_DOWNLOAD, true)

  fun setBackgroundDownload(context: Context, value: Boolean) {
    prefs(context).edit().putBoolean(KEY_BACKGROUND_DOWNLOAD, value).apply()
  }

  fun backgroundUpload(context: Context): Boolean =
    prefs(context).getBoolean(KEY_BACKGROUND_UPLOAD, true)

  fun setBackgroundUpload(context: Context, value: Boolean) {
    prefs(context).edit().putBoolean(KEY_BACKGROUND_UPLOAD, value).apply()
  }

  fun network(context: Context): BackgroundSyncNetwork =
    BackgroundSyncNetwork.fromWire(prefs(context).getString(KEY_NETWORK, BackgroundSyncNetwork.ANY.wireValue))

  fun setNetwork(context: Context, value: BackgroundSyncNetwork) {
    prefs(context).edit().putString(KEY_NETWORK, value.wireValue).apply()
  }

  fun clipboardAccessMethod(context: Context): ClipboardAccessMethod =
    ClipboardAccessMethod.fromWire(
      prefs(context).getString(KEY_CLIPBOARD_METHOD, ClipboardAccessMethod.OVERLAY_POLLING.wireValue)
    )

  fun setClipboardAccessMethod(context: Context, value: ClipboardAccessMethod) {
    prefs(context).edit().putString(KEY_CLIPBOARD_METHOD, value.wireValue).apply()
  }

  fun autoApplyReceived(context: Context): Boolean =
    prefs(context).getBoolean(KEY_AUTO_APPLY_RECEIVED, false)

  fun setAutoApplyReceived(context: Context, value: Boolean) {
    prefs(context).edit().putBoolean(KEY_AUTO_APPLY_RECEIVED, value).apply()
  }

  fun automaticCapture(context: Context): Boolean =
    prefs(context).getBoolean(KEY_AUTOMATIC_CAPTURE, true)

  fun setAutomaticCapture(context: Context, value: Boolean) {
    prefs(context).edit().putBoolean(KEY_AUTOMATIC_CAPTURE, value).apply()
  }

  fun deviceName(context: Context): String? =
    prefs(context).getString(KEY_DEVICE_NAME, null)?.trim()?.takeIf { it.isNotEmpty() }

  fun setDeviceName(context: Context, value: String?) {
    val clean = value?.trim()?.takeIf { it.isNotEmpty() }
    prefs(context).edit().apply {
      if (clean == null) remove(KEY_DEVICE_NAME) else putString(KEY_DEVICE_NAME, clean)
    }.apply()
  }

  fun isEffectivelyEnabled(context: Context): Boolean = enabled(context) && !tempDisabled
}

internal object ServiceStopRecord {
  const val USER_STOP = "user_stop"
  const val USER_PAUSE = "user_pause"
  const val SYSTEM_TIMEOUT = "system_timeout"
  const val START_DENIED = "start_denied"
  const val SYSTEM_RESTART = "system_restart"
  const val POLICY = "policy"

  val USER_CHOSEN = setOf(USER_STOP, USER_PAUSE, POLICY)

  private const val FILE = "unic.background-service-stop"
  private const val KEY_REASON = "last_stop_reason"
  private const val KEY_AT = "last_stop_at"

  private fun prefs(context: Context) =
    context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

  fun write(context: Context, reason: String) {
    prefs(context).edit()
      .putString(KEY_REASON, reason)
      .putLong(KEY_AT, System.currentTimeMillis())
      .apply()
  }

  fun clear(context: Context) {
    prefs(context).edit().clear().apply()
  }

  fun lastReason(context: Context): String? = prefs(context).getString(KEY_REASON, null)

  fun lastAt(context: Context): Long? =
    prefs(context).getLong(KEY_AT, 0L).takeIf { it > 0L }
}
