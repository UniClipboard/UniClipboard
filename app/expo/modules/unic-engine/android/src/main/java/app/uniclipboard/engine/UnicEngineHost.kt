package app.uniclipboard.engine

import android.Manifest
import android.app.KeyguardManager
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.provider.OpenableColumns
import android.provider.Settings
import android.util.Log
import androidx.core.content.FileProvider
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.launch
import mobile.AndroidNetworkInterfaces
import mobile.Engine
import mobile.EventSink
import mobile.FileOpener
import mobile.FileStream
import mobile.Mobile
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.net.NetworkInterface
import java.net.SocketException
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors

internal interface UnicEngineUiListener {
  fun onEngineEvent(eventJSON: String)
  fun onEngineResync(reason: String)
  fun onBackgroundSyncStateChanged(reason: String)
}

/**
 * Process-scoped owner of the in-process Go Engine, network callbacks, system clipboard observer,
 * and all three Android background clipboard access modes (`overlay-polling`, `overlay-event`,
 * `shizuku`).
 *
 * Ownership model:
 * - Retained by the UI ([UnicEngineModule]) while mounted AND by [BackgroundSyncService] while
 *   running.
 * - Destroying the Activity or JS bridge detaches the UI listener without stopping
 *   [BackgroundSyncService] or shutting down the Go Engine.
 * - System `START_STICKY` or `BOOT_COMPLETED` recovery can bootstrap the Go Engine natively via
 *   [ensureHeadlessEngineStarted] without requiring JS to be alive.
 */
internal object UnicEngineHost {
  private const val TAG = "UnicEngineHost"

  val engineExecutor = Executors.newSingleThreadExecutor { runnable ->
    Thread(runnable, "unic-engine-worker").apply { isDaemon = true }
  }
  val engineDispatcher = engineExecutor.asCoroutineDispatcher()
  val engineScope = CoroutineScope(SupervisorJob() + engineDispatcher)
  private val mainHandler = Handler(Looper.getMainLooper())

  private val androidNetworkInterfaces = AndroidNetworkInterfaces {
    val snapshot = JSONArray()
    val interfaces = try {
      NetworkInterface.getNetworkInterfaces()?.toList().orEmpty()
    } catch (_: SocketException) {
      emptyList()
    }
    for (iface in interfaces) {
      try {
        val addresses = iface.interfaceAddresses.mapNotNull { address ->
          val host = address.address.hostAddress?.substringBefore('%') ?: return@mapNotNull null
          "$host/${address.networkPrefixLength}"
        }
        snapshot.put(
          JSONObject()
            .put("name", iface.name)
            .put("index", iface.index)
            .put("mtu", iface.mtu)
            .put("up", iface.isUp)
            .put("loopback", iface.isLoopback)
            .put("addresses", JSONArray(addresses)),
        )
      } catch (_: SocketException) {
      }
    }
    snapshot.toString()
  }

  @Volatile private var appContext: Context? = null
  @Volatile private var uiListener: UnicEngineUiListener? = null
  @Volatile private var uiAttached: Boolean = false
  @Volatile private var serviceRetained: Boolean = false
  @Volatile var isForeground: Boolean = false
    private set
  @Volatile internal var isEngineSuspended: Boolean = false
    private set
  @Volatile private var hasActivityWindowFocus: Boolean = false

  @Volatile var engine: Engine? = null
    private set

  @Volatile private var overlayController: OverlayClipboardController? = null
  @Volatile private var shizukuController: ShizukuClipboardController? = null
  @Volatile private var resourceCollector: AndroidResourcePowerCollector? = null

  private var networkCallback: ConnectivityManager.NetworkCallback? = null
  private var connectivityManager: ConnectivityManager? = null
  private var clipboardManager: ClipboardManager? = null
  private var clipListener: ClipboardManager.OnPrimaryClipChangedListener? = null
  private var screenReceiver: BroadcastReceiver? = null

  private var observationEpoch: String = ""
  private var observationSequence: Long = 0L
  private var lastObservedFingerprint: String? = null
  private var lastAppliedFingerprint: String? = null
  private var resolvedDataDir: String? = null
  private var resolvedCacheDir: String? = null
  private var bgDrainJob: kotlinx.coroutines.Job? = null

  @Synchronized
  private fun ensureInitialized(context: Context): Context {
    val app = context.applicationContext
    if (appContext == null) {
      appContext = app
    }
    if (resourceCollector == null) {
      val collector = AndroidResourcePowerCollector(app)
      resourceCollector = collector
      Mobile.registerHostResourceProvider(collector)
    }
    if (overlayController == null) {
      overlayController = OverlayClipboardController(
        appContext = app,
        onBackgroundClipCaptured = { snapshot ->
          handleBackgroundClipCaptured(app, snapshot)
        },
        onMonitorStateChanged = {
          onBackgroundMonitorStateChanged(app)
        },
      )
    }
    if (shizukuController == null) {
      shizukuController = ShizukuClipboardController(
        appContext = app,
        onBackgroundClipCaptured = { snapshot ->
          handleBackgroundClipCaptured(app, snapshot)
        },
        onStateChanged = { state ->
          resourceCollector?.recordStateSnapshot()
          engineScope.launch {
            reconcileBackgroundMonitors(app)
            refreshHostCapabilities(app)
          }
          notifyBackgroundSyncStateChanged(app, "shizuku:$state")
        },
      ).also { it.registerListeners() }
    }
    return app
  }

  fun attachUi(context: Context, listener: UnicEngineUiListener) {
    val app = ensureInitialized(context)
    uiAttached = true
    uiListener = listener
    // Re-register Shizuku listeners if previously unbound.
    shizukuController?.registerListeners()
    engineScope.launch {
      reconcileBackgroundMonitors(app)
    }
  }

  fun detachUi(listener: UnicEngineUiListener) {
    if (uiListener === listener) {
      uiListener = null
    }
    uiAttached = false
    hasActivityWindowFocus = false
    isForeground = false
    val app = appContext ?: return
    overlayController?.setForegroundState(false)
    shizukuController?.setForegroundState(false)
    resourceCollector?.recordStateSnapshot()
    engineScope.launch {
      if (!serviceRetained && !BackgroundSyncService.running && !shouldKeepAliveWithoutService(app)) {
        shutdownEngineInternal()
      } else {
        reconcileRuntimeState(app)
        reconcileBackgroundMonitors(app)
        refreshHostCapabilities(app)
      }
    }
  }

  fun retainForService(context: Context) {
    val app = ensureInitialized(context)
    serviceRetained = true
    engineScope.launch {
      reconcileRuntimeState(app)
      reconcileBackgroundMonitors(app)
      refreshHostCapabilities(app)
    }
  }

  fun releaseForService(context: Context) {
    val app = ensureInitialized(context)
    serviceRetained = false
    engineScope.launch {
      if (!uiAttached && !shouldKeepAliveWithoutService(app)) {
        shutdownEngineInternal()
      } else {
        reconcileRuntimeState(app)
        reconcileBackgroundMonitors(app)
        refreshHostCapabilities(app)
      }
    }
  }

  fun onServiceRunningChanged(context: Context, running: Boolean) {
    resourceCollector?.recordStateSnapshot()
    val rawApp = appContext ?: context.applicationContext
    if (appContext == null) {
      appContext = rawApp
    }
    engineScope.launch {
      val app = ensureInitialized(rawApp)
      resourceCollector?.onStateTransition(engine)
      reconcileRuntimeState(app)
      reconcileBackgroundMonitors(app)
      refreshHostCapabilities(app)
    }
    notifyBackgroundSyncStateChanged(rawApp, if (running) "service_started" else "service_stopped")
  }

