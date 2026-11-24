package app.uniclipboard.engine

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.ConnectivityManager
import android.net.Network
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.system.Os
import android.view.WindowManager
import androidx.core.content.FileProvider
import expo.modules.kotlin.Promise
import expo.modules.kotlin.exception.CodedException
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.launch
import mobile.AndroidNetworkInterfaces
import mobile.CancelToken
import mobile.Engine
import mobile.EventSink
import mobile.FileOpener
import mobile.FileStream
import mobile.MigrationProgressSink
import mobile.MigrationSession
import mobile.Mobile
import mobile.SecretProvider
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

private const val REQ_PICK_SETUP_FOLDER = 0x5501
private const val REQ_PICK_UPLOAD_FILE = 0x5502

private val androidNetworkInterfaces = AndroidNetworkInterfaces {
  val snapshot = JSONArray()
  val interfaces = try {
    NetworkInterface.getNetworkInterfaces()?.toList().orEmpty()
  } catch (_: SocketException) {
    emptyList()
  }
  for (iface in interfaces) {
    try {
      val addresses = iface.interfaceAddresses.map { address ->
        "${address.address.hostAddress.substringBefore('%')}/${address.networkPrefixLength}"
      }
      snapshot.put(JSONObject()
        .put("name", iface.name)
        .put("index", iface.index)
        .put("mtu", iface.mtu)
        .put("up", iface.isUp)
        .put("loopback", iface.isLoopback)
        .put("addresses", JSONArray(addresses)))
    } catch (_: SocketException) {
      // An interface can disappear while the snapshot is being collected.
    }
  }
  snapshot.toString()
}

class UnicEngineModule : Module() {
  private val engineExecutor = Executors.newSingleThreadExecutor()
  private val engineDispatcher = engineExecutor.asCoroutineDispatcher()
  private val engineScope = CoroutineScope(SupervisorJob() + engineDispatcher)
  private val mainHandler = Handler(Looper.getMainLooper())

  private var engine: Engine? = null
  private var setupSession: MigrationSession? = null
  @Volatile private var activeMigrationCancel: CancelToken? = null

  private var pendingFolderPromise: Promise? = null
  private var pendingFilePromise: Pair<Promise, String?>? = null

  private var networkCallback: ConnectivityManager.NetworkCallback? = null
  private var connectivityManager: ConnectivityManager? = null

  private var observationEpoch: String = ""
  private var observationSequence: Long = 0L
  private var lastObservedFingerprint: String? = null
  private var lastAppliedFingerprint: String? = null
  @Volatile private var isForeground: Boolean = true
  private var clipListener: ClipboardManager.OnPrimaryClipChangedListener? = null
  private var clipboardManager: ClipboardManager? = null

  private sealed class AndroidClipSnapshot {
    object Empty : AndroidClipSnapshot()
    data class Text(val text: String, val html: String) : AndroidClipSnapshot()
    data class ImageUri(val uri: Uri, val mime: String) : AndroidClipSnapshot()
  }

  private fun sha256Hex(bytes: ByteArray): String {
    val digest = MessageDigest.getInstance("SHA-256").digest(bytes)
    val sb = StringBuilder(digest.size * 2)
    for (b in digest) {
      sb.append(String.format("%02x", b))
    }
    return sb.toString()
  }

  private fun imageExtensionForMime(mime: String?): String {
    return when (mime?.lowercase()) {
      "image/jpeg", "image/jpg" -> "jpg"
      "image/webp" -> "webp"
      "image/gif" -> "gif"
      else -> "png"
    }
  }

  private fun updateHostCapabilities(eng: Engine, observe: String, apply: String) {
    val caps = JSONObject()
      .put("read_formats", JSONArray(listOf("text/plain", "text/html", "image/png")))
      .put("write_formats", JSONArray(listOf("text/plain", "text/html", "image/png")))
      .put("read_files", true)
      .put("write_files", true)
      .put("observe", observe)
      .put("apply", apply)
    try {
      eng.updateHostCapabilities(caps.toString())
    } catch (_: Exception) {
    }
  }

