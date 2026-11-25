package app.uniclipboard.engine

import android.app.Application
import android.content.ClipData
import android.content.ClipDescription
import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.DeadObjectException
import android.os.IBinder
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import android.provider.Settings
import android.util.Log
import androidx.annotation.Keep
import org.json.JSONObject
import java.io.FileOutputStream
import java.lang.reflect.InvocationTargetException

/**
 * Privileged Shizuku UserService running in shell UID (2000) to read/write the system clipboard
 * without an overlay window.
 *
 * Hardening vs v1:
 * - Supports Shizuku 13.x Context-injected constructor alongside no-arg fallback so ContentResolver
 *   remains available inside standalone `app_process`.
 * - Clears cached `IClipboard` Binder handle on `DeadObjectException` (e.g. system_server restart)
 *   and re-acquires transparently.
 * - Exposes lightweight `getPrimaryClipStamp()` via `getPrimaryClipDescription` to avoid serializing
 *   full clipboard payloads on idle checks.
 * - Preserves `html` alongside plain text.
 */
@Keep
class ClipboardUserService : IClipboardUserService.Stub {
  private val serviceContext: Context?

  @Keep
  constructor() : super() {
    serviceContext = null
  }

  @Keep
  constructor(context: Context) : super() {
    serviceContext = context
  }

  companion object {
    private const val TAG = "UnicShizukuService"
    private const val PACKAGE_NAME = "com.android.shell"

    init {
      if (android.os.Process.myUid() == 0) {
        try {
          android.system.Os.setgid(2000)
          android.system.Os.setuid(2000)
        } catch (_: Exception) {
        }
      }
    }

    @Volatile
    private var cachedClipboardService: Any? = null

    private fun getClipboardService(forceRefresh: Boolean = false): Any? {
      if (!forceRefresh) {
        cachedClipboardService?.let { return it }
      }
      return try {
        val serviceManager = Class.forName("android.os.ServiceManager")
        val binder = serviceManager.getMethod("getService", String::class.java)
          .invoke(null, "clipboard") as? IBinder ?: return null
        val stub = Class.forName("android.content.IClipboard\$Stub")
        stub.getMethod("asInterface", IBinder::class.java).invoke(null, binder).also {
          cachedClipboardService = it
        }
      } catch (error: Exception) {
        Log.e(TAG, "Failed to obtain clipboard service", error)
        cachedClipboardService = null
        null
      }
    }

    private fun fallbackApplication(): Application? {
      return try {
        val activityThread = Class.forName("android.app.ActivityThread")
        activityThread.getMethod("currentApplication").invoke(null) as? Application
      } catch (_: Exception) {
        try {
          val appGlobals = Class.forName("android.app.AppGlobals")
          appGlobals.getMethod("getInitialApplication").invoke(null) as? Application
        } catch (error: Exception) {
          Log.e(TAG, "Failed to obtain application context", error)
          null
        }
      }
    }
  }

  @Volatile
  private var lastClip: ClipData? = null

  private fun contentResolver(): ContentResolver? =
    serviceContext?.contentResolver ?: fallbackApplication()?.contentResolver

  override fun init(callerToken: IBinder) {
    try {
      callerToken.linkToDeath({ destroy() }, 0)
    } catch (_: Exception) {
    }
  }