  fun notifyBackgroundSyncStateChanged(context: Context, reason: String) {
    if (appContext == null) {
      appContext = context.applicationContext
    }
    uiListener?.onBackgroundSyncStateChanged(reason)
  }

  fun onActivityEntersForeground(context: Context, hasFocusProvider: () -> Boolean) {
    isForeground = true
    overlayController?.setForegroundState(true)
    shizukuController?.setForegroundState(true)
    resourceCollector?.recordStateSnapshot()
    val rawApp = appContext ?: context.applicationContext
    if (appContext == null) {
      appContext = rawApp
    }

    if (BackgroundSyncPrefs.isEffectivelyEnabled(rawApp) && BackgroundSyncPrefs.foregroundNotification(rawApp)) {
      runCatching { BackgroundSyncService.start(rawApp) }
    }

    engineScope.launch {
      val app = ensureInitialized(rawApp)
      val eng = engine ?: return@launch
      resourceCollector?.onStateTransition(eng)
      reconcileRuntimeState(app)
      reconcileBackgroundMonitors(app)
    }

    mainHandler.postDelayed({
      engineScope.launch {
        val app = ensureInitialized(rawApp)
        val eng = engine ?: return@launch
        if (!isForeground) return@launch
        syncForegroundClipboard(app, eng, isResume = true, hasFocusProvider = hasFocusProvider)
      }
    }, 150L)
  }

  fun onActivityEntersBackground(context: Context) {
    isForeground = false
    hasActivityWindowFocus = false
    overlayController?.setForegroundState(false)
    shizukuController?.setForegroundState(false)
    resourceCollector?.recordStateSnapshot()
    val rawApp = appContext ?: context.applicationContext
    if (appContext == null) {
      appContext = rawApp
    }
    engineScope.launch {
      val app = ensureInitialized(rawApp)
      val eng = engine ?: return@launch
      resourceCollector?.onStateTransition(eng)
      if (!canObserveInBackground(app)) {
        observationEpoch = ""
        observationSequence = 0L
      }
      reconcileRuntimeState(app)
      reconcileBackgroundMonitors(app)
      refreshHostCapabilities(app)
    }
  }

  /**
   * Initializes or attaches to the singleton Go Engine. Must be called on [engineScope].
   */
  fun initializeEngine(
    context: Context,
    dataDir: String,
    cacheDir: String,
    deviceName: String?,
    hasFocusProvider: () -> Boolean,
  ): String {
    val app = ensureInitialized(context)
    resolvedDataDir = dataDir
    resolvedCacheDir = cacheDir
    val resolvedName = deviceName?.trim()?.takeIf { it.isNotEmpty() }
      ?: BackgroundSyncPrefs.deviceName(app)
      ?: (Build.MODEL ?: "Android")
    BackgroundSyncPrefs.setDeviceName(app, resolvedName)

    engine?.let { existing ->
      reconcileRuntimeState(app)
      reconcileBackgroundMonitors(app)
      refreshHostCapabilities(app)
      if (isForeground) {
        mainHandler.postDelayed({
          engineScope.launch {
            val eng = engine ?: return@launch
            if (isForeground) {
              syncForegroundClipboard(app, eng, isResume = false, hasFocusProvider = hasFocusProvider)
            }
          }
        }, 150L)
      }
      return existing.state
    }

    val snapshot = startNewEngineInternal(app, dataDir, cacheDir, resolvedName)
    if (BackgroundSyncPrefs.isEffectivelyEnabled(app) && BackgroundSyncPrefs.foregroundNotification(app)) {
      runCatching { BackgroundSyncService.start(app) }
    }
    if (isForeground) {
      mainHandler.postDelayed({
        engineScope.launch {
          val eng = engine ?: return@launch
          if (isForeground) {
            syncForegroundClipboard(app, eng, isResume = false, hasFocusProvider = hasFocusProvider)
          }
        }
      }, 150L)
    }
    return engine?.state?.ifEmpty { snapshot } ?: snapshot
  }

  /**
   * Bootstraps the Go Engine natively without any UI/JS runtime (for `START_STICKY` or
   * `BOOT_COMPLETED` recovery) when first-install setup is already complete.
   */
  fun ensureHeadlessEngineStarted(context: Context) {
    val app = ensureInitialized(context)
    engineScope.launch {
      if (engine != null) {
        reconcileRuntimeState(app)
        reconcileBackgroundMonitors(app)
        refreshHostCapabilities(app)
        return@launch
      }
      val dataDir = resolvedDataDir ?: File(app.filesDir, "unic-v2").apply { mkdirs() }.canonicalPath
      val cacheDir = resolvedCacheDir ?: File(app.cacheDir, "unic-cache").apply { mkdirs() }.canonicalPath
      val defaultV1 = File(app.filesDir, "UniClipboard").let {
        try { it.canonicalPath } catch (_: Exception) { it.absolutePath }
      }
      val setupComplete = try {
        val setup = Mobile.openSetupFlow(dataDir, defaultV1)
        JSONObject(setup.state).optBoolean("complete", false)
      } catch (error: Exception) {
        Log.w(TAG, "Headless setup check failed: ${error.javaClass.simpleName}")
        false
      }
      if (!setupComplete) {
        Log.i(TAG, "Setup incomplete; skipping headless engine startup")
        return@launch
      }

      val name = BackgroundSyncPrefs.deviceName(app) ?: (Build.MODEL ?: "Android")
      try {
        startNewEngineInternal(app, dataDir, cacheDir, name)
        Log.i(TAG, "Headless Go Engine started successfully")
      } catch (error: Exception) {
        Log.e(TAG, "Failed to start headless Go Engine", error)
      }
    }
  }

  /**
   * Explicitly shuts down Engine and stops background sync (used when entering later v1 migration).
   */
  fun shutdownForMigrationOrExplicitStop(context: Context) {
    val app = ensureInitialized(context)
    BackgroundSyncService.stop(app, ServiceStopRecord.POLICY)
    shutdownEngineInternal()
  }

  private fun startNewEngineInternal(
    app: Context,
    dataDir: String,
    cacheDir: String,
    deviceName: String,
  ): String {
    val cfg = JSONObject()
      .put("data_dir", dataDir)
      .put("cache_dir", cacheDir)
      .put("device_name", deviceName)
      .put(
        "host_capabilities",
        JSONObject()
          .put("read_formats", JSONArray(listOf("text/plain", "text/html", "image/png")))
          .put("write_formats", JSONArray(listOf("text/plain", "text/html", "image/png")))
          .put("read_files", true)
          .put("write_files", true)
          .put("observe", computeObserveCapability(app))
          .put("apply", computeApplyCapability(app)),
      )
    Mobile.registerAndroidNetworkInterfaces(androidNetworkInterfaces)
    val eng = Mobile.newEngine(cfg.toString())
    resourceCollector?.onEngineStarted()
    resourceCollector?.let { eng.setHostResourceProvider(it) }
    val sink = object : EventSink {
      override fun onEvent(eventJSON: String?) {
        if (eventJSON == null) return
        uiListener?.onEngineEvent(eventJSON)
        handleNativeEngineEvent(app, eventJSON)
      }

      override fun onResyncRequired(reason: String?) {
        uiListener?.onEngineResync(reason ?: "overflow")
      }
    }
    val snapshot = eng.subscribe(sink)
    engine = eng
    observationEpoch = ""
    observationSequence = 0L

    reconcileRuntimeState(app)
    eng.start(null)
    startNetworkMonitor(app)
    startClipboardListener(app)
    startScreenReceiver(app)
    updateScreenInteractiveState(app)
    reconcileBackgroundMonitors(app)
    refreshHostCapabilities(app)
    return snapshot
  }

