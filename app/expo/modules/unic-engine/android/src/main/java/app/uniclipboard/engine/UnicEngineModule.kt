package app.uniclipboard.engine

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
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
import expo.modules.kotlin.Promise
import expo.modules.kotlin.exception.CodedException
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.launch
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
import java.io.File
import java.io.InputStream
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger

private const val REQ_PICK_SETUP_FOLDER = 0x5501
private const val REQ_PICK_UPLOAD_FILE = 0x5502

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

  private val clipboardEpoch = "android-cb-1"
  private val clipboardSeq = AtomicInteger(0)
  @Volatile private var lastAppliedSequence = -1

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

  private fun resolveDir(envName: String, fallback: File): String {
    val override = Os.getenv(envName)?.trim()
    val dir = if (!override.isNullOrEmpty()) File(override) else fallback
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
    val override = Os.getenv("UNIC_MOBILE_V1_DIR")?.trim()
    if (!override.isNullOrEmpty()) {
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

    Events("onEngineEvent", "onEngineResync", "onMigrationProgress")

    OnActivityEntersForeground {
      engineScope.launch {
        try { engine?.setRuntimeState("active") } catch (_: Exception) {}
      }
    }

    OnActivityEntersBackground {
      engineScope.launch {
        try { engine?.setRuntimeState("suspended") } catch (_: Exception) {}
      }
    }

    OnDestroy {
      activeMigrationCancel?.cancel()
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
              .put("read_formats", JSONArray(listOf("text/plain", "text/html", "image/png", "image/jpeg")))
              .put("write_formats", JSONArray(listOf("text/plain", "text/html", "image/png", "image/jpeg")))
              .put("read_files", true)
              .put("write_files", true)
              .put("observe", "available")
              .put("apply", "available"),
          )
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
        eng.setRuntimeState("active")
        eng.start(null)
        engine = eng
        startNetworkMonitor()
        eng.state.ifEmpty { snapshot }
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("shutdownEngine") { ->
      stopNetworkMonitor()
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

    AsyncFunction("applyHistoryToClipboard") { historyID: String ->
      try {
        val eng = requireEngine()
        val ctx = appContextOrThrow()
        val beforeSeq = clipboardSeq.get()
        val ticketJSON = eng.beginApplyHistory(historyID, clipboardEpoch, beforeSeq.toString(), true)
        val ticket = JSONObject(ticketJSON)
        val token = ticket.getString("token")
        val leaseID = ticket.getString("lease_id")

        var applied = false
        val previewJSON = eng.readHistoryTextPreview(historyID, 1048576L)
        val text = JSONObject(previewJSON).optString("text", "")
        if (text.isNotEmpty()) {
          onMainSync {
            val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
            cm?.setPrimaryClip(ClipData.newPlainText("UniClipboard", text))
          }
          applied = true
        }

        val afterSeq = if (applied) clipboardSeq.incrementAndGet() else beforeSeq
        if (applied) {
          lastAppliedSequence = afterSeq
        }

        val reportPayload = if (applied) {
          JSONObject()
            .put("token", token)
            .put("outcome", "applied")
            .put("baseline_lease_id", leaseID)
            .put("epoch", clipboardEpoch)
            .put("sequence", afterSeq.toString())
        } else {
          JSONObject()
            .put("token", token)
            .put("outcome", "failed")
            .put("error_code", "unavailable")
        }
        eng.reportApplyResult(reportPayload.toString())
        if (!applied) {
          throw rawCodeException("unavailable", "clipboard content could not be applied")
        }
        "{\"applied\":true}"
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
        val seq = clipboardSeq.incrementAndGet()
        val text = onMainSync {
          val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
          val clip = cm?.primaryClip
          if (clip != null && clip.itemCount > 0) clip.getItemAt(0).coerceToText(ctx)?.toString() else null
        }
        if (text.isNullOrEmpty()) {
          return@AsyncFunction "{\"skipped\":true,\"reason\":\"empty_or_unsupported\"}"
        }
        val handleJSON = eng.importText(text, "", "android-cb:$seq")
        val leaseID = JSONObject(handleJSON).optString("lease_id", "")
        if (leaseID.isEmpty()) {
          throw rawCodeException("internal", "failed to stage clipboard lease")
        }
        try {
          val obsReq = JSONObject()
            .put("epoch", clipboardEpoch)
            .put("sequence", seq.toString())
            .put("mode", "change")
            .put("lease_id", leaseID)
          eng.observeClipboard(obsReq.toString())
        } finally {
          try { eng.releaseContent(leaseID) } catch (_: Exception) {}
        }
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
