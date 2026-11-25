package app.uniclipboard.engine

import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Binder
import android.os.Build
import android.os.DeadObjectException
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.provider.Settings
import android.util.Log
import org.json.JSONObject
import rikka.shizuku.Shizuku
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Manages the `shizuku` background clipboard mode off the main UI thread.
 *
 * Hardening for long-running reliability:
 * - All Binder IPC (`bindUserService`, `getPrimaryClipStamp`, `getPrimaryClipJson`,
 *   `copyPrimaryClipToFile`, `setPrimaryClipText`) runs on a dedicated `HandlerThread`, never
 *   blocking the Android UI thread (prevents ANRs if Shizuku restarts or stalls).
 * - Binding watchdog timeout (10s) prevents `binding = true` from wedging permanently if a bind
 *   callback is dropped; automatic exponential-backoff reconnect on disconnect / `DeadObjectException`.
 * - Screen-aware monitoring pauses polling while screen is off/locked and checks immediately on
 *   screen-on / unlock.
 * - Uses lightweight `getPrimaryClipStamp()` first to avoid copying large clip text on idle ticks.
 */
internal class ShizukuClipboardController(
  private val appContext: Context,
  private val onBackgroundClipCaptured: (AndroidClipSnapshot) -> Unit,
  private val onStateChanged: (String) -> Unit,
) {
  companion object {
    private const val TAG = "UnicShizukuClip"
    private const val REQUEST_CODE_PERMISSION = 10086
    private const val POLL_INTERVAL_MS = 500L
    private const val BIND_STALE_TIMEOUT_MS = 10_000L
    private const val CONNECT_WAIT_SECONDS = 3L
    private const val INITIAL_RECONNECT_BACKOFF_MS = 1000L
    private const val MAX_RECONNECT_BACKOFF_MS = 30_000L
    private const val SHIZUKU_PACKAGE = "moe.shizuku.privileged.api"
    private const val SHIZUKU_LAUNCHER_ACTIVITY = "moe.shizuku.manager.MainActivity"
    private const val SHIZUKU_SETUP_URL = "https://shizuku.rikka.app/guide/setup/"
  }

  private val workerThread = HandlerThread("unic-shizuku-worker").apply {
    isDaemon = true
    start()
  }
  private val workerHandler = Handler(workerThread.looper)

  private val callerToken = Binder()
  private val stateLock = Any()

  @Volatile private var clipboardService: IClipboardUserService? = null
  @Volatile private var connectionLatch = CountDownLatch(1)
  @Volatile private var binding = false
  @Volatile private var bindingStartedAtMs = 0L
  @Volatile private var reconnectBackoffMs = INITIAL_RECONNECT_BACKOFF_MS

  @Volatile private var monitoringDesired = false
  @Volatile private var screenInteractive = true
  @Volatile private var appInForeground = true
  @Volatile private var listenersRegistered = false

  private var lastStamp = ""
  private var lastSnapshotJson = ""

  private val userServiceArgs: Shizuku.UserServiceArgs by lazy {
    Shizuku.UserServiceArgs(
      ComponentName(appContext.packageName, ClipboardUserService::class.java.name)
    )
      .daemon(false)
      .processNameSuffix("clipboard")
      .debuggable(BuildConfig.DEBUG)
      .version(3)
  }

  private val serviceConnection = object : ServiceConnection {
    override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
      val iface = binder?.let(IClipboardUserService.Stub::asInterface)
      synchronized(stateLock) {
        binding = false
        bindingStartedAtMs = 0L
        clipboardService = iface
        reconnectBackoffMs = INITIAL_RECONNECT_BACKOFF_MS
        connectionLatch.countDown()
      }
      workerHandler.post {
        try {
          iface?.init(callerToken)
        } catch (_: Exception) {
        }
        if (monitoringDesired) {
          seedBaselineFromService(iface)
          reconcilePollLoop(immediate = false)
        }
      }
      onStateChanged("connected")
    }

    override fun onServiceDisconnected(name: ComponentName?) {
      synchronized(stateLock) {
        binding = false
        bindingStartedAtMs = 0L
        clipboardService = null
      }
      onStateChanged("disconnected")
      if (monitoringDesired) {
        scheduleReconnect()
      }
    }
  }

  private val permissionListener = Shizuku.OnRequestPermissionResultListener { requestCode, result ->
    if (requestCode == REQUEST_CODE_PERMISSION) {
      val granted = result == PackageManager.PERMISSION_GRANTED
      if (granted) {
        workerHandler.post { bindUserServiceInternal() }
      }
      onStateChanged(if (granted) "authorized" else "denied")
    }
  }

  private val binderReceivedListener = Shizuku.OnBinderReceivedListener {
    synchronized(stateLock) {
      reconnectBackoffMs = INITIAL_RECONNECT_BACKOFF_MS
    }
    if (hasPermission()) {
      workerHandler.post { bindUserServiceInternal() }
    }
    onStateChanged("available")
  }

  private val binderDeadListener = Shizuku.OnBinderDeadListener {
    synchronized(stateLock) {
      clipboardService = null
      binding = false
      bindingStartedAtMs = 0L
    }
    onStateChanged("unavailable")
  }

  private val reconnectRunnable = Runnable {
    if (!monitoringDesired) return@Runnable
    if (clipboardService == null && hasPermission()) {
      bindUserServiceInternal()
    }
  }

  private val pollRunnable = object : Runnable {
    override fun run() {
      if (!shouldRunPollTick()) return
      val service = clipboardService
      if (service == null) {
        if (hasPermission()) {
          bindUserServiceInternal()
        }
        scheduleNextPoll()
        return
      }

      try {
        val stamp = service.primaryClipStamp.orEmpty()
        val shouldFetchFull = stamp.isEmpty() || stamp != lastStamp
        if (shouldFetchFull) {
          val json = service.primaryClipJson.orEmpty()
          if (stamp.isNotEmpty()) lastStamp = stamp
          if (json.isNotEmpty() && json != lastSnapshotJson) {
            lastSnapshotJson = json
            dispatchSnapshot(service, json)
          }
        }
      } catch (_: DeadObjectException) {
        Log.w(TAG, "Shizuku user service binder died; rebinding")
        synchronized(stateLock) {
          clipboardService = null
          binding = false
          bindingStartedAtMs = 0L
        }
        scheduleReconnect()
      } catch (error: Exception) {
        Log.w(TAG, "Shizuku poll error: ${error.javaClass.simpleName}")
      }

      scheduleNextPoll()
    }
  }

  @Synchronized
  fun registerListeners() {
    if (listenersRegistered) return
    listenersRegistered = true
    try {
      Shizuku.addRequestPermissionResultListener(permissionListener)
      Shizuku.addBinderReceivedListenerSticky(binderReceivedListener)
      Shizuku.addBinderDeadListener(binderDeadListener)
    } catch (error: Exception) {
      Log.w(TAG, "Failed to register Shizuku listeners: ${error.javaClass.simpleName}")
    }
  }

  fun isAvailable(): Boolean = try {
    Shizuku.pingBinder()
  } catch (_: Exception) {
    false
  }

  fun hasPermission(): Boolean = try {
    isAvailable() && Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED
  } catch (_: Exception) {
    false
  }

  fun isBackgroundClipboardRestricted(): Boolean {
    return try {
      val developmentMiui =
        Build.MANUFACTURER.equals("Xiaomi", ignoreCase = true) &&
          Build.VERSION.INCREMENTAL.endsWith(".DEV", ignoreCase = true)
      if (!developmentMiui) return false
      Settings.Secure.getInt(
        appContext.contentResolver,
        "mi_lab_ai_clipboard_enable",
        1,
      ) == 1
    } catch (_: Exception) {
      false
    }
  }

  fun requestPermission(): Boolean {
    return try {
      if (!isAvailable() || Shizuku.isPreV11()) return false
      Shizuku.requestPermission(REQUEST_CODE_PERMISSION)
      true
    } catch (_: Exception) {
      false
    }
  }

  fun openShizukuAppOrGuide(): Boolean {
    val appIntent = Intent(Intent.ACTION_MAIN).apply {
      addCategory(Intent.CATEGORY_LAUNCHER)
      setClassName(SHIZUKU_PACKAGE, SHIZUKU_LAUNCHER_ACTIVITY)
      addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    try {
      appContext.startActivity(appIntent)
      return true
    } catch (_: Exception) {
    }
    val webIntent = Intent(Intent.ACTION_VIEW, Uri.parse(SHIZUKU_SETUP_URL)).apply {
      addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    return try {
      appContext.startActivity(webIntent)
      true
    } catch (_: Exception) {
      false
    }
  }

  fun setForegroundState(inForeground: Boolean) {
    appInForeground = inForeground
    workerHandler.post { reconcilePollLoop(immediate = false) }
  }

  fun setScreenInteractive(interactive: Boolean) {
    val becameInteractive = !screenInteractive && interactive
    screenInteractive = interactive
    workerHandler.post { reconcilePollLoop(immediate = becameInteractive) }
  }

  fun startMonitoring() {
    registerListeners()
    monitoringDesired = true
    workerHandler.post {
      val service = ensureConnectedInternal()
      if (service != null) {
        seedBaselineFromService(service)
      }
      reconcilePollLoop(immediate = false)
    }
  }

  fun stopMonitoring() {
    monitoringDesired = false
    workerHandler.post {
      workerHandler.removeCallbacks(pollRunnable)
      workerHandler.removeCallbacks(reconnectRunnable)
      lastStamp = ""
      lastSnapshotJson = ""
    }
  }

  fun writeTextSync(text: String): Boolean {
    if (!hasPermission()) return false
    val latch = CountDownLatch(1)
    var ok = false
    workerHandler.post {
      try {
        val service = ensureConnectedInternal()
        ok = service?.setPrimaryClipText(text) == true
        if (ok) {
          // Refresh baseline so our own write isn't re-emitted on the next poll tick.
          seedBaselineFromService(service)
        }
      } catch (_: DeadObjectException) {
        synchronized(stateLock) { clipboardService = null }
      } catch (_: Exception) {
      } finally {
        latch.countDown()
      }
    }
    try {
      latch.await(CONNECT_WAIT_SECONDS + 1, TimeUnit.SECONDS)
    } catch (_: InterruptedException) {
    }
    return ok
  }

  fun resolveBackgroundClipboardRestrictionSync(): Boolean {
    if (!hasPermission()) return false
    val latch = CountDownLatch(1)
    var ok = false
    workerHandler.post {
      try {
        ok = ensureConnectedInternal()?.resolveBackgroundClipboardRestriction() == true
      } catch (_: Exception) {
      } finally {
        latch.countDown()
      }
    }
    try {
      latch.await(CONNECT_WAIT_SECONDS + 1, TimeUnit.SECONDS)
    } catch (_: InterruptedException) {
    }
    return ok
  }

  fun shutdown() {
    stopMonitoring()
    workerHandler.post { unbindUserServiceInternal() }
    if (listenersRegistered) {
      listenersRegistered = false
      try {
        Shizuku.removeRequestPermissionResultListener(permissionListener)
        Shizuku.removeBinderReceivedListener(binderReceivedListener)
        Shizuku.removeBinderDeadListener(binderDeadListener)
      } catch (_: Exception) {
      }
    }
  }

  private fun shouldRunPollTick(): Boolean =
    monitoringDesired && screenInteractive && !appInForeground

  private fun reconcilePollLoop(immediate: Boolean) {
    workerHandler.removeCallbacks(pollRunnable)
    if (!shouldRunPollTick()) return
    workerHandler.postDelayed(pollRunnable, if (immediate) 150L else POLL_INTERVAL_MS)
  }

  private fun scheduleNextPoll() {
    if (shouldRunPollTick()) {
      workerHandler.postDelayed(pollRunnable, POLL_INTERVAL_MS)
    }
  }

  private fun scheduleReconnect() {
    val delay = synchronized(stateLock) {
      val current = reconnectBackoffMs
      reconnectBackoffMs = (reconnectBackoffMs * 2).coerceAtMost(MAX_RECONNECT_BACKOFF_MS)
      current
    }
    workerHandler.removeCallbacks(reconnectRunnable)
    workerHandler.postDelayed(reconnectRunnable, delay)
  }

  private fun bindUserServiceInternal() {
    val shouldBind = synchronized(stateLock) {
      if (clipboardService != null || !hasPermission()) return
      val now = SystemClock.elapsedRealtime()
      if (binding) {
        if (now - bindingStartedAtMs < BIND_STALE_TIMEOUT_MS) {
          return
        }
        Log.w(TAG, "Shizuku bindUserService timed out after ${now - bindingStartedAtMs}ms; resetting")
        binding = false
        try {
          Shizuku.unbindUserService(userServiceArgs, serviceConnection, true)
        } catch (_: Exception) {
        }
      }
      binding = true
      bindingStartedAtMs = now
      connectionLatch = CountDownLatch(1)
      true
    }
    if (!shouldBind) return
    try {
      Shizuku.bindUserService(userServiceArgs, serviceConnection)
    } catch (error: Exception) {
      Log.w(TAG, "Shizuku bindUserService failed: ${error.javaClass.simpleName}")
      synchronized(stateLock) {
        binding = false
        bindingStartedAtMs = 0L
        connectionLatch.countDown()
      }
      if (monitoringDesired) scheduleReconnect()
    }
  }

  private fun ensureConnectedInternal(): IClipboardUserService? {
    clipboardService?.let { return it }
    if (!hasPermission()) return null
    bindUserServiceInternal()
    val latch = synchronized(stateLock) { connectionLatch }
    try {
      latch.await(CONNECT_WAIT_SECONDS, TimeUnit.SECONDS)
    } catch (_: InterruptedException) {
    }
    return clipboardService
  }

  private fun unbindUserServiceInternal() {
    val active = synchronized(stateLock) {
      val wasBound = clipboardService != null || binding
      clipboardService = null
      binding = false
      bindingStartedAtMs = 0L
      wasBound
    }
    if (!active) return
    try {
      Shizuku.unbindUserService(userServiceArgs, serviceConnection, true)
    } catch (_: Exception) {
    }
  }

  private fun seedBaselineFromService(service: IClipboardUserService?) {
    if (service == null) return
    try {
      lastStamp = service.primaryClipStamp.orEmpty()
      lastSnapshotJson = service.primaryClipJson.orEmpty()
    } catch (_: Exception) {
    }
  }

  private fun dispatchSnapshot(service: IClipboardUserService, json: String) {
    val obj = try {
      JSONObject(json)
    } catch (_: Exception) {
      return
    }
    val type = obj.optString("type", "")
    when (type) {
      "text" -> {
        val text = obj.optString("content", "")
        val html = obj.optString("html", "")
        if (text.isNotEmpty() && !text.trim().startsWith("unic-pair:")) {
          onBackgroundClipCaptured(AndroidClipSnapshot.Text(text, html))
        }
      }
      "image" -> {
        val mime = obj.optString("mimeType", "image/png").lowercase()
        val ext = when {
          mime.contains("jpeg") || mime.contains("jpg") -> "jpg"
          mime.contains("gif") -> "gif"
          mime.contains("webp") -> "webp"
          else -> "png"
        }
        val dir = File(appContext.cacheDir, "unic-shizuku-clip").apply { mkdirs() }
        val dest = File(dir, "clip_${UUID.randomUUID()}.$ext")
        val copied = try {
          ParcelFileDescriptor.open(
            dest,
            ParcelFileDescriptor.MODE_CREATE or ParcelFileDescriptor.MODE_TRUNCATE or
              ParcelFileDescriptor.MODE_READ_WRITE,
          ).use { pfd ->
            service.copyPrimaryClipToFile(pfd)
          }
        } catch (_: Exception) {
          false
        }
        if (copied && dest.length() > 0L) {
          onBackgroundClipCaptured(AndroidClipSnapshot.StagedImageFile(dest, mime))
        } else {
          runCatching { dest.delete() }
        }
      }
    }
  }
}