  private fun shutdownEngineInternal() {
    overlayController?.stopAll()
    shizukuController?.stopMonitoring()
    stopScreenReceiver()
    stopClipboardListener()
    stopNetworkMonitor()
    observationEpoch = ""
    observationSequence = 0L
    isEngineSuspended = false
    engine?.let {
      try {
        it.flushResourceSample()
      } catch (_: Exception) {
      }
      try {
        it.close(null)
      } catch (_: Exception) {
      }
      engine = null
    }
    resourceCollector?.resetOnSuspension()
  }

  // ---------------------------------------------------------------------------
  // Background sync configuration & permissions
  // ---------------------------------------------------------------------------

  fun isIgnoringBatteryOptimizations(context: Context): Boolean {
    val pm = context.applicationContext.getSystemService(Context.POWER_SERVICE) as? PowerManager
      ?: return false
    return pm.isIgnoringBatteryOptimizations(context.packageName)
  }

  fun requestIgnoreBatteryOptimizations(context: Context): Boolean {
    val intent = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
      data = Uri.parse("package:${context.packageName}")
      addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    return try {
      context.applicationContext.startActivity(intent)
      true
    } catch (_: Exception) {
      false
    }
  }

  fun requestOverlayPermission(context: Context): Boolean {
    ensureInitialized(context)
    return overlayController?.requestOverlayPermission() == true
  }

  fun requestShizukuPermission(context: Context): Boolean {
    ensureInitialized(context)
    return shizukuController?.requestPermission() == true
  }

  fun openShizukuAppOrGuide(context: Context): Boolean {
    ensureInitialized(context)
    return shizukuController?.openShizukuAppOrGuide() == true
  }

  fun resolveBackgroundClipboardRestriction(context: Context): Boolean {
    ensureInitialized(context)
    val ok = shizukuController?.resolveBackgroundClipboardRestrictionSync() == true
    val app = context.applicationContext
    engineScope.launch {
      reconcileBackgroundMonitors(app)
      refreshHostCapabilities(app)
    }
    notifyBackgroundSyncStateChanged(app, "restriction_resolved")
    return ok
  }

  fun setAutomaticCapture(context: Context, enabled: Boolean) {
    val app = ensureInitialized(context)
    BackgroundSyncPrefs.setAutomaticCapture(app, enabled)
    reconcileBackgroundMonitors(app)
    refreshHostCapabilities(app)
  }

  fun setAutoApplyReceived(context: Context, enabled: Boolean) {
    val app = ensureInitialized(context)
    BackgroundSyncPrefs.setAutoApplyReceived(app, enabled)
  }

