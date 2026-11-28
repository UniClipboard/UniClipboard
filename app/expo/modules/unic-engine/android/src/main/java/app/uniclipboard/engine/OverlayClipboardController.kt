package app.uniclipboard.engine

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.PixelFormat
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.util.Log
import android.view.Gravity
import android.view.View
import android.view.ViewTreeObserver
import android.view.WindowManager
import java.io.BufferedReader
import java.io.InputStreamReader
import java.util.ArrayDeque
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/**
 * Manages the two overlay-based Android background clipboard access modes:
 * 1. `overlay-polling`: native screen-aware periodic polling via a near-invisible 1px overlay.
 * 2. `overlay-event`: supervised `logcat` listener for `ClipboardService` denial lines, triggering
 *    an on-demand overlay focus read only when another app actually copies.
 *
 * Hardening for long-running reliability:
 * - Strictly serialized FIFO focus queue (concurrent reads & background auto-apply writes never race
 *   or leave the overlay stuck focusable).
 * - Single focus acquisition reads text, HTML, and image URI together.
 * - Automatically pauses polling while screen is off or locked; immediately checks on screen-on/unlock.
 * - Automatically recovers if WindowManager detaches or invalidates the persistent overlay view.
 * - Supervised `logcat` process uses `-T 1` (clock-independent), `redirectErrorStream(true)` (no stderr
 *   pipe deadlock), trailing-edge coalescing debounce (rapid copies never dropped), and exponential
 *   backoff restart if `logd` resets or the child process exits.
 */