  private fun isAutoSyncEnabled(eng: Engine): Boolean {
    return try {
      val state = JSONObject(eng.state)
      val sync = state.optJSONObject("settings")?.optJSONObject("value")?.optJSONObject("sync")
      sync != null && sync.optBoolean("enabled", false) && sync.optBoolean("automatic", false)
    } catch (_: Exception) {
      false
    }
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
      if (!text.isNullOrEmpty()) {
        return@onMainSync AndroidClipSnapshot.Text(text, item.htmlText ?: "")
      }
      AndroidClipSnapshot.Empty
    }
  }

  private fun stageAndroidClipboardSnapshot(eng: Engine, ctx: Context, snap: AndroidClipSnapshot): Pair<String, String>? {
    return when (snap) {
      is AndroidClipSnapshot.Empty -> null
      is AndroidClipSnapshot.Text -> {
        val fp = "text:${sha256Hex(snap.text.toByteArray(Charsets.UTF_8))}"
        val handleJSON = eng.importText(snap.text, snap.html, "android-cb:$fp")
        val leaseID = JSONObject(handleJSON).optString("lease_id", "")
        if (leaseID.isEmpty()) null else Pair(fp, leaseID)
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
          // Stream larger image via FileOpener as file attachment.
          var displayName = snap.uri.lastPathSegment ?: "clipboard-image.${imageExtensionForMime(snap.mime)}"
          var fileSize = 0L
          ctx.contentResolver.query(snap.uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
            if (c.moveToFirst()) {
              val nIdx = c.getColumnIndex(OpenableColumns.DISPLAY_NAME)
              if (nIdx >= 0 && !c.isNull(nIdx)) displayName = c.getString(nIdx)
              val sIdx = c.getColumnIndex(OpenableColumns.SIZE)
              if (sIdx >= 0 && !c.isNull(sIdx)) fileSize = c.getLong(sIdx)
            }
          }
          if (fileSize <= 0L) return null
          val fp = "img-uri:${sha256Hex("${snap.uri}:$fileSize".toByteArray(Charsets.UTF_8))}"
          val opener = AndroidFileOpener(ctx, snap.uri, fileSize, "size:$fileSize;uri:${snap.uri.hashCode()}")
          val handleJSON = eng.importFileSource(displayName, snap.mime, true, opener)
          val leaseID = JSONObject(handleJSON).optString("lease_id", "")
          return if (leaseID.isEmpty()) null else Pair(fp, leaseID)
        }
        val fp = "img:${sha256Hex(rawBytes)}"
        val leaseID = if (snap.mime == "image/png") {
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
            val fileName = "clipboard-image.${imageExtensionForMime(snap.mime)}"
            JSONObject(eng.importBytes(snap.mime, fileName, rawBytes)).optString("lease_id", "")
          }
        }
        if (finalLease.isEmpty()) null else Pair(fp, finalLease)
      }
    }
  }

  private fun observeCurrentAndroidClipboard(eng: Engine, ctx: Context, explicit: Boolean): String {
    val snap = readAndroidClipboardSnapshot(ctx)
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

  private fun syncAndroidForegroundClipboard(eng: Engine, isResume: Boolean) {
    val ctx = appContext.reactContext ?: return
    val hasFocus = onMainSync { appContext.currentActivity?.hasWindowFocus() ?: false }
    if (!hasFocus) {
      updateHostCapabilities(eng, "unavailable", if (isForeground) "available" else "unavailable")
      return
    }
    updateHostCapabilities(eng, "available", "available")
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
      if (changedWhileAway && isAutoSyncEnabled(eng)) {
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

  private fun scheduleForegroundClipboardSync(isResume: Boolean) {
    val activity = appContext.currentActivity
    if (activity != null) {
      mainHandler.postDelayed({
        engineScope.launch {
          val eng = engine ?: return@launch
          if (!isForeground) return@launch
          syncAndroidForegroundClipboard(eng, isResume)
        }
      }, 150L)
    } else {
      engineScope.launch {
        val eng = engine ?: return@launch
        if (!isForeground) return@launch
        updateHostCapabilities(eng, "unavailable", "available")
      }
    }
  }

  private fun startClipboardListener() {
    if (clipListener != null) return
    val ctx = appContext.reactContext ?: return
    mainHandler.post {
      val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return@post
      val listener = ClipboardManager.OnPrimaryClipChangedListener {
        if (!isForeground) return@OnPrimaryClipChangedListener
        engineScope.launch {
          val eng = engine ?: return@launch
          if (!isForeground) return@launch
          if (isAutoSyncEnabled(eng)) {
            try {
              observeCurrentAndroidClipboard(eng, ctx, explicit = false)
            } catch (_: Exception) {
            }
          } else {
            sendEvent("onClipboardChanged", mapOf("hasStrings" to true))
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

  private fun cleanupOldClipboardFiles(dir: File) {
    val files = dir.listFiles()?.sortedByDescending { it.lastModified() } ?: return
    if (files.size <= 5) return
    for (f in files.drop(5)) {
      try { f.delete() } catch (_: Exception) {}
    }
  }

  private fun writeImageFileToClipboard(ctx: Context, destFile: File) {
    val contentUri = FileProvider.getUriForFile(ctx, "${ctx.packageName}.unic.fileprovider", destFile)
    onMainSync {
      val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
        ?: throw rawCodeException("unavailable", "clipboard service unavailable")
      cm.setPrimaryClip(ClipData.newUri(ctx.contentResolver, "UniClipboard", contentUri))
    }
  }

  private fun formatGoError(err: Throwable): String {
    val raw = err.message ?: ""
    try {
      val obj = JSONObject(raw)
      val code = obj.optString("code", "internal")
      return "[unic:$code] $raw"
    } catch (_: Exception) {
      val fallback = JSONObject()
        .put("code", "internal")
        .put("message", "operation failed")
        .toString()
      return "[unic:internal] $fallback"
    }
  }

  private fun moduleException(err: Throwable): CodedException {
    return CodedException("UnicEngineError", formatGoError(err), err)
  }

  private fun rawCodeException(code: String, message: String): CodedException {
    val json = JSONObject().put("code", code).put("message", message).toString()
    return CodedException("UnicEngineError", "[unic:$code] $json", null)
  }

  private fun appContextOrThrow(): Context {
    return appContext.reactContext ?: throw rawCodeException("invalid_state", "react context unavailable")
  }

  private fun launchOverride(key: String): String? {
    val fromIntent = appContext.currentActivity?.intent?.getStringExtra(key)?.trim()
    if (!fromIntent.isNullOrEmpty()) {
      return fromIntent
    }
    val fromEnv = Os.getenv(key)?.trim()
    if (!fromEnv.isNullOrEmpty()) {
      return fromEnv
    }
    return null
  }

  private fun resolveDir(envName: String, fallback: File): String {
    val override = launchOverride(envName)
    val dir = if (override != null) File(override) else fallback
    if (!dir.exists()) {
      dir.mkdirs()
    }
    return dir.canonicalPath
  }

  private fun dataDirectoryPath(): String {
    val ctx = appContextOrThrow()
    return resolveDir("UNIC_MOBILE_DATA_DIR", File(ctx.filesDir, "unic-v2"))
  }

  private fun cacheDirectoryPath(): String {
    val ctx = appContextOrThrow()
    return resolveDir("UNIC_MOBILE_CACHE_DIR", File(ctx.cacheDir, "unic-cache"))
  }

  private fun defaultV1DirectoryPath(): String {
    val override = launchOverride("UNIC_MOBILE_V1_DIR")
    if (override != null) {
      return try { File(override).canonicalPath } catch (_: Exception) { override }
    }
    val ctx = appContextOrThrow()
    val fallback = File(ctx.filesDir, "UniClipboard")
    return try { fallback.canonicalPath } catch (_: Exception) { fallback.absolutePath }
  }

  private fun ensureSetupSession(): MigrationSession {
    setupSession?.let { return it }
    val created = Mobile.openSetupFlow(dataDirectoryPath(), defaultV1DirectoryPath())
    setupSession = created
    return created
  }

  private fun requireEngine(): Engine {
    return engine ?: throw rawCodeException("invalid_state", "engine is not initialized")
  }

  private fun shutdownEngineInternal() {
    engine?.let {
      try {
        it.close(null)
      } catch (_: Exception) {
      }
      engine = null
    }
  }

  private fun startNetworkMonitor() {
    if (networkCallback != null) return
    val ctx = appContext.reactContext ?: return
    val cm = ctx.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager ?: return
    val cb = object : ConnectivityManager.NetworkCallback() {
      override fun onAvailable(network: Network) {
        engineScope.launch {
          try { engine?.notifyNetworkChanged() } catch (_: Exception) {}
        }
      }

      override fun onLost(network: Network) {
        engineScope.launch {
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

  private fun setKeepScreenOn(enabled: Boolean) {
    val activity = appContext.currentActivity ?: return
    mainHandler.post {
      if (enabled) {
        activity.window?.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      } else {
        activity.window?.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
      }
    }
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

  private fun tryResolveTreeUriToLocalPath(uri: Uri): String? {
    if (uri.scheme == "file") {
      return uri.path
    }
    if (!DocumentsContract.isTreeUri(uri)) {
      return null
    }
    val docId = DocumentsContract.getTreeDocumentId(uri) ?: return null
    val parts = docId.split(":", limit = 2)
    if (parts.isEmpty()) return null
    if (parts[0].equals("primary", ignoreCase = true)) {
      val root = Environment.getExternalStorageDirectory()
      val rel = if (parts.size > 1) parts[1] else ""
      val candidate = if (rel.isEmpty()) root else File(root, rel)
      if (candidate.exists() && candidate.isDirectory) {
        return candidate.canonicalPath
      }
    }
    return null
  }

  private inner class AndroidFileOpener(
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

  private inner class NoStoredSecretProvider : SecretProvider {
    override fun readSecret(service: String?, account: String?): ByteArray {
      throw IllegalStateException("v1 secret must be entered via passphrase on android")
    }
  }

  override fun definition() = ModuleDefinition {
    Name("UnicEngine")

    Events("onEngineEvent", "onEngineResync", "onMigrationProgress", "onClipboardChanged")

    OnActivityEntersForeground {
      isForeground = true
      engineScope.launch {
        val eng = engine ?: return@launch
        try { eng.setRuntimeState("active") } catch (_: Exception) {}
        scheduleForegroundClipboardSync(isResume = true)
      }
    }

    OnActivityEntersBackground {
      isForeground = false
      engineScope.launch {
        observationEpoch = ""
        observationSequence = 0L
        val eng = engine ?: return@launch
        updateHostCapabilities(eng, "unavailable", "unavailable")
        try { eng.setRuntimeState("suspended") } catch (_: Exception) {}
      }
    }

    OnDestroy {
      activeMigrationCancel?.cancel()
      stopClipboardListener()
      stopNetworkMonitor()
      setKeepScreenOn(false)
      shutdownEngineInternal()
      engineExecutor.shutdownNow()
    }

    OnActivityResult { _, payload ->
      when (payload.requestCode) {
        REQ_PICK_SETUP_FOLDER -> {
          val promise = pendingFolderPromise ?: return@OnActivityResult
          pendingFolderPromise = null
          if (payload.resultCode != Activity.RESULT_OK || payload.data?.data == null) {
            promise.resolve("{\"cancelled\":true}")
            return@OnActivityResult
          }
          val uri = payload.data!!.data!!
          engineScope.launch {
            try {
              val localPath = tryResolveTreeUriToLocalPath(uri)
                ?: throw rawCodeException("invalid_argument", "unsupported or invalid v1 data directory")
              val sess = ensureSetupSession()
              val resJSON = sess.selectSource(localPath)
              val parsed = JSONObject(resJSON)
              val out = JSONObject()
                .put("cancelled", false)
                .put("source", parsed.optString("source", localPath))
              promise.resolve(out.toString())
            } catch (ce: CodedException) {
              promise.reject(ce)
            } catch (t: Throwable) {
              promise.reject(moduleException(t))
            }
          }
        }

        REQ_PICK_UPLOAD_FILE -> {
          val pending = pendingFilePromise ?: return@OnActivityResult
          pendingFilePromise = null
          val promise = pending.first
          val deviceIdsJson = pending.second
          if (payload.resultCode != Activity.RESULT_OK || payload.data?.data == null) {
            promise.resolve("{\"cancelled\":true}")
            return@OnActivityResult
          }
          val uri = payload.data!!.data!!
          engineScope.launch {
            try {
              val eng = requireEngine()
              val ctx = appContextOrThrow()
              var displayName = uri.lastPathSegment ?: "file"
              var fileSize = 0L
              ctx.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) {
                  val nameIdx = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                  if (nameIdx >= 0 && !cursor.isNull(nameIdx)) {
                    displayName = cursor.getString(nameIdx)
                  }
                  val sizeIdx = cursor.getColumnIndex(OpenableColumns.SIZE)
                  if (sizeIdx >= 0 && !cursor.isNull(sizeIdx)) {
                    fileSize = cursor.getLong(sizeIdx)
                  }
                }
              }
              val mime = ctx.contentResolver.getType(uri) ?: "application/octet-stream"
              val version = "size:$fileSize;uri:${uri.hashCode()}"
              val opener = AndroidFileOpener(ctx, uri, fileSize, version)
              val handleJSON = eng.importFileSource(displayName, mime, true, opener)
              val leaseID = JSONObject(handleJSON).optString("lease_id", "")
              if (leaseID.isEmpty()) {
                throw rawCodeException("internal", "invalid file handle")
              }
              try {
                val commitReq = JSONObject()
                  .put("request_id", "mobile-file-${UUID.randomUUID()}")
                  .put("lease_id", leaseID)
                if (!deviceIdsJson.isNullOrEmpty()) {
                  val ids = JSONArray(deviceIdsJson)
                  if (ids.length() > 0) commitReq.put("device_ids", ids)
                }
                val subJSON = eng.commitContent(null, commitReq.toString())
                val out = JSONObject()
                  .put("cancelled", false)
                  .put("submission", JSONObject(subJSON))
                promise.resolve(out.toString())
              } finally {
                try { eng.releaseContent(leaseID) } catch (_: Exception) {}
              }
            } catch (ce: CodedException) {
              promise.reject(ce)
            } catch (t: Throwable) {
              promise.reject(moduleException(t))
            }
          }
        }
      }
    }

    // ---- Setup & v1 Migration ----

    AsyncFunction("openSetupFlow") { ->
      try {
        ensureSetupSession().state
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("getSetupState") { ->
      try {
        ensureSetupSession().state
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("detectSetupSource") { dir: String ->
      try {
        ensureSetupSession().detectSource(dir)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("selectSetupCustomSource") { promise: Promise ->
      val activity = appContext.currentActivity
      if (activity == null) {
        promise.reject(rawCodeException("invalid_state", "no foreground activity"))
        return@AsyncFunction
      }
      pendingFolderPromise?.resolve("{\"cancelled\":true}")
      pendingFolderPromise = promise
      val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      }
      activity.startActivityForResult(intent, REQ_PICK_SETUP_FOLDER)
    }

    AsyncFunction("beginLaterMigration") { ->
      try {
        shutdownEngineInternal()
        ensureSetupSession().beginLaterMigration()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("runSetupMigration") { passphrase: String? ->
      shutdownEngineInternal()
      setKeepScreenOn(true)
      try {
        val sess = ensureSetupSession()
        val token = Mobile.newCancelToken()
        activeMigrationCancel = token
        val pwBytes = if (!passphrase.isNullOrEmpty()) passphrase.toByteArray(Charsets.UTF_8) else null
        val sink = MigrationProgressSink { stage, processed, total ->
          sendEvent(
            "onMigrationProgress",
            mapOf(
              "stage" to stage,
              "processed" to processed,
              "total" to total,
            ),
          )
        }
        sess.run(token, pwBytes, NoStoredSecretProvider(), sink)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      } finally {
        activeMigrationCancel = null
        setKeepScreenOn(false)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("cancelSetupMigration") { ->
      activeMigrationCancel?.cancel()
    }

    AsyncFunction("markSetupComplete") { ->
      try {
        ensureSetupSession().markComplete()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    // ---- Engine Lifecycle ----

    AsyncFunction("initializeEngine") { deviceName: String? ->
      try {
        engine?.let { return@AsyncFunction it.state }
        val resolvedName = deviceName?.trim()?.takeIf { it.isNotEmpty() } ?: (Build.MODEL ?: "Android")
        val cfg = JSONObject()
          .put("data_dir", dataDirectoryPath())
          .put("cache_dir", cacheDirectoryPath())
          .put("device_name", resolvedName)
          .put(
            "host_capabilities",
            JSONObject()
              .put("read_formats", JSONArray(listOf("text/plain", "text/html", "image/png")))
              .put("write_formats", JSONArray(listOf("text/plain", "text/html", "image/png")))
              .put("read_files", true)
              .put("write_files", true)
              .put("observe", if (isForeground) "available" else "unavailable")
              .put("apply", if (isForeground) "available" else "unavailable"),
          )
        Mobile.registerAndroidNetworkInterfaces(androidNetworkInterfaces)
        val eng = Mobile.newEngine(cfg.toString())
        val sink = object : EventSink {
          override fun onEvent(eventJSON: String?) {
            if (eventJSON != null) {
              sendEvent("onEngineEvent", mapOf("event" to eventJSON))
            }
          }

          override fun onResyncRequired(reason: String?) {
            sendEvent("onEngineResync", mapOf("reason" to (reason ?: "overflow")))
          }
        }
        val snapshot = eng.subscribe(sink)
        eng.setRuntimeState(if (isForeground) "active" else "suspended")
        eng.start(null)
        engine = eng
        observationEpoch = ""
        observationSequence = 0L
        startNetworkMonitor()
        startClipboardListener()
        if (isForeground) {
          scheduleForegroundClipboardSync(isResume = false)
        }
        eng.state.ifEmpty { snapshot }
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("shutdownEngine") { ->
      stopClipboardListener()
      stopNetworkMonitor()
      observationEpoch = ""
      observationSequence = 0L
      shutdownEngineInternal()
    }.runOnQueue(engineScope)

    AsyncFunction("getState") { ->
      try {
        requireEngine().state
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("getCapabilities") { ->
      try {
        requireEngine().capabilities
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("updateSettings") { reqJSON: String ->
      try {
        requireEngine().updateSettings(reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    // ---- History & Content ----

    AsyncFunction("listHistory") { reqJSON: String ->
      try {
        requireEngine().listHistory(reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("getHistoryItem") { historyID: String ->
      try {
        requireEngine().getHistoryItem(historyID)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("readHistoryTextPreview") { historyID: String, maxBytes: Int ->
      try {
        requireEngine().readHistoryTextPreview(historyID, maxBytes.toLong())
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("readHistoryFullText") { historyID: String, maxBytes: Int ->
      try {
        requireEngine().readHistoryFullText(historyID, maxBytes.toLong())
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("exportHistoryImagePreview") { historyID: String ->
      try {
        val eng = requireEngine()
        val previewDir = File(cacheDirectoryPath(), "previews")
        if (!previewDir.exists()) previewDir.mkdirs()
        val destFile = File(previewDir, "$historyID.img")
        val exportedJSON = eng.exportHistoryPartToFile(historyID, "image/", destFile.canonicalPath)
        val obj = JSONObject(exportedJSON)
        obj.put("uri", Uri.fromFile(destFile).toString())
        obj.toString()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("updateHistoryItem") { reqJSON: String ->
      try {
        requireEngine().updateHistoryItem(reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("deleteHistoryItem") { historyID: String, expectedRevision: String ->
      try {
        requireEngine().deleteHistoryItem(historyID, expectedRevision)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("listHistoryTags") { ->
      try {
        requireEngine().listHistoryTags()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("searchHistory") { reqJSON: String ->
      try {
        requireEngine().searchHistory(null, reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("countHistorySearches") { reqJSON: String ->
      try {
        requireEngine().countHistorySearches(null, reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("commitText") { text: String, deviceIdsJson: String? ->
      try {
        val eng = requireEngine()
        val handleJSON = eng.importText(text, "", "")
        val leaseID = JSONObject(handleJSON).optString("lease_id", "")
        if (leaseID.isEmpty()) {
          throw rawCodeException("internal", "invalid content handle")
        }
        try {
          val commitReq = JSONObject()
            .put("request_id", "mobile-text-${UUID.randomUUID()}")
            .put("lease_id", leaseID)
          if (!deviceIdsJson.isNullOrEmpty()) {
            val ids = JSONArray(deviceIdsJson)
            if (ids.length() > 0) commitReq.put("device_ids", ids)
          }
          eng.commitContent(null, commitReq.toString())
        } finally {
          try { eng.releaseContent(leaseID) } catch (_: Exception) {}
        }
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("pickAndCommitFile") { deviceIdsJson: String?, promise: Promise ->
      val activity = appContext.currentActivity
      if (activity == null) {
        promise.reject(rawCodeException("invalid_state", "no foreground activity"))
        return@AsyncFunction
      }
      pendingFilePromise?.first?.resolve("{\"cancelled\":true}")
      pendingFilePromise = Pair(promise, deviceIdsJson)
      val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = "*/*"
      }
      activity.startActivityForResult(intent, REQ_PICK_UPLOAD_FILE)
    }

    AsyncFunction("resendHistoryItem") { reqJSON: String ->
      try {
        requireEngine().resendHistoryItem(null, reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("listReceived") { ->
      try {
        requireEngine().listReceived()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("dismissReceived") { receiptID: String ->
      try {
        requireEngine().dismissReceived(receiptID)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("applyReceivedToClipboard") { receiptID: String ->
      try {
        val eng = requireEngine()
        val ctx = appContextOrThrow()
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
          ?: throw rawCodeException("not_found", "received item not found")

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
          var appliedFp: String? = null

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
            val clipDir = File(cacheDirectoryPath(), "clipboard")
            if (!clipDir.exists()) clipDir.mkdirs()
            val destFile = File(clipDir, "clip-recv-${UUID.randomUUID()}.$ext")
            eng.exportPartToFile(leaseID, partID, destFile.canonicalPath)
            val imgBytes = destFile.readBytes()
            appliedFp = "img:${sha256Hex(imgBytes)}"
            lastAppliedFingerprint = appliedFp
            lastObservedFingerprint = appliedFp
            writeImageFileToClipboard(ctx, destFile)
            cleanupOldClipboardFiles(clipDir)
            applied = true
          } else if (txtPart != null) {
            val partID = txtPart.optString("id", "")
            val previewJSON = eng.readLeasedText(leaseID, partID, 4194304L)
            val text = JSONObject(previewJSON).optString("text", "")
            if (text.isNotEmpty()) {
              appliedFp = "text:${sha256Hex(text.toByteArray(Charsets.UTF_8))}"
              lastAppliedFingerprint = appliedFp
              lastObservedFingerprint = appliedFp
              onMainSync {
                val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                  ?: throw rawCodeException("unavailable", "clipboard service unavailable")
                cm.setPrimaryClip(ClipData.newPlainText("UniClipboard", text))
              }
              applied = true
            }
          }

          if (!applied) {
            reportApplyFailure(eng, token, "unsupported")
            reported = true
            throw rawCodeException("unsupported", "received content cannot be applied to clipboard")
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
          "{\"applied\":true}"
        } finally {
          if (!reported) {
            reportApplyFailure(eng, token, "internal")
          }
          try { eng.releaseContent(leaseID) } catch (_: Exception) {}
        }
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("applyHistoryToClipboard") { historyID: String ->
      try {
        val eng = requireEngine()
        val ctx = appContextOrThrow()
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
            val clipDir = File(cacheDirectoryPath(), "clipboard")
            if (!clipDir.exists()) clipDir.mkdirs()
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
            writeImageFileToClipboard(ctx, finalFile)
            cleanupOldClipboardFiles(clipDir)
            applied = true
          } else {
            val previewJSON = eng.readHistoryFullText(historyID, 4194304L)
            val text = JSONObject(previewJSON).optString("text", "")
            if (text.isNotEmpty()) {
              val fp = "text:${sha256Hex(text.toByteArray(Charsets.UTF_8))}"
              lastAppliedFingerprint = fp
              lastObservedFingerprint = fp
              onMainSync {
                val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
                  ?: throw rawCodeException("unavailable", "clipboard service unavailable")
                cm.setPrimaryClip(ClipData.newPlainText("UniClipboard", text))
              }
              applied = true
            }
          }

          if (!applied) {
            reportApplyFailure(eng, token, "unsupported")
            reported = true
            throw rawCodeException("unsupported", "clipboard content could not be applied")
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
          "{\"applied\":true}"
        } finally {
          if (!reported) {
            reportApplyFailure(eng, token, "internal")
          }
          try { eng.releaseContent(leaseID) } catch (_: Exception) {}
        }
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("readSystemClipboardAndCommit") { ->
      try {
        val eng = requireEngine()
        val ctx = appContextOrThrow()
        observeCurrentAndroidClipboard(eng, ctx, explicit = true)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    // ---- Actions ----

    AsyncFunction("listActions") { ->
      try {
        requireEngine().listActions()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("runAction") { reqJSON: String ->
      try {
        requireEngine().runAction(null, reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    // ---- Pairing & Devices ----

    AsyncFunction("createPairingCode") { ->
      try {
        requireEngine().createPairingCode(null)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("getPairingCode") { ->
      try {
        requireEngine().pairingCode
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("cancelPairingCode") { ->
      try {
        requireEngine().cancelPairingCode()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("exportPairingTicket") { ->
      try {
        requireEngine().exportPairingTicket(null)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("joinByCode") { reqJSON: String ->
      try {
        requireEngine().joinByCode(null, reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("joinByTicket") { reqJSON: String ->
      try {
        requireEngine().joinByTicket(null, reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("getOperation") { opID: String ->
      try {
        requireEngine().getOperation(opID)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("listDevices") { ->
      try {
        requireEngine().listDevices()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("approveDevice") { reqJSON: String ->
      try {
        requireEngine().approveDevice(reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("renameDevice") { reqJSON: String ->
      try {
        requireEngine().renameDevice(reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("setBindingSync") { reqJSON: String ->
      try {
        requireEngine().setBindingSync(reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("unpairDevice") { deviceID: String, expectedRevision: Long ->
      try {
        requireEngine().unpairDevice(deviceID, expectedRevision)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("listCandidates") { ->
      try {
        requireEngine().listCandidates()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("pairCandidate") { reqJSON: String ->
      try {
        requireEngine().pairCandidate(reqJSON)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("ignoreCandidate") { deviceID: String, ignored: Boolean ->
      try {
        requireEngine().ignoreCandidate(deviceID, ignored)
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)
  }
}