  fun getBackgroundSyncStatusJSON(context: Context): String {
    val app = ensureInitialized(context)
    val method = BackgroundSyncPrefs.clipboardAccessMethod(app)
    val auth = buildAuthorizationState(app, method)
    val notifGranted = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
      app.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
    } else {
      true
    }
    val json = JSONObject()
      .put("enabled", BackgroundSyncPrefs.enabled(app))
      .put("temp_disabled", BackgroundSyncPrefs.tempDisabled)
      .put("running", BackgroundSyncService.running)
      .put("foreground_notification", BackgroundSyncPrefs.foregroundNotification(app))
      .put("background_download", BackgroundSyncPrefs.backgroundDownload(app))
      .put("background_upload", BackgroundSyncPrefs.backgroundUpload(app))
      .put("network", BackgroundSyncPrefs.network(app).wireValue)
      .put("clipboard_access_method", method.wireValue)
      .put("ignoring_battery_optimizations", isIgnoringBatteryOptimizations(app))
      .put("notification_permission_granted", notifGranted)
      .put("application_id", app.packageName)
      .put("authorization", auth)
    ServiceStopRecord.lastReason(app)?.let { json.put("last_stop_reason", it) }
    ServiceStopRecord.lastAt(app)?.let { json.put("last_stop_at", it) }
    return json.toString()
  }

  fun updateBackgroundSyncConfig(context: Context, patchJSON: String): String {
    val app = ensureInitialized(context)
    val patch = JSONObject(patchJSON)

    if (patch.has("enabled")) {
      val enabled = patch.getBoolean("enabled")
      BackgroundSyncPrefs.setEnabled(app, enabled)
      if (!enabled) {
        BackgroundSyncPrefs.tempDisabled = false
      }
    }
    if (patch.has("temp_disabled")) {
      BackgroundSyncPrefs.tempDisabled = patch.getBoolean("temp_disabled")
    }
    if (patch.has("foreground_notification")) {
      BackgroundSyncPrefs.setForegroundNotification(app, patch.getBoolean("foreground_notification"))
    }
    if (patch.has("background_download")) {
      BackgroundSyncPrefs.setBackgroundDownload(app, patch.getBoolean("background_download"))
    }
    if (patch.has("background_upload")) {
      BackgroundSyncPrefs.setBackgroundUpload(app, patch.getBoolean("background_upload"))
    }
    if (patch.has("network")) {
      BackgroundSyncPrefs.setNetwork(app, BackgroundSyncNetwork.fromWire(patch.optString("network")))
    }
    if (patch.has("clipboard_access_method")) {
      BackgroundSyncPrefs.setClipboardAccessMethod(
        app,
        ClipboardAccessMethod.fromWire(patch.optString("clipboard_access_method")),
      )
    }

    val shouldRunService =
      BackgroundSyncPrefs.isEffectivelyEnabled(app) && BackgroundSyncPrefs.foregroundNotification(app)
    if (shouldRunService) {
      BackgroundSyncService.start(app)
    } else if (BackgroundSyncService.running) {
      val reason = when {
        !BackgroundSyncPrefs.enabled(app) -> ServiceStopRecord.USER_STOP
        BackgroundSyncPrefs.tempDisabled -> ServiceStopRecord.USER_PAUSE
        else -> ServiceStopRecord.POLICY
      }
      BackgroundSyncService.stop(app, reason)
    }

    reconcileRuntimeState(app)
    reconcileBackgroundMonitors(app)
    refreshHostCapabilities(app)
    notifyBackgroundSyncStateChanged(app, "config_updated")
    return getBackgroundSyncStatusJSON(app)
  }

  private fun buildAuthorizationState(app: Context, method: ClipboardAccessMethod): JSONObject {
    val obj = JSONObject()
    when (method) {
      ClipboardAccessMethod.OVERLAY_POLLING -> {
        val granted = overlayController?.hasOverlayPermission() == true
        obj.put("status", if (granted) "ready" else "unauthorized")
        obj.put("monitoring_status", "ready")
      }
      ClipboardAccessMethod.OVERLAY_EVENT -> {
        val overlayGranted = overlayController?.hasOverlayPermission() == true
        val logsGranted = overlayController?.hasReadLogsPermission() == true
        obj.put("status", if (overlayGranted) "ready" else "unauthorized")
        obj.put("monitoring_status", if (logsGranted) "ready" else "setup-required")
        if (!logsGranted) {
          obj.put("setup_command", "adb shell pm grant ${app.packageName} android.permission.READ_LOGS")
        }
      }
      ClipboardAccessMethod.SHIZUKU -> {
        val shizuku = shizukuController
        val restricted = shizuku?.isBackgroundClipboardRestricted() == true
        val available = shizuku?.isAvailable() == true
        val authorized = shizuku?.hasPermission() == true
        when {
          restricted -> {
            obj.put("status", "incompatible")
            obj.put("monitoring_status", "setup-required")
          }
          !available -> {
            obj.put("status", "unavailable")
            obj.put("monitoring_status", "setup-required")
            obj.put("setup_url", "https://shizuku.rikka.app/guide/setup/")
          }
          else -> {
            obj.put("status", if (authorized) "ready" else "unauthorized")
            obj.put("monitoring_status", if (authorized) "ready" else "setup-required")
          }
        }
      }
    }
    return obj
  }

  // ---------------------------------------------------------------------------
  // Runtime state, capabilities & background monitor reconciliation
  // ---------------------------------------------------------------------------

  private fun shouldKeepAliveWithoutService(app: Context): Boolean =
    BackgroundSyncPrefs.isEffectivelyEnabled(app) && !BackgroundSyncPrefs.foregroundNotification(app)

  private fun isBackgroundSyncPermittedNow(app: Context): Boolean {
    if (!BackgroundSyncPrefs.isEffectivelyEnabled(app)) return false
    val activeCarrier = BackgroundSyncService.running || !BackgroundSyncPrefs.foregroundNotification(app)
    if (!activeCarrier) return false
    if (BackgroundSyncPrefs.network(app) == BackgroundSyncNetwork.WIFI && !isOnWifiNetwork(app)) {
      return false
    }
    return true
  }

  private fun isOnWifiNetwork(app: Context): Boolean {
    val cm = app.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return true
    val active = cm.activeNetwork ?: return false
    val caps = cm.getNetworkCapabilities(active) ?: return false
    return caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
      caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)
  }

  private fun reconcileRuntimeState(app: Context) {
    val eng = engine ?: return
    val desired = if (isForeground || isBackgroundSyncPermittedNow(app)) "active" else "suspended"
    if (desired == "active") {
      bgDrainJob?.cancel()
      bgDrainJob = null
      isEngineSuspended = false
      try {
        eng.setRuntimeState("active")
      } catch (_: Exception) {
      }
      resourceCollector?.recordStateSnapshot()
      resourceCollector?.onStateTransition(eng)
      return
    }
    if (runCatching { eng.hasActiveSends() }.getOrDefault(false)) {
      if (bgDrainJob?.isActive == true) return
      bgDrainJob = engineScope.launch {
        val deadlineMs = System.currentTimeMillis() + 25_000L
        while (System.currentTimeMillis() < deadlineMs) {
          if (isForeground || isBackgroundSyncPermittedNow(app)) {
            return@launch
          }
          val active = runCatching { engine?.hasActiveSends() == true }.getOrDefault(false)
          if (!active) break
          kotlinx.coroutines.delay(350L)
        }
        if (!isForeground && !isBackgroundSyncPermittedNow(app)) {
          isEngineSuspended = true
          try {
            engine?.setRuntimeState("suspended")
          } catch (_: Exception) {
          }
          resourceCollector?.recordStateSnapshot()
          resourceCollector?.onStateTransition(engine)
        }
      }
      return
    }
    bgDrainJob?.cancel()
    bgDrainJob = null
    isEngineSuspended = true
    try {
      eng.setRuntimeState("suspended")
    } catch (_: Exception) {
    }
    resourceCollector?.recordStateSnapshot()
    resourceCollector?.onStateTransition(eng)
  }

  private fun isMethodReadyForRead(app: Context, method: ClipboardAccessMethod): Boolean {
    return when (method) {
      ClipboardAccessMethod.OVERLAY_POLLING -> overlayController?.hasOverlayPermission() == true
      ClipboardAccessMethod.OVERLAY_EVENT ->
        overlayController?.hasOverlayPermission() == true &&
          overlayController?.hasReadLogsPermission() == true
      ClipboardAccessMethod.SHIZUKU ->
        shizukuController?.isBackgroundClipboardRestricted() != true &&
          shizukuController?.hasPermission() == true
    }
  }

  private fun isMethodReadyForWrite(app: Context, method: ClipboardAccessMethod): Boolean {
    return when (method) {
      ClipboardAccessMethod.OVERLAY_POLLING,
      ClipboardAccessMethod.OVERLAY_EVENT -> overlayController?.hasOverlayPermission() == true
      ClipboardAccessMethod.SHIZUKU ->
        shizukuController?.isBackgroundClipboardRestricted() != true &&
          shizukuController?.hasPermission() == true
    }
  }

  private fun canObserveInBackground(app: Context): Boolean {
    if (!isBackgroundSyncPermittedNow(app)) return false
    if (!BackgroundSyncPrefs.backgroundUpload(app) || !BackgroundSyncPrefs.automaticCapture(app)) return false
    return isMethodReadyForRead(app, BackgroundSyncPrefs.clipboardAccessMethod(app))
  }

  private fun canApplyInBackground(app: Context): Boolean {
    if (!isBackgroundSyncPermittedNow(app)) return false
    if (!BackgroundSyncPrefs.backgroundDownload(app)) return false
    return isMethodReadyForWrite(app, BackgroundSyncPrefs.clipboardAccessMethod(app))
  }

  private fun computeObserveCapability(app: Context): String {
    if (isForeground) {
      return if (hasActivityWindowFocus) "available" else "unavailable"
    }
    if (!isBackgroundSyncPermittedNow(app) || !BackgroundSyncPrefs.backgroundUpload(app)) {
      return "unavailable"
    }
    return if (isMethodReadyForRead(app, BackgroundSyncPrefs.clipboardAccessMethod(app))) {
      "available"
    } else {
      "permission_required"
    }
  }

  private fun computeApplyCapability(app: Context): String {
    if (isForeground) return "available"
    if (!isBackgroundSyncPermittedNow(app) || !BackgroundSyncPrefs.backgroundDownload(app)) {
      return "unavailable"
    }
    return if (isMethodReadyForWrite(app, BackgroundSyncPrefs.clipboardAccessMethod(app))) {
      "available"
    } else {
      "permission_required"
    }
  }

  fun refreshHostCapabilities(context: Context) {
    val app = ensureInitialized(context)
    val eng = engine ?: return
    val caps = JSONObject()
      .put("read_formats", JSONArray(listOf("text/plain", "text/html", "image/png")))
      .put("write_formats", JSONArray(listOf("text/plain", "text/html", "image/png")))
      .put("read_files", true)
      .put("write_files", true)
      .put("observe", computeObserveCapability(app))
      .put("apply", computeApplyCapability(app))
    try {
      eng.updateHostCapabilities(caps.toString())
    } catch (_: Exception) {
    }
  }

  private fun reconcileBackgroundMonitors(app: Context) {
    val overlay = overlayController ?: return
    val shizuku = shizukuController ?: return
    val shouldMonitor =
      engine != null &&
        isBackgroundSyncPermittedNow(app) &&
        BackgroundSyncPrefs.backgroundUpload(app) &&
        BackgroundSyncPrefs.automaticCapture(app)

    if (!shouldMonitor) {
      overlay.stopAll()
      shizuku.stopMonitoring()
      resourceCollector?.recordStateSnapshot()
      resourceCollector?.onStateTransition(engine)
      return
    }

    when (BackgroundSyncPrefs.clipboardAccessMethod(app)) {
      ClipboardAccessMethod.OVERLAY_POLLING -> {
        shizuku.stopMonitoring()
        overlay.stopEventMonitor()
        overlay.startPolling()
      }
      ClipboardAccessMethod.OVERLAY_EVENT -> {
        shizuku.stopMonitoring()
        overlay.stopPolling()
        overlay.startEventMonitor()
      }
      ClipboardAccessMethod.SHIZUKU -> {
        overlay.stopAll()
        shizuku.startMonitoring()
      }
    }
    resourceCollector?.recordStateSnapshot()
    resourceCollector?.onStateTransition(engine)
  }

  internal fun onBackgroundMonitorStateChanged(context: Context) {
    resourceCollector?.recordStateSnapshot()
    val rawApp = appContext ?: context.applicationContext
    if (appContext == null) {
      appContext = rawApp
    }
    engineScope.launch {
      ensureInitialized(rawApp)
      resourceCollector?.onStateTransition(engine)
    }
  }

  internal fun activeBackgroundMode(): String? {
    if (isForeground || isEngineSuspended) return null
    if (overlayController?.isPollingActive() == true) return "overlay_polling"
    if (overlayController?.isEventMonitorActive() == true) return "overlay_event"
    if (shizukuController?.isMonitoringActive() == true) return "shizuku"
    if (BackgroundSyncService.running) return "fgs_idle"
    return null
  }

  // ---------------------------------------------------------------------------
  // System clipboard observation, staging & background auto-apply
  // ---------------------------------------------------------------------------

  fun markSelfAppliedText(text: String) {
    val fp = "text:${sha256Hex(text.toByteArray(Charsets.UTF_8))}"
    lastAppliedFingerprint = fp
    lastObservedFingerprint = fp
    overlayController?.suppressLogcatBriefly()
  }

  fun observeCurrentForegroundClipboard(context: Context, explicit: Boolean): String {
    val app = ensureInitialized(context)
    val eng = engine ?: throw IllegalStateException("engine is not initialized")
    val snap = readAndroidClipboardSnapshot(app)
    return observeClipSnapshot(app, eng, snap, explicit = explicit)
  }

  private fun handleBackgroundClipCaptured(app: Context, snapshot: AndroidClipSnapshot) {
    engineScope.launch {
      try {
        val eng = engine ?: return@launch
        if (isForeground) return@launch
        if (!isBackgroundSyncPermittedNow(app) ||
          !BackgroundSyncPrefs.backgroundUpload(app) ||
          !BackgroundSyncPrefs.automaticCapture(app)
        ) {
          return@launch
        }
        observeClipSnapshot(app, eng, snapshot, explicit = false)
      } catch (error: Exception) {
        Log.w(TAG, "Background clipboard observe failed: ${error.javaClass.simpleName}")
      } finally {
        if (snapshot is AndroidClipSnapshot.StagedImageFile) {
          runCatching { snapshot.file.delete() }
        }
      }
    }
  }

  private fun observeClipSnapshot(
    ctx: Context,
    eng: Engine,
    snap: AndroidClipSnapshot,
    explicit: Boolean,
  ): String {
    if (snap is AndroidClipSnapshot.Empty) {
      return "{\"skipped\":true,\"reason\":\"empty_or_unsupported\"}"
    }
    val staged = stageAndroidClipboardSnapshot(eng, ctx, snap)
      ?: return "{\"skipped\":true,\"reason\":\"empty_or_unsupported\"}"
    val (fp, leaseID) = staged
    try {
      if (fp == lastAppliedFingerprint) {
        return "{\"skipped\":true,\"reason\":\"self_applied\"}"
      }
      if (!explicit && fp == lastObservedFingerprint) {
        return "{\"skipped\":true,\"reason\":\"unchanged\"}"
      }
      ensureEmptyObservationBaseline(eng)
      val nextSeq = observationSequence + 1L
      val obsReq = JSONObject()
        .put("epoch", observationEpoch)
        .put("sequence", nextSeq.toString())
        .put("mode", "change")
        .put("lease_id", leaseID)
      val res = eng.observeClipboard(obsReq.toString())
      observationSequence = nextSeq
      lastObservedFingerprint = fp
      return res
    } finally {
      try { eng.releaseContent(leaseID) } catch (_: Exception) {}
    }
  }

  private fun syncForegroundClipboard(
    ctx: Context,
    eng: Engine,
    isResume: Boolean,
    hasFocusProvider: () -> Boolean,
  ) {
    val hasFocus = onMainSync { hasFocusProvider() }
    hasActivityWindowFocus = hasFocus
    refreshHostCapabilities(ctx)
    if (!hasFocus) return
    if (!BackgroundSyncPrefs.automaticCapture(ctx)) return

    val snap = readAndroidClipboardSnapshot(ctx)
    if (snap is AndroidClipSnapshot.Empty) {
      if (observationEpoch.isEmpty()) {
        try { ensureEmptyObservationBaseline(eng) } catch (_: Exception) {}
      }
      return
    }
    val staged = try {
      stageAndroidClipboardSnapshot(eng, ctx, snap)
    } catch (_: Exception) {
      null
    } ?: return
    val (fp, leaseID) = staged
    try {
      val changedWhileAway = isResume &&
        lastObservedFingerprint != null &&
        fp != lastObservedFingerprint &&
        fp != lastAppliedFingerprint
      if (changedWhileAway && BackgroundSyncPrefs.automaticCapture(ctx)) {
        ensureEmptyObservationBaseline(eng)
        val nextSeq = observationSequence + 1L
        val obsReq = JSONObject()
          .put("epoch", observationEpoch)
          .put("sequence", nextSeq.toString())
          .put("mode", "change")
          .put("lease_id", leaseID)
        eng.observeClipboard(obsReq.toString())
        observationSequence = nextSeq
      } else if (observationEpoch.isEmpty()) {
        val newEpoch = "android-${UUID.randomUUID().toString().take(12)}"
        val baseReq = JSONObject()
          .put("epoch", newEpoch)
          .put("sequence", "1")
          .put("mode", "baseline")
          .put("lease_id", leaseID)
        eng.observeClipboard(baseReq.toString())
        observationEpoch = newEpoch
        observationSequence = 1L
      }
      lastObservedFingerprint = fp
    } catch (_: Exception) {
    } finally {
      try { eng.releaseContent(leaseID) } catch (_: Exception) {}
    }
  }

  private fun handleNativeEngineEvent(app: Context, eventJSON: String) {
    val kind = try {
      JSONObject(eventJSON).optString("kind", "")
    } catch (_: Exception) {
      return
    }
    if (kind != "received") return
    if (isForeground) return
    if (!BackgroundSyncPrefs.autoApplyReceived(app)) return
    if (!canApplyInBackground(app)) return

    engineScope.launch {
      val eng = engine ?: return@launch
      if (isForeground) return@launch
      tryAutoApplyLatestReceivedInBackground(app, eng)
    }
  }

  private fun tryAutoApplyLatestReceivedInBackground(app: Context, eng: Engine) {
    try {
      val list = JSONArray(eng.listReceived())
      if (list.length() == 0) return
      val newest = list.optJSONObject(0) ?: return
      if (newest.optString("apply_state", "") != "available") return
      val receiptID = newest.optString("receipt_id", "")
      if (receiptID.isEmpty()) return
      applyReceivedToClipboard(app, receiptID)
    } catch (error: Exception) {
      Log.w(TAG, "Background auto-apply skipped: ${error.javaClass.simpleName}")
    }
  }

  fun applyReceivedToClipboard(context: Context, receiptID: String): String {
    val app = ensureInitialized(context)
    val eng = engine ?: throw IllegalStateException("engine is not initialized")
    val list = JSONArray(eng.listReceived())
    var entry: JSONObject? = null
    for (i in 0 until list.length()) {
      val cand = list.optJSONObject(i)
      if (cand != null && cand.optString("receipt_id") == receiptID) {
        entry = cand
        break
      }
    }
    val parts = entry?.optJSONObject("content")?.optJSONArray("parts")
      ?: throw NoSuchElementException("received item not found")

    val expectedEpoch = observationEpoch
    val expectedSeq = observationSequence.toString()
    val beginReq = JSONObject()
      .put("receipt_id", receiptID)
      .put("expected_epoch", expectedEpoch)
      .put("expected_sequence", expectedSeq)
    val ticket = JSONObject(eng.beginApply(beginReq.toString()))
    val token = ticket.getString("token")
    val leaseID = ticket.getString("lease_id")

    var reported = false
    try {
      var applied = false
      var imgPart: JSONObject? = null
      var txtPart: JSONObject? = null
      for (i in 0 until parts.length()) {
        val p = parts.optJSONObject(i) ?: continue
        if (p.optString("kind") != "representation") continue
        val mime = p.optString("mime", "").lowercase()
        if (imgPart == null && mime.startsWith("image/")) {
          imgPart = p
        } else if (txtPart == null && (mime == "text/plain" || mime == "text/html")) {
          txtPart = p
        }
      }

      if (imgPart != null) {
        val partID = imgPart.optString("id", "")
        val mime = imgPart.optString("mime", "image/png")
        val ext = imageExtensionForMime(mime)
        val cacheRoot = resolvedCacheDir ?: File(app.cacheDir, "unic-cache").apply { mkdirs() }.canonicalPath
        val clipDir = File(cacheRoot, "clipboard").apply { mkdirs() }
        val destFile = File(clipDir, "clip-recv-${UUID.randomUUID()}.$ext")
        eng.exportPartToFile(leaseID, partID, destFile.canonicalPath)
        val imgBytes = destFile.readBytes()
        val appliedFp = "img:${sha256Hex(imgBytes)}"
        lastAppliedFingerprint = appliedFp
        lastObservedFingerprint = appliedFp
        applied = writeImageToClipboardAware(app, destFile)
        cleanupOldClipboardFiles(clipDir)
      } else if (txtPart != null) {
        val partID = txtPart.optString("id", "")
        val previewJSON = eng.readLeasedText(leaseID, partID, 4194304L)
        val text = JSONObject(previewJSON).optString("text", "")
        if (text.isNotEmpty()) {
          val appliedFp = "text:${sha256Hex(text.toByteArray(Charsets.UTF_8))}"
          lastAppliedFingerprint = appliedFp
          lastObservedFingerprint = appliedFp
          applied = writeTextToClipboardAware(app, text)
        }
      }

      if (!applied) {
        reportApplyFailure(eng, token, if (!isForeground) "permission_denied" else "unsupported")
        reported = true
        throw UnsupportedOperationException("received content cannot be applied to clipboard")
      }

      val reportPayload = if (expectedEpoch.isEmpty()) {
        JSONObject()
          .put("token", token)
          .put("outcome", "applied")
          .put("epoch", "")
          .put("sequence", "0")
      } else {
        val nextSeq = observationSequence + 1L
        observationSequence = nextSeq
        JSONObject()
          .put("token", token)
          .put("outcome", "applied")
          .put("baseline_lease_id", leaseID)
          .put("epoch", expectedEpoch)
          .put("sequence", nextSeq.toString())
      }
      eng.reportApplyResult(reportPayload.toString())
      reported = true
      return "{\"applied\":true}"
    } finally {
      if (!reported) {
        reportApplyFailure(eng, token, "internal")
      }
      try { eng.releaseContent(leaseID) } catch (_: Exception) {}
    }
  }

  fun applyHistoryToClipboard(context: Context, historyID: String): String {
    val app = ensureInitialized(context)
    val eng = engine ?: throw IllegalStateException("engine is not initialized")
    val expectedEpoch = observationEpoch
    val expectedSeq = observationSequence.toString()
    val ticketJSON = eng.beginApplyHistory(historyID, expectedEpoch, expectedSeq, true)
    val ticket = JSONObject(ticketJSON)
    val token = ticket.getString("token")
    val leaseID = ticket.getString("lease_id")

    var reported = false
    try {
      var applied = false
      val itemObj = JSONObject(eng.getHistoryItem(historyID))
      val kind = itemObj.optString("kind", "")
      if (kind == "image") {
        val cacheRoot = resolvedCacheDir ?: File(app.cacheDir, "unic-cache").apply { mkdirs() }.canonicalPath
        val clipDir = File(cacheRoot, "clipboard").apply { mkdirs() }
        val tmpFile = File(clipDir, "clip-${UUID.randomUUID()}.png")
        val expObj = JSONObject(eng.exportHistoryPartToFile(historyID, "image/", tmpFile.canonicalPath))
        val mime = expObj.optJSONObject("part")?.optString("mime", "image/png") ?: "image/png"
        val ext = imageExtensionForMime(mime)
        val finalFile = if (ext != "png") {
          val renamed = File(clipDir, "clip-${UUID.randomUUID()}.$ext")
          if (tmpFile.renameTo(renamed)) renamed else tmpFile
        } else {
          tmpFile
        }
        val imgBytes = finalFile.readBytes()
        val fp = "img:${sha256Hex(imgBytes)}"
        lastAppliedFingerprint = fp
        lastObservedFingerprint = fp
        applied = writeImageToClipboardAware(app, finalFile)
        cleanupOldClipboardFiles(clipDir)
      } else {
        val previewJSON = eng.readHistoryFullText(historyID, 4194304L)
        val text = JSONObject(previewJSON).optString("text", "")
        if (text.isNotEmpty()) {
          val fp = "text:${sha256Hex(text.toByteArray(Charsets.UTF_8))}"
          lastAppliedFingerprint = fp
          lastObservedFingerprint = fp
          applied = writeTextToClipboardAware(app, text)
        }
      }

      if (!applied) {
        reportApplyFailure(eng, token, if (!isForeground) "permission_denied" else "unsupported")
        reported = true
        throw UnsupportedOperationException("clipboard content could not be applied")
      }

      val reportPayload = if (expectedEpoch.isEmpty()) {
        JSONObject()
          .put("token", token)
          .put("outcome", "applied")
          .put("epoch", "")
          .put("sequence", "0")
      } else {
        val nextSeq = observationSequence + 1L
        observationSequence = nextSeq
        JSONObject()
          .put("token", token)
          .put("outcome", "applied")
          .put("baseline_lease_id", leaseID)
          .put("epoch", expectedEpoch)
          .put("sequence", nextSeq.toString())
      }
      eng.reportApplyResult(reportPayload.toString())
      reported = true
      return "{\"applied\":true}"
    } finally {
      if (!reported) {
        reportApplyFailure(eng, token, "internal")
      }
      try { eng.releaseContent(leaseID) } catch (_: Exception) {}
    }
  }

  private fun writeTextToClipboardAware(app: Context, text: String): Boolean {
    overlayController?.suppressLogcatBriefly()
    if (!isForeground) {
      val method = BackgroundSyncPrefs.clipboardAccessMethod(app)
      if (method == ClipboardAccessMethod.SHIZUKU && shizukuController?.hasPermission() == true) {
        if (shizukuController?.writeTextSync(text) == true) return true
      } else if (overlayController?.hasOverlayPermission() == true) {
        if (overlayController?.writeTextSync(text) == true) return true
      }
      return false
    }
    return onMainSync {
      val cm = app.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return@onMainSync false
      cm.setPrimaryClip(ClipData.newPlainText("UniClipboard", text))
      true
    }
  }

  private fun writeImageToClipboardAware(app: Context, destFile: File): Boolean {
    overlayController?.suppressLogcatBriefly()
    val contentUri = FileProvider.getUriForFile(app, "${app.packageName}.unic.fileprovider", destFile)
    if (!isForeground) {
      if (overlayController?.hasOverlayPermission() == true) {
        return overlayController?.writeImageUriSync(contentUri) == true
      }
      return false
    }
    return onMainSync {
      val cm = app.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return@onMainSync false
      cm.setPrimaryClip(ClipData.newUri(app.contentResolver, "UniClipboard", contentUri))
      true
    }
  }

  // ---------------------------------------------------------------------------
  // Monitors: Network, PrimaryClipListener, Screen state
  // ---------------------------------------------------------------------------

  private fun startNetworkMonitor(app: Context) {
    if (networkCallback != null) return
    val cm = app.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return
    val cb = object : ConnectivityManager.NetworkCallback() {
      override fun onAvailable(network: Network) {
        engineScope.launch {
          reconcileRuntimeState(app)
          reconcileBackgroundMonitors(app)
          refreshHostCapabilities(app)
          try { engine?.notifyNetworkChanged() } catch (_: Exception) {}
        }
      }

      override fun onCapabilitiesChanged(network: Network, networkCapabilities: NetworkCapabilities) {
        engineScope.launch {
          reconcileRuntimeState(app)
          reconcileBackgroundMonitors(app)
          refreshHostCapabilities(app)
        }
      }

      override fun onLost(network: Network) {
        engineScope.launch {
          reconcileRuntimeState(app)
          reconcileBackgroundMonitors(app)
          refreshHostCapabilities(app)
          try { engine?.notifyNetworkChanged() } catch (_: Exception) {}
        }
      }
    }
    try {
      cm.registerDefaultNetworkCallback(cb)
      connectivityManager = cm
      networkCallback = cb
    } catch (_: Exception) {
    }
  }

  private fun stopNetworkMonitor() {
    val cb = networkCallback ?: return
    val cm = connectivityManager
    networkCallback = null
    connectivityManager = null
    try {
      cm?.unregisterNetworkCallback(cb)
    } catch (_: Exception) {
    }
  }

  private fun startClipboardListener(app: Context) {
    if (clipListener != null) return
    mainHandler.post {
      if (clipListener != null) return@post
      val cm = app.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return@post
      val listener = ClipboardManager.OnPrimaryClipChangedListener {
        if (!isForeground) {
          // While backgrounded, attempting to inspect primaryClip is what prompts Android's
          // ClipboardService to log the denial line for our package when `overlay-event` is active.
          if (BackgroundSyncPrefs.clipboardAccessMethod(app) == ClipboardAccessMethod.OVERLAY_EVENT) {
            runCatching { cm.primaryClip }
          }
          return@OnPrimaryClipChangedListener
        }
        engineScope.launch {
          val eng = engine ?: return@launch
          if (!isForeground) return@launch
          if (BackgroundSyncPrefs.automaticCapture(app)) {
            try {
              val snap = readAndroidClipboardSnapshot(app)
              observeClipSnapshot(app, eng, snap, explicit = false)
            } catch (_: Exception) {
            }
          }
        }
      }
      cm.addPrimaryClipChangedListener(listener)
      clipboardManager = cm
      clipListener = listener
    }
  }

  private fun stopClipboardListener() {
    val listener = clipListener ?: return
    val cm = clipboardManager
    clipListener = null
    clipboardManager = null
    mainHandler.post {
      try {
        cm?.removePrimaryClipChangedListener(listener)
      } catch (_: Exception) {
      }
    }
  }

  private fun startScreenReceiver(app: Context) {
    if (screenReceiver != null) return
    val receiver = object : BroadcastReceiver() {
      override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
          Intent.ACTION_SCREEN_OFF -> applyScreenInteractiveState(interactive = false)
          Intent.ACTION_USER_PRESENT -> applyScreenInteractiveState(interactive = true)
          else -> engineScope.launch { updateScreenInteractiveState(app) }
        }
      }
    }
    val filter = IntentFilter().apply {
      addAction(Intent.ACTION_SCREEN_OFF)
      addAction(Intent.ACTION_SCREEN_ON)
      addAction(Intent.ACTION_USER_PRESENT)
    }
    try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        app.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
      } else {
        @Suppress("UnspecifiedRegisterReceiverFlag")
        app.registerReceiver(receiver, filter)
      }
      screenReceiver = receiver
    } catch (_: Exception) {
    }
  }

  private fun stopScreenReceiver() {
    val receiver = screenReceiver ?: return
    screenReceiver = null
    try {
      appContext?.unregisterReceiver(receiver)
    } catch (_: Exception) {
    }
  }

  private fun applyScreenInteractiveState(interactive: Boolean) {
    overlayController?.setScreenInteractive(interactive)
    shizukuController?.setScreenInteractive(interactive)
    resourceCollector?.recordStateSnapshot(screenState = if (interactive) "on" else "off")
    engineScope.launch {
      resourceCollector?.onStateTransition(engine)
    }
  }

  private fun updateScreenInteractiveState(app: Context) {
    val pm = app.getSystemService(Context.POWER_SERVICE) as? PowerManager
    val km = app.getSystemService(Context.KEYGUARD_SERVICE) as? KeyguardManager
    val interactive = (pm?.isInteractive ?: true) && (km?.isKeyguardLocked != true)
    applyScreenInteractiveState(interactive)
  }

  // ---------------------------------------------------------------------------
  // Clipboard reading, staging & utilities
  // ---------------------------------------------------------------------------

  private fun ensureEmptyObservationBaseline(eng: Engine) {
    if (observationEpoch.isNotEmpty()) return
    val newEpoch = "android-${UUID.randomUUID().toString().take(12)}"
    val req = JSONObject()
      .put("epoch", newEpoch)
      .put("sequence", "1")
      .put("mode", "baseline")
    eng.observeClipboard(req.toString())
    observationEpoch = newEpoch
    observationSequence = 1L
  }

  private fun readAndroidClipboardSnapshot(ctx: Context): AndroidClipSnapshot {
    return onMainSync {
      val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        ?: return@onMainSync AndroidClipSnapshot.Empty
      val clip = try {
        cm.primaryClip
      } catch (_: Exception) {
        null
      } ?: return@onMainSync AndroidClipSnapshot.Empty
      if (clip.itemCount <= 0) return@onMainSync AndroidClipSnapshot.Empty
      val item = clip.getItemAt(0)
      val uri = item.uri
      if (uri != null) {
        val resolvedMime = ctx.contentResolver.getType(uri)
          ?: (if (clip.description.mimeTypeCount > 0) clip.description.getMimeType(0) else null)
        if (resolvedMime != null && resolvedMime.startsWith("image/", ignoreCase = true)) {
          return@onMainSync AndroidClipSnapshot.ImageUri(uri, resolvedMime.lowercase())
        }
      }
      val text = item.coerceToText(ctx)?.toString()
      if (!text.isNullOrEmpty() && !text.trim().startsWith("unic-pair:")) {
        return@onMainSync AndroidClipSnapshot.Text(text, item.htmlText ?: "")
      }
      AndroidClipSnapshot.Empty
    }
  }

  private fun stageAndroidClipboardSnapshot(
    eng: Engine,
    ctx: Context,
    snap: AndroidClipSnapshot,
  ): Pair<String, String>? {
    return when (snap) {
      is AndroidClipSnapshot.Empty -> null
      is AndroidClipSnapshot.Text -> {
        val fp = "text:${sha256Hex(snap.text.toByteArray(Charsets.UTF_8))}"
        val handleJSON = eng.importText(snap.text, snap.html, "android-cb:$fp")
        val leaseID = JSONObject(handleJSON).optString("lease_id", "")
        if (leaseID.isEmpty()) null else Pair(fp, leaseID)
      }
      is AndroidClipSnapshot.StagedImageFile -> {
        val maxRepBytes = 32 * 1024 * 1024L
        if (!snap.file.exists() || snap.file.length() <= 0L) return null
        if (snap.file.length() > maxRepBytes) return null
        val rawBytes = snap.file.readBytes()
        stageImageBytes(eng, rawBytes, snap.mime)
      }
      is AndroidClipSnapshot.ImageUri -> {
        val maxRepBytes = 32 * 1024 * 1024
        val rawBytes = ctx.contentResolver.openInputStream(snap.uri)?.use { input ->
          val out = ByteArrayOutputStream()
          val buf = ByteArray(16384)
          var total = 0
          var exceeded = false
          while (true) {
            val n = input.read(buf)
            if (n <= 0) break
            total += n
            if (total > maxRepBytes) {
              exceeded = true
              break
            }
            out.write(buf, 0, n)
          }
          if (exceeded) null else out.toByteArray()
        }
        if (rawBytes == null || rawBytes.isEmpty()) {
          var displayName = snap.uri.lastPathSegment ?: "clipboard-image.${imageExtensionForMime(snap.mime)}"
          var fileSize = 0L
          ctx.contentResolver.query(
            snap.uri,
            arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE),
            null,
            null,
            null,
          )?.use { c ->
            if (c.moveToFirst()) {
              val nIdx = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
              if (nIdx >= 0 && !c.isNull(nIdx)) displayName = c.getString(nIdx)
              val sIdx = c.getColumnIndex(OpenableColumns.SIZE)
              if (sIdx >= 0 && !c.isNull(sIdx)) fileSize = c.getLong(sIdx)
            }
          }
          if (fileSize <= 0L) return null
          val fp = "img-uri:${sha256Hex("${snap.uri}:$fileSize".toByteArray(Charsets.UTF_8))}"
          val opener = HostFileOpener(ctx, snap.uri, fileSize, "size:$fileSize;uri:${snap.uri.hashCode()}")
          val handleJSON = eng.importFileSource(displayName, snap.mime, true, opener)
          val leaseID = JSONObject(handleJSON).optString("lease_id", "")
          return if (leaseID.isEmpty()) null else Pair(fp, leaseID)
        }
        stageImageBytes(eng, rawBytes, snap.mime)
      }
    }
  }

  private fun stageImageBytes(eng: Engine, rawBytes: ByteArray, mime: String): Pair<String, String>? {
    if (rawBytes.isEmpty()) return null
    val fp = "img:${sha256Hex(rawBytes)}"
    val leaseID = if (mime == "image/png") {
      try {
        JSONObject(eng.importBytes("image/png", "", rawBytes)).optString("lease_id", "")
      } catch (_: Exception) {
        ""
      }
    } else {
      ""
    }
    val finalLease = if (leaseID.isNotEmpty()) {
      leaseID
    } else {
      val bmp = BitmapFactory.decodeByteArray(rawBytes, 0, rawBytes.size)
      if (bmp != null) {
        val pngOut = ByteArrayOutputStream()
        bmp.compress(Bitmap.CompressFormat.PNG, 100, pngOut)
        bmp.recycle()
        val pngBytes = pngOut.toByteArray()
        JSONObject(eng.importBytes("image/png", "", pngBytes)).optString("lease_id", "")
      } else {
        val fileName = "clipboard-image.${imageExtensionForMime(mime)}"
        JSONObject(eng.importBytes(mime, fileName, rawBytes)).optString("lease_id", "")
      }
    }
    return if (finalLease.isEmpty()) null else Pair(fp, finalLease)
  }

  private fun reportApplyFailure(eng: Engine, token: String, errorCode: String) {
    try {
      val payload = JSONObject()
        .put("token", token)
        .put("outcome", "failed")
        .put("error_code", errorCode)
      eng.reportApplyResult(payload.toString())
    } catch (_: Exception) {
    }
  }

  private fun cleanupOldClipboardFiles(dir: File) {
    val files = dir.listFiles()?.sortedByDescending { it.lastModified() } ?: return
    if (files.size <= 5) return
    for (f in files.drop(5)) {
      try { f.delete() } catch (_: Exception) {}
    }
  }

  private fun imageExtensionForMime(mime: String?): String {
    return when (mime?.lowercase()) {
      "image/jpeg", "image/jpg" -> "jpg"
      "image/webp" -> "webp"
      "image/gif" -> "gif"
      else -> "png"
    }
  }

  private fun sha256Hex(bytes: ByteArray): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(bytes)
    val sb = StringBuilder(digest.size * 2)
    for (b in digest) {
      sb.append(String.format("%02x", b))
    }
    return sb.toString()
  }

  private fun <T> onMainSync(block: () -> T): T {
    if (Looper.myLooper() == Looper.getMainLooper()) {
      return block()
    }
    val latch = CountDownLatch(1)
    var value: T? = null
    var failure: Throwable? = null
    mainHandler.post {
      try {
        value = block()
      } catch (t: Throwable) {
        failure = t
      } finally {
        latch.countDown()
      }
    }
    latch.await()
    failure?.let { throw it }
    @Suppress("UNCHECKED_CAST")
    return value as T
  }

  private class HostFileOpener(
    private val context: Context,
    private val uri: Uri,
    private val fileSize: Long,
    private val fileVersion: String,
  ) : FileOpener {
    override fun open(): FileStream {
      val stream: InputStream = context.contentResolver.openInputStream(uri)
        ?: throw IllegalStateException("cannot open content uri")
      return object : FileStream {
        @Volatile private var closed = false
        override fun readChunk(maxBytes: Long): ByteArray {
          if (closed || maxBytes <= 0) return ByteArray(0)
          val size = maxBytes.coerceAtMost(262144L).toInt()
          val buf = ByteArray(size)
          val n = stream.read(buf, 0, size)
          if (n <= 0) return ByteArray(0)
          return if (n == size) buf else buf.copyOf(n)
        }

        override fun close() {
          if (closed) return
          closed = true
          try { stream.close() } catch (_: Exception) {}
        }
      }
    }

    override fun size(): Long = fileSize
    override fun version(): String = fileVersion
  }
}