internal class OverlayClipboardController(
  private val appContext: Context,
  private val onBackgroundClipCaptured: (AndroidClipSnapshot) -> Unit,
  private val onMonitorStateChanged: (() -> Unit)? = null,
) {
  companion object {
    private const val TAG = "UnicOverlayClip"
    private const val RETRY_DELAY_MS = 10L
    private const val MAX_RETRIES = 5
    private const val FOCUS_WAIT_TIMEOUT_MS = 1000L
    private const val IDLE_ALPHA = 0.01f
    private const val IDLE_DISMISS_MS = 10_000L
    private const val POLL_INTERVAL_MS = 1000L
    private const val LOGCAT_STARTUP_IGNORE_MS = 250L
    private const val LOGCAT_DEBOUNCE_MS = 150L
    private const val LOGCAT_MAX_COALESCE_MS = 400L
    private const val LOGCAT_HEALTHY_RUN_MS = 30_000L
    private const val LOGCAT_INITIAL_BACKOFF_MS = 1000L
    private const val LOGCAT_MAX_BACKOFF_MS = 30_000L
    private const val SELF_WRITE_SUPPRESS_MS = 1200L
    private const val SYNC_OP_TIMEOUT_MS = 3000L
  }

  private val mainHandler = Handler(Looper.getMainLooper())

  // Persistent 1px overlay window (accessed only on main thread).
  private var persistentView: View? = null
  private var persistentWindowManager: WindowManager? = null
  private var persistentParams: WindowManager.LayoutParams? = null
  private val idleDismissRunnable = Runnable { removePersistentOverlay() }

  // Strictly serialized focus queue (accessed only on main thread).
  private data class FocusTask(
    val tag: String,
    val onError: (String) -> Unit,
    val op: (Context, () -> Unit) -> Unit,
  )

  private val focusQueue = ArrayDeque<FocusTask>()
  private var focusBusy = false

  // Polling state (accessed on main thread).
  @Volatile private var pollingDesired = false
  @Volatile private var screenInteractive = true
  @Volatile private var appInForeground = true
  @Volatile private var cachedOverlayPermission = false
  @Volatile private var cachedReadLogsPermission = false
  private var pollReadInFlight = false

  private val pollRunnable = object : Runnable {
    override fun run() {
      if (!shouldRunPollTick()) return
      if (!pollReadInFlight && hasOverlayPermission()) {
        pollReadInFlight = true
        readSnapshotAsync("poll") { snapshot ->
          pollReadInFlight = false
          if (snapshot != null && snapshot !is AndroidClipSnapshot.Empty) {
            onBackgroundClipCaptured(snapshot)
          }
        }
      }
      if (shouldRunPollTick()) {
        mainHandler.postDelayed(this, POLL_INTERVAL_MS)
      }
    }
  }

  // Supervised logcat event-monitor state.
  @Volatile private var eventMonitorDesired = false
  @Volatile private var stopLogcat = true
  @Volatile private var logcatProcess: Process? = null
  private var logcatThread: Thread? = null
  @Volatile private var suppressLogcatUntilElapsedMs = 0L

  // Trailing-edge coalescing debounce for logcat triggers (accessed on main thread).
  private var burstStartElapsedMs = 0L
  private val logcatTriggerRunnable = Runnable {
    burstStartElapsedMs = 0L
    if (!eventMonitorDesired || appInForeground) return@Runnable
    if (!hasOverlayPermission()) return@Runnable
    readSnapshotAsync("logcat") { snapshot ->
      if (snapshot != null && snapshot !is AndroidClipSnapshot.Empty) {
        onBackgroundClipCaptured(snapshot)
      }
    }
  }

  fun hasOverlayPermission(): Boolean {
    val granted = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
      Settings.canDrawOverlays(appContext)
    } else {
      true
    }
    cachedOverlayPermission = granted
    return granted
  }

  fun hasReadLogsPermission(): Boolean {
    val granted = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
      appContext.checkSelfPermission(Manifest.permission.READ_LOGS) == PackageManager.PERMISSION_GRANTED
    } else {
      true
    }
    cachedReadLogsPermission = granted
    return granted
  }

  fun requestOverlayPermission(): Boolean {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return true
    val intent = Intent(
      Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
      Uri.parse("package:${appContext.packageName}")
    ).apply {
      addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    return try {
      appContext.startActivity(intent)
      true
    } catch (error: Exception) {
      Log.w(TAG, "Failed to launch overlay permission settings: ${error.javaClass.simpleName}")
      false
    }
  }

  fun suppressLogcatBriefly() {
    suppressLogcatUntilElapsedMs = SystemClock.elapsedRealtime() + SELF_WRITE_SUPPRESS_MS
  }

  fun setForegroundState(inForeground: Boolean) {
    appInForeground = inForeground
    mainHandler.post {
      if (inForeground) {
        mainHandler.removeCallbacks(pollRunnable)
        mainHandler.removeCallbacks(logcatTriggerRunnable)
        burstStartElapsedMs = 0L
        removePersistentOverlay()
      } else {
        reconcilePollingLoop()
      }
    }
  }

  fun setScreenInteractive(interactive: Boolean) {
    val becameInteractive = !screenInteractive && interactive
    screenInteractive = interactive
    mainHandler.post {
      if (!interactive) {
        mainHandler.removeCallbacks(pollRunnable)
        removePersistentOverlay()
      } else {
        reconcilePollingLoop(immediate = becameInteractive)
      }
    }
  }

  fun startPolling() {
    hasOverlayPermission()
    pollingDesired = true
    mainHandler.post { reconcilePollingLoop(immediate = true) }
  }

  fun stopPolling() {
    pollingDesired = false
    mainHandler.post {
      mainHandler.removeCallbacks(pollRunnable)
      pollReadInFlight = false
      if (!eventMonitorDesired) {
        removePersistentOverlay()
      }
    }
  }

  fun startEventMonitor() {
    if (eventMonitorDesired && logcatThread?.isAlive == true) return
    hasOverlayPermission()
    eventMonitorDesired = true
    if (!hasReadLogsPermission()) {
      Log.i(TAG, "READ_LOGS not granted yet; event monitor waiting")
      return
    }
    startLogcatSupervisor()
  }

  fun stopEventMonitor() {
    val wasActive = isEventMonitorActive()
    eventMonitorDesired = false
    stopLogcatSupervisor()
    if (wasActive) {
      onMonitorStateChanged?.invoke()
    }
    mainHandler.post {
      mainHandler.removeCallbacks(logcatTriggerRunnable)
      burstStartElapsedMs = 0L
      if (!pollingDesired) {
        removePersistentOverlay()
      }
    }
  }

  fun stopAll() {
    val wasActive = isPollingActive() || isEventMonitorActive()
    pollingDesired = false
    eventMonitorDesired = false
    stopLogcatSupervisor()
    if (wasActive) {
      onMonitorStateChanged?.invoke()
    }
    mainHandler.post {
      mainHandler.removeCallbacks(pollRunnable)
      mainHandler.removeCallbacks(logcatTriggerRunnable)
      burstStartElapsedMs = 0L
      pollReadInFlight = false
      focusQueue.clear()
      removePersistentOverlay()
    }
  }

  /**
   * Synchronously writes text to the Android clipboard using overlay window focus.
   * Must NOT be called on the main thread.
   */
  fun writeTextSync(text: String): Boolean {
    if (!hasOverlayPermission()) return false
    suppressLogcatBriefly()
    val latch = CountDownLatch(1)
    var success = false
    enqueueFocusTask(
      tag = "write-text",
      onError = { latch.countDown() },
      op = { context, done ->
        writeClipWithRetry(context, ClipData.newPlainText("UniClipboard", text), expectedText = text, attempt = 0) { ok ->
          success = ok
          done()
          latch.countDown()
        }
      },
    )
    try {
      latch.await(SYNC_OP_TIMEOUT_MS, TimeUnit.MILLISECONDS)
    } catch (_: InterruptedException) {
    }
    return success
  }

  /**
   * Synchronously writes an image content URI to the Android clipboard using overlay window focus.
   * Must NOT be called on the main thread.
   */
  fun writeImageUriSync(contentUri: Uri): Boolean {
    if (!hasOverlayPermission()) return false
    suppressLogcatBriefly()
    val latch = CountDownLatch(1)
    var success = false
    enqueueFocusTask(
      tag = "write-image",
      onError = { latch.countDown() },
      op = { context, done ->
        val clip = ClipData.newUri(context.contentResolver, "UniClipboard", contentUri)
        writeClipWithRetry(context, clip, expectedText = null, expectedUri = contentUri, attempt = 0) { ok ->
          success = ok
          done()
          latch.countDown()
        }
      },
    )
    try {
      latch.await(SYNC_OP_TIMEOUT_MS, TimeUnit.MILLISECONDS)
    } catch (_: InterruptedException) {
    }
    return success
  }

  private fun shouldRunPollTick(): Boolean =
    pollingDesired && !appInForeground && screenInteractive && hasOverlayPermission()

  fun isPollingActive(): Boolean =
    pollingDesired && !appInForeground && screenInteractive && cachedOverlayPermission

  fun isEventMonitorActive(): Boolean =
    eventMonitorDesired &&
      !stopLogcat &&
      !appInForeground &&
      cachedOverlayPermission &&
      cachedReadLogsPermission &&
      logcatProcess != null

  private fun reconcilePollingLoop(immediate: Boolean = false) {
    mainHandler.removeCallbacks(pollRunnable)
    if (!shouldRunPollTick()) return
    mainHandler.postDelayed(pollRunnable, if (immediate) 200L else POLL_INTERVAL_MS)
  }

  private fun readSnapshotAsync(tag: String, callback: (AndroidClipSnapshot?) -> Unit) {
    enqueueFocusTask(
      tag = tag,
      onError = { callback(null) },
      op = { context, done ->
        readClipWithRetry(context, 0) { clip ->
          val snapshot = extractSnapshot(context, clip)
          done()
          callback(snapshot)
        }
      },
    )
  }

  private fun extractSnapshot(context: Context, clip: ClipData?): AndroidClipSnapshot {
    if (clip == null || clip.itemCount <= 0) return AndroidClipSnapshot.Empty
    val item = clip.getItemAt(0)
    val uri = item.uri
    if (uri != null) {
      val resolvedMime = context.contentResolver.getType(uri)
        ?: (if (clip.description.mimeTypeCount > 0) clip.description.getMimeType(0) else null)
      if (resolvedMime != null && resolvedMime.startsWith("image/", ignoreCase = true)) {
        return AndroidClipSnapshot.ImageUri(uri, resolvedMime.lowercase())
      }
    }
    val text = item.coerceToText(context)?.toString()
    if (!text.isNullOrEmpty() && !text.trim().startsWith("unic-pair:")) {
      return AndroidClipSnapshot.Text(text, item.htmlText ?: "")
    }
    return AndroidClipSnapshot.Empty
  }

  // ---------------------------------------------------------------------------
  // Serialized focus queue & persistent overlay management (main thread only)
  // ---------------------------------------------------------------------------

  private fun enqueueFocusTask(
    tag: String,
    onError: (String) -> Unit,
    op: (Context, () -> Unit) -> Unit,
  ) {
    if (!hasOverlayPermission()) {
      onError("permission_required")
      return
    }
    mainHandler.post {
      // Drop stale queued poll ticks if another task is already queued to prevent backlog buildup.
      if (tag == "poll" && focusQueue.any { it.tag == "poll" }) {
        onError("coalesced")
        return@post
      }
      focusQueue.addLast(FocusTask(tag, onError, op))
      drainFocusQueue()
    }
  }

  private fun drainFocusQueue() {
    if (focusBusy) return
    val task = focusQueue.pollFirst() ?: return
    if (!hasOverlayPermission()) {
      task.onError("permission_required")
      drainFocusQueue()
      return
    }
    focusBusy = true
    resetIdleDismissTimer()

    val overlay = ensurePersistentOverlay()
    if (overlay == null) {
      focusBusy = false
      task.onError("overlay_unavailable")
      drainFocusQueue()
      return
    }
    val (view, wm, params) = overlay

    var released = false
    val releaseAndContinue = {
      if (!released) {
        released = true
        try {
          params.flags = unfocusableFlags(params.flags)
          wm.updateViewLayout(view, params)
        } catch (_: Exception) {
          removePersistentOverlay()
        }
        resetIdleDismissTimer()
        focusBusy = false
        drainFocusQueue()
      }
    }

    try {
      params.flags = focusableFlags(params.flags)
      wm.updateViewLayout(view, params)
      view.requestLayout()

      runWhenFocused(view) {
        try {
          task.op(appContext, releaseAndContinue)
        } catch (error: Exception) {
          Log.w(TAG, "Overlay op ${task.tag} failed: ${error.javaClass.simpleName}")
          releaseAndContinue()
          task.onError(error.message ?: "overlay_error")
        }
      }
    } catch (error: Exception) {
      Log.w(TAG, "Failed to update overlay layout (${task.tag}): ${error.javaClass.simpleName}")
      removePersistentOverlay()
      released = true
      focusBusy = false
      task.onError(error.message ?: "layout_error")
      drainFocusQueue()
    }
  }

  private fun ensurePersistentOverlay(): Triple<View, WindowManager, WindowManager.LayoutParams>? {
    val existingView = persistentView
    val existingWm = persistentWindowManager
    val existingParams = persistentParams
    if (existingView != null && existingWm != null && existingParams != null && existingView.isAttachedToWindow) {
      return Triple(existingView, existingWm, existingParams)
    }
    removePersistentOverlay()
    if (!hasOverlayPermission()) return null

    return try {
      val wm = appContext.getSystemService(Context.WINDOW_SERVICE) as? WindowManager ?: return null
      val view = View(appContext).apply {
        isFocusable = true
        isFocusableInTouchMode = true
        addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
          override fun onViewAttachedToWindow(v: View) {}
          override fun onViewDetachedFromWindow(v: View) {
            if (persistentView === v) {
              persistentView = null
              persistentWindowManager = null
              persistentParams = null
            }
          }
        })
      }

      val layoutType = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
      } else {
        @Suppress("DEPRECATION")
        WindowManager.LayoutParams.TYPE_PHONE
      }

      val params = WindowManager.LayoutParams(
        1,
        1,
        layoutType,
        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
          WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
        PixelFormat.TRANSLUCENT,
      ).apply {
        alpha = IDLE_ALPHA
        gravity = Gravity.START or Gravity.TOP
        x = 0
        y = 0
      }

      wm.addView(view, params)
      persistentView = view
      persistentWindowManager = wm
      persistentParams = params
      Triple(view, wm, params)
    } catch (error: Exception) {
      Log.w(TAG, "Unable to create overlay window: ${error.javaClass.simpleName}")
      removePersistentOverlay()
      null
    }
  }

  private fun removePersistentOverlay() {
    mainHandler.removeCallbacks(idleDismissRunnable)
    val view = persistentView
    val wm = persistentWindowManager
    persistentView = null
    persistentWindowManager = null
    persistentParams = null
    if (view != null && wm != null) {
      try {
        wm.removeView(view)
      } catch (_: Exception) {
      }
    }
  }

  private fun resetIdleDismissTimer() {
    mainHandler.removeCallbacks(idleDismissRunnable)
    mainHandler.postDelayed(idleDismissRunnable, IDLE_DISMISS_MS)
  }

  private fun focusableFlags(flags: Int): Int =
    (flags and WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE.inv()) or
      WindowManager.LayoutParams.FLAG_ALT_FOCUSABLE_IM

  private fun unfocusableFlags(flags: Int): Int =
    (flags or WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE) and
      WindowManager.LayoutParams.FLAG_ALT_FOCUSABLE_IM.inv()

  private fun runWhenFocused(view: View, then: () -> Unit) {
    if (view.hasWindowFocus()) {
      then()
      return
    }
    var settled = false
    var listener: ViewTreeObserver.OnWindowFocusChangeListener? = null
    val timeout = Runnable {
      if (!settled) {
        settled = true
        try {
          listener?.let { view.viewTreeObserver.removeOnWindowFocusChangeListener(it) }
        } catch (_: Exception) {
        }
        then()
      }
    }
    listener = ViewTreeObserver.OnWindowFocusChangeListener { hasFocus ->
      if (hasFocus && !settled) {
        settled = true
        mainHandler.removeCallbacks(timeout)
        try {
          listener?.let { view.viewTreeObserver.removeOnWindowFocusChangeListener(it) }
        } catch (_: Exception) {
        }
        then()
      }
    }
    try {
      view.viewTreeObserver.addOnWindowFocusChangeListener(listener)
      mainHandler.postDelayed(timeout, FOCUS_WAIT_TIMEOUT_MS)
    } catch (_: Exception) {
      then()
    }
  }

  private fun readClipWithRetry(context: Context, attempt: Int, callback: (ClipData?) -> Unit) {
    val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
    val clip = try {
      cm?.primaryClip
    } catch (_: Exception) {
      null
    }
    if (clip != null || attempt >= MAX_RETRIES) {
      callback(clip)
    } else {
      mainHandler.postDelayed({ readClipWithRetry(context, attempt + 1, callback) }, RETRY_DELAY_MS)
    }
  }

  private fun writeClipWithRetry(
    context: Context,
    clipData: ClipData,
    expectedText: String?,
    expectedUri: Uri? = null,
    attempt: Int,
    callback: (Boolean) -> Unit,
  ) {
    val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
    if (cm == null) {
      callback(false)
      return
    }
    try {
      cm.setPrimaryClip(clipData)
    } catch (_: Exception) {
    }
    val current = try {
      cm.primaryClip
    } catch (_: Exception) {
      null
    }
    val verified = current != null && current.itemCount > 0 && when {
      expectedText != null -> current.getItemAt(0).coerceToText(context)?.toString() == expectedText
      expectedUri != null -> current.getItemAt(0).uri == expectedUri
      else -> true
    }
    if (verified || attempt >= MAX_RETRIES) {
      callback(verified)
    } else {
      mainHandler.postDelayed(
        { writeClipWithRetry(context, clipData, expectedText, expectedUri, attempt + 1, callback) },
        RETRY_DELAY_MS,
      )
    }
  }

  // ---------------------------------------------------------------------------
  // Supervised logcat event trigger (background thread)
  // ---------------------------------------------------------------------------

  @Synchronized
  private fun startLogcatSupervisor() {
    if (logcatThread?.isAlive == true && !stopLogcat) return
    stopLogcat = false
    val pkg = appContext.packageName
    val thread = Thread({
      var backoffMs = LOGCAT_INITIAL_BACKOFF_MS
      while (!stopLogcat && eventMonitorDesired) {
        if (!hasReadLogsPermission()) {
          try {
            Thread.sleep(backoffMs)
          } catch (_: InterruptedException) {
            break
          }
          backoffMs = (backoffMs * 2).coerceAtMost(LOGCAT_MAX_BACKOFF_MS)
          continue
        }

        val startedAt = SystemClock.elapsedRealtime()
        var proc: Process? = null
        try {
          proc = ProcessBuilder(
            "logcat",
            "-v",
            "brief",
            "-T",
            "1",
            "ClipboardService:W",
            "*:S",
          )
            .redirectErrorStream(true)
            .start()
          logcatProcess = proc
          onMonitorStateChanged?.invoke()
          Log.i(TAG, "logcat event supervisor started")

          BufferedReader(InputStreamReader(proc.inputStream)).use { reader ->
            var line: String?
            while (!stopLogcat && eventMonitorDesired) {
              line = reader.readLine() ?: break
              val now = SystemClock.elapsedRealtime()
              if (now - startedAt < LOGCAT_STARTUP_IGNORE_MS) continue
              if (now < suppressLogcatUntilElapsedMs) continue
              if (line.contains(pkg)) {
                scheduleCoalescedLogcatRead()
              }
            }
          }
        } catch (error: Exception) {
          if (!stopLogcat && eventMonitorDesired) {
            Log.w(TAG, "logcat supervisor interrupted: ${error.javaClass.simpleName}")
          }
        } finally {
          destroyProcessQuietly(proc)
          if (logcatProcess === proc) {
            logcatProcess = null
            onMonitorStateChanged?.invoke()
          }
        }

        if (stopLogcat || !eventMonitorDesired) break
        val ranMs = SystemClock.elapsedRealtime() - startedAt
        backoffMs = if (ranMs >= LOGCAT_HEALTHY_RUN_MS) {
          LOGCAT_INITIAL_BACKOFF_MS
        } else {
          (backoffMs * 2).coerceAtMost(LOGCAT_MAX_BACKOFF_MS)
        }
        Log.i(TAG, "logcat exited after ${ranMs}ms; restarting in ${backoffMs}ms")
        try {
          Thread.sleep(backoffMs)
        } catch (_: InterruptedException) {
          break
        }
      }
    }, "unic-logcat-supervisor").apply {
      isDaemon = true
      start()
    }
    logcatThread = thread
  }

  @Synchronized
  private fun stopLogcatSupervisor() {
    stopLogcat = true
    destroyProcessQuietly(logcatProcess)
    logcatProcess = null
    try {
      logcatThread?.interrupt()
    } catch (_: Exception) {
    }
    logcatThread = null
  }

  private fun destroyProcessQuietly(proc: Process?) {
    if (proc == null) return
    try {
      proc.destroy()
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && proc.isAlive) {
        proc.destroyForcibly()
      }
    } catch (_: Exception) {
    }
  }

  private fun scheduleCoalescedLogcatRead() {
    mainHandler.post {
      if (!eventMonitorDesired || appInForeground) return@post
      val now = SystemClock.elapsedRealtime()
      if (burstStartElapsedMs == 0L) {
        burstStartElapsedMs = now
      }
      val elapsed = now - burstStartElapsedMs
      val remainingMax = (LOGCAT_MAX_COALESCE_MS - elapsed).coerceAtLeast(0L)
      val delay = minOf(LOGCAT_DEBOUNCE_MS, remainingMax)
      mainHandler.removeCallbacks(logcatTriggerRunnable)
      mainHandler.postDelayed(logcatTriggerRunnable, delay)
    }
  }
}