  override fun getPrimaryClipStamp(): String {
    val desc = invokeClipboard("getPrimaryClipDescription") as? ClipDescription ?: return ""
    val timestamp = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) desc.timestamp else 0L
    if (timestamp <= 0L) return ""
    val mime = if (desc.mimeTypeCount > 0) desc.getMimeType(0) else ""
    val label = desc.label?.toString().orEmpty()
    return "$timestamp|${desc.mimeTypeCount}|$mime|$label"
  }

  override fun getPrimaryClipJson(): String {
    val clip = invokeClipboard("getPrimaryClip") as? ClipData ?: return ""
    if (clip.itemCount == 0) return ""
    lastClip = clip

    val description = clip.description ?: return ""
    val item = clip.getItemAt(0)
    val mimeType = if (description.mimeTypeCount > 0) {
      description.getMimeType(0) ?: "application/octet-stream"
    } else {
      "application/octet-stream"
    }
    val uri = item.uri
    val type = when {
      mimeType.startsWith("text/") && item.text != null -> "text"
      mimeType.startsWith("image/") && uri != null -> "image"
      uri != null -> "files"
      item.text != null -> "text"
      else -> return ""
    }
    val content = if (type == "text") item.text?.toString().orEmpty() else uri.toString()

    return JSONObject().apply {
      put("type", type)
      put("content", content)
      put("html", item.htmlText ?: "")
      put("mimeType", mimeType)
      if (uri != null) put("displayName", queryDisplayName(uri))
    }.toString()
  }

  override fun copyPrimaryClipToFile(destination: ParcelFileDescriptor): Boolean {
    val clip = lastClip ?: (invokeClipboard("getPrimaryClip") as? ClipData) ?: return false
    if (clip.itemCount == 0) return false
    val uri = clip.getItemAt(0).uri ?: return false
    val resolver = contentResolver() ?: return false

    return try {
      resolver.openInputStream(uri)?.use { input ->
        FileOutputStream(destination.fileDescriptor).use { output -> input.copyTo(output) }
      } ?: return false
      true
    } catch (_: Exception) {
      false
    }
  }

  override fun setPrimaryClipText(text: String): Boolean {
    return invokeClipboard("setPrimaryClip", ClipData.newPlainText("UniClipboard", text)) != null
  }

  override fun resolveBackgroundClipboardRestriction(): Boolean {
    val resolver = contentResolver() ?: return false
    return try {
      Settings.Secure.putInt(resolver, "mi_lab_ai_clipboard_enable", 0) &&
        Settings.Secure.getInt(resolver, "mi_lab_ai_clipboard_enable", 1) == 0
    } catch (error: Exception) {
      Log.e(TAG, "Failed to disable MIUI smart clipboard restriction", error)
      false
    }
  }

  override fun destroy() {
    lastClip = null
    cachedClipboardService = null
    System.exit(0)
  }

  private fun queryDisplayName(uri: Uri): String {
    val resolver = contentResolver() ?: return ""
    return try {
      resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
        if (cursor.moveToFirst()) cursor.getString(0).orEmpty() else ""
      }.orEmpty()
    } catch (_: Exception) {
      ""
    }
  }

  private fun invokeClipboard(methodName: String, clipData: ClipData? = null): Any? {
    repeat(2) { attempt ->
      val clipboard = getClipboardService(forceRefresh = attempt > 0) ?: return null
      val methods = clipboard.javaClass.methods
        .filter { it.name == methodName }
        .sortedByDescending { it.parameterCount }

      var sawDeadBinder = false
      for (method in methods) {
        val args = arrayOfNulls<Any>(method.parameterCount)
        var stringIndex = 0
        var supported = true
        method.parameterTypes.forEachIndexed { index, type ->
          args[index] = when {
            ClipData::class.java.isAssignableFrom(type) -> clipData
            type == String::class.java -> if (stringIndex++ == 0) PACKAGE_NAME else null
            type == Int::class.javaPrimitiveType || type == Int::class.java -> 0
            type == Long::class.javaPrimitiveType || type == Long::class.java -> 0L
            type == Boolean::class.javaPrimitiveType || type == Boolean::class.java -> false
            else -> {
              supported = false
              null
            }
          }
        }
        if (!supported || (methodName == "setPrimaryClip" && clipData == null)) continue

        try {
          val result = method.invoke(clipboard, *args)
          return result ?: if (method.returnType == Void.TYPE) true else null
        } catch (error: InvocationTargetException) {
          val cause = error.cause ?: error
          if (cause is DeadObjectException) {
            sawDeadBinder = true
            cachedClipboardService = null
            break
          }
          Log.e(
            TAG,
            "Failed ${method.name}(${method.parameterTypes.joinToString { it.simpleName }})",
            cause,
          )
        } catch (error: Exception) {
          Log.e(
            TAG,
            "Failed ${method.name}(${method.parameterTypes.joinToString { it.simpleName }})",
            error,
          )
        }
      }
      if (!sawDeadBinder) return null
    }
    return null
  }
}
