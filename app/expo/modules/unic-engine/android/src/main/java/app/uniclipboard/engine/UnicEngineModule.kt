package app.uniclipboard.engine

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageDecoder
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.system.Os
import android.view.WindowManager
import androidx.core.content.FileProvider
import expo.modules.interfaces.permissions.PermissionsStatus
import expo.modules.kotlin.Promise
import expo.modules.kotlin.exception.CodedException
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import kotlinx.coroutines.launch
import mobile.CancelToken
import mobile.Engine
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
import java.util.UUID
import java.util.concurrent.CountDownLatch

private const val REQ_PICK_SETUP_FOLDER = 0x5501
private const val REQ_PICK_UPLOAD_FILE = 0x5502
private const val REQ_PICK_IMAGE = 0x5503
private const val REQ_SAVE_FILES = 0x5504

class UnicEngineModule : Module() {
  private val engineScope get() = UnicEngineHost.engineScope
  private val mainHandler = Handler(Looper.getMainLooper())

  private var setupSession: MigrationSession? = null
  private var stagedMigrationSource: File? = null
  private var stagedMigrationURI: Uri? = null
  private val incomingShare by lazy { IncomingShare(appContextOrThrow()) }
  private var coldShareCaptured = false
  @Volatile private var activeMigrationCancel: CancelToken? = null

  private var pendingFolderPromise: Promise? = null
  private var pendingFilePromise: Pair<Promise, String?>? = null

  private data class ImageRequest(val promise: Promise, val cameraFile: File? = null)
  private data class ExportFile(val source: File, val name: String, val mime: String)
  private data class StagedExport(val directory: File, val files: List<ExportFile>)
  private data class ExportRequest(val promise: Promise, val staged: StagedExport)

  private var pendingPickerCommit: (() -> Unit)? = null
  private var pendingImage: ImageRequest? = null
  private var pendingExport: ExportRequest? = null

  private val uiListener = object : UnicEngineUiListener {
    override fun onEngineEvent(eventJSON: String) {
      runCatching { sendEvent("onEngineEvent", mapOf("event" to eventJSON)) }
    }

    override fun onEngineResync(reason: String) {
      runCatching { sendEvent("onEngineResync", mapOf("reason" to reason)) }
    }

    override fun onBackgroundSyncStateChanged(reason: String) {
      runCatching { sendEvent("onBackgroundSyncStateChanged", mapOf("reason" to reason)) }
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

  private fun legacyMobileConfigJSON(): String {
    val ctx = appContextOrThrow()
    ctx.getExternalFilesDir(null)
    val dbCandidates = org.json.JSONArray()
    val attachmentRoots = org.json.JSONArray()

    fun putPath(arr: org.json.JSONArray, file: File) {
      val path = try { file.canonicalPath } catch (_: Exception) { file.absolutePath }
      arr.put(path)
    }

    launchOverride("UNIC_MOBILE_LEGACY_DB")?.let { putPath(dbCandidates, File(it)) }
    putPath(dbCandidates, File(File(ctx.filesDir, "SQLite"), "uniclipboard.db"))
    putPath(dbCandidates, ctx.getDatabasePath("uniclipboard.db"))

    launchOverride("UNIC_MOBILE_LEGACY_ATTACHMENTS")?.split(File.pathSeparatorChar)?.forEach { segment ->
      val trimmed = segment.trim()
      if (trimmed.isNotEmpty()) putPath(attachmentRoots, File(trimmed))
    }
    putPath(attachmentRoots, File(ctx.filesDir, "clipboards"))
    putPath(attachmentRoots, File(ctx.cacheDir, "temp_files"))

    return JSONObject()
      .put("database_candidates", dbCandidates)
      .put("attachment_roots", attachmentRoots)
      .toString()
  }

  private fun ensureSetupSession(): MigrationSession {
    setupSession?.let { return it }
    File(appContextOrThrow().cacheDir, "unic-migration-source").deleteRecursively()
    val created = Mobile.openSetupFlow(dataDirectoryPath(), defaultV1DirectoryPath())
    created.configureLegacyMobile(legacyMobileConfigJSON())
    setupSession = created
    return created
  }

  private fun requireEngine(): Engine {
    return UnicEngineHost.engine ?: throw rawCodeException("invalid_state", "engine is not initialized")
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

  private fun selectMigrationTree(uri: Uri): JSONObject {
    val session = ensureSetupSession()
    val context = appContextOrThrow()
    val staged = stageMigrationTree(context, uri, File(context.cacheDir, "unic-migration-source"))
    try {
      val result = JSONObject(session.selectSource(staged.canonicalPath))
      clearStagedMigrationSource()
      stagedMigrationSource = staged
      stagedMigrationURI = uri
      return result
    } finally {
      if (stagedMigrationSource != staged) staged.deleteRecursively()
    }
  }

  private fun clearStagedMigrationSource() {
    stagedMigrationSource?.deleteRecursively()
    stagedMigrationSource = null
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

  private fun stageExport(id: String, imagesOnly: Boolean): StagedExport {
    val payload = JSONObject(requireEngine().stageHistoryExport(id, imagesOnly))
    val parts = payload.getJSONArray("files")
    return StagedExport(File(payload.getString("directory")), List(parts.length()) { i ->
      val file = parts.getJSONObject(i)
      ExportFile(File(file.getString("path")), file.getString("name"), file.getString("mime"))
    })
  }

  private fun commitPickedImage(uri: Uri): String {
    val resolver = appContextOrThrow().contentResolver
    val bitmap = if (Build.VERSION.SDK_INT >= 28) {
      ImageDecoder.decodeBitmap(ImageDecoder.createSource(resolver, uri)) { decoder, info, _ ->
        decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
        val scale = minOf(1.0, 4096.0 / maxOf(info.size.width, info.size.height))
        if (scale < 1) {
          decoder.setTargetSize(
            maxOf(1, (info.size.width * scale).toInt()), maxOf(1, (info.size.height * scale).toInt())
          )
        }
      }
    } else {
      val options = BitmapFactory.Options().apply { inJustDecodeBounds = true }
      resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, options) }
      if (options.outWidth <= 0 || options.outHeight <= 0) {
        throw rawCodeException("invalid_argument", "cannot read image")
      }
      options.inJustDecodeBounds = false
      options.inSampleSize = 1
      while (maxOf(options.outWidth, options.outHeight) / options.inSampleSize > 4096) {
        options.inSampleSize *= 2
      }
      resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, options) }
        ?: throw rawCodeException("invalid_argument", "cannot decode image")
    }
    val png = try {
      ByteArrayOutputStream().use { output ->
        if (!bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) {
          throw rawCodeException("internal", "cannot encode image")
        }
        output.toByteArray()
      }
    } finally {
      bitmap.recycle()
    }
    val eng = requireEngine()
    val lease = JSONObject(eng.importBytes("image/png", "", png)).getString("lease_id")
    try {
      val request = JSONObject().put("request_id", "mobile-image-${UUID.randomUUID()}").put("lease_id", lease)
      return JSONObject().put("cancelled", false)
        .put("submission", JSONObject(eng.commitContent(null, request.toString()))).toString()
    } finally {
      try { eng.releaseContent(lease) } catch (_: Exception) {}
    }
  }

  private fun savePhotos(staged: StagedExport) {
    val resolver = appContextOrThrow().contentResolver
    val inserted = mutableListOf<Uri>()
    try {
      for (file in staged.files) {
        val row = ContentValues().apply {
          put(MediaStore.Images.Media.DISPLAY_NAME, file.name)
          put(MediaStore.Images.Media.MIME_TYPE, file.mime)
          if (Build.VERSION.SDK_INT >= 29) {
            put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/UniClipboard")
            put(MediaStore.Images.Media.IS_PENDING, 1)
          }
        }
        val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, row)
          ?: throw rawCodeException("permission_denied", "cannot create photo")
        inserted.add(uri)
        writeExport(file, uri)
      }
      if (Build.VERSION.SDK_INT < 29) return
      for (uri in inserted) {
        val published = ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }
        if (resolver.update(uri, published, null, null) != 1) {
          throw rawCodeException("internal", "cannot publish photo")
        }
      }
    } catch (error: Exception) {
      inserted.forEach { uri -> runCatching { resolver.delete(uri, null, null) } }
      throw error
    }
  }

  private fun writeExport(file: ExportFile, uri: Uri) {
    val resolver = appContextOrThrow().contentResolver
    val stream = resolver.openOutputStream(uri, "wt")
      ?: throw rawCodeException("permission_denied", "cannot write destination")
    stream.use { output -> file.source.inputStream().use { input -> input.copyTo(output) } }
  }

  private fun presentDocumentExport(staged: StagedExport, promise: Promise) {
    mainHandler.post {
      val activity = appContext.currentActivity
      if (activity == null || pendingExport != null) {
        staged.directory.deleteRecursively()
        promise.reject(rawCodeException("invalid_state", "document picker unavailable"))
        return@post
      }
      try {
        val intent = if (staged.files.size == 1) {
          val file = staged.files.first()
          Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = file.mime
            putExtra(Intent.EXTRA_TITLE, file.name)
          }
        } else {
          Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
        }
        pendingExport = ExportRequest(promise, staged)
        activity.startActivityForResult(intent, REQ_SAVE_FILES)
      } catch (error: Exception) {
        pendingExport = null
        staged.directory.deleteRecursively()
        promise.reject(moduleException(error))
      }
    }
  }

  private fun writeDocumentExport(staged: StagedExport, destination: Uri) {
    val resolver = appContextOrThrow().contentResolver
    val written = mutableListOf<Uri>()
    try {
      if (staged.files.size == 1) {
        written.add(destination)
        writeExport(staged.files.first(), destination)
      } else {
        val directory = DocumentsContract.buildDocumentUriUsingTree(
          destination, DocumentsContract.getTreeDocumentId(destination)
        )
        for (file in staged.files) {
          val uri = DocumentsContract.createDocument(resolver, directory, file.mime, file.name)
            ?: throw rawCodeException("permission_denied", "cannot create document")
          written.add(uri)
          writeExport(file, uri)
        }
      }
    } catch (error: Exception) {
      written.forEach { uri -> runCatching { DocumentsContract.deleteDocument(resolver, uri) } }
      throw error
    }
  }

  private fun commitAfterResume(promise: Promise, action: () -> Unit) {
    val commit = {
      engineScope.launch {
        try {
          action()
        } catch (error: Exception) {
          promise.reject(if (error is CodedException) error else moduleException(error))
        }
      }
      Unit
    }
    if (UnicEngineHost.isForeground) commit() else pendingPickerCommit = commit
  }

  override fun definition() = ModuleDefinition {
    Name("UnicEngine")

    Events(
      "onEngineEvent",
      "onEngineResync",
      "onMigrationProgress",
      "onClipboardChanged",
      "onIncomingShare",
      "onBackgroundSyncStateChanged",
    )

    OnNewIntent { intent ->
      engineScope.launch {
        if (runCatching { incomingShare.stage(intent) }.getOrDefault(false)) {
          sendEvent("onIncomingShare", emptyMap<String, Any>())
        }
      }
    }

    AsyncFunction("listIncomingShares") { ->
      if (!coldShareCaptured) {
        coldShareCaptured = true
        appContext.currentActivity?.intent?.let { runCatching { incomingShare.stage(it) } }
      }
      incomingShare.list()
    }.runOnQueue(engineScope)

    AsyncFunction("saveIncomingShare") { id: String ->
      try {
        val item = incomingShare.item(id) ?: throw rawCodeException("not_found", "shared item unavailable")
        val result = requireEngine().commitSharedItem(item.path, "")
        incomingShare.remove(id)
        result
      } catch (error: Throwable) {
        throw if (error is CodedException) error else moduleException(error)
      }
    }.runOnQueue(engineScope)

    OnCreate {
      appContext.reactContext?.let { ctx ->
        UnicEngineHost.attachUi(ctx, uiListener)
      }
    }

    // ---- Background Sync & Clipboard Access Methods (Android) ----

    AsyncFunction("getBackgroundSyncEnabled") { ->
      BackgroundSyncPrefs.isEffectivelyEnabled(appContextOrThrow())
    }

    AsyncFunction("setBackgroundSyncEnabled") { enabled: Boolean, promise: Promise ->
      val context = appContextOrThrow()
      val apply = {
        engineScope.launch {
          try {
            val patch = JSONObject()
              .put("enabled", enabled)
              .put("temp_disabled", false)
            UnicEngineHost.updateBackgroundSyncConfig(context, patch.toString())
            promise.resolve(enabled)
          } catch (error: Exception) {
            promise.reject(moduleException(error))
          }
        }
      }
      mainHandler.post {
        val permission = Manifest.permission.POST_NOTIFICATIONS
        val permissions = appContext.permissions
        if (enabled && Build.VERSION.SDK_INT >= 33 && permissions?.hasGrantedPermissions(permission) != true) {
          if (permissions == null) {
            promise.reject(rawCodeException("permission_denied", "notifications unavailable"))
          } else {
            permissions.askForPermissions({ result ->
              if (result[permission]?.status == PermissionsStatus.GRANTED) apply()
              else promise.reject(rawCodeException("permission_denied", "notification access denied"))
            }, permission)
          }
        } else {
          apply()
        }
      }
    }

    AsyncFunction("getBackgroundSyncStatus") { ->
      try {
        UnicEngineHost.getBackgroundSyncStatusJSON(appContextOrThrow())
      } catch (error: Throwable) {
        throw if (error is CodedException) error else moduleException(error)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("updateBackgroundSyncConfig") { patchJSON: String ->
      try {
        UnicEngineHost.updateBackgroundSyncConfig(appContextOrThrow(), patchJSON)
      } catch (error: Throwable) {
        throw if (error is CodedException) error else moduleException(error)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("requestIgnoreBatteryOptimizations") { ->
      UnicEngineHost.requestIgnoreBatteryOptimizations(appContextOrThrow())
    }

    AsyncFunction("requestOverlayPermission") { ->
      UnicEngineHost.requestOverlayPermission(appContextOrThrow())
    }

    AsyncFunction("requestShizukuPermission") { ->
      UnicEngineHost.requestShizukuPermission(appContextOrThrow())
    }

    AsyncFunction("openShizukuAppOrGuide") { ->
      UnicEngineHost.openShizukuAppOrGuide(appContextOrThrow())
    }

    AsyncFunction("resolveBackgroundClipboardRestriction") { ->
      try {
        UnicEngineHost.resolveBackgroundClipboardRestriction(appContextOrThrow())
      } catch (error: Throwable) {
        throw if (error is CodedException) error else moduleException(error)
      }
    }.runOnQueue(engineScope)

    OnActivityEntersForeground {
      val context = appContext.reactContext
      if (context != null) {
        UnicEngineHost.onActivityEntersForeground(context) {
          appContext.currentActivity?.hasWindowFocus() ?: false
        }
      }
      pendingPickerCommit?.invoke()
      pendingPickerCommit = null
    }

    OnActivityEntersBackground {
      val context = appContext.reactContext
      if (context != null) {
        UnicEngineHost.onActivityEntersBackground(context)
      }
    }

    OnDestroy {
      activeMigrationCancel?.cancel()
      setKeepScreenOn(false)
      // Detach UI only; BackgroundSyncService retains UnicEngineHost so the Go Engine and
      // background clipboard monitor keep running after Activity / JS teardown.
      UnicEngineHost.detachUi(uiListener)
      pendingImage?.cameraFile?.delete()
      pendingExport?.staged?.directory?.deleteRecursively()
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
              val selected = selectMigrationTree(uri)
              val response = JSONObject()
                .put("cancelled", false)
                .put("source", selected.optString("source"))
                .put("source_kind", selected.optString("source_kind"))
                .put("requires_unlock", selected.optBoolean("requires_unlock", false))
              promise.resolve(response.toString())
            } catch (_: InvalidMigrationTree) {
              promise.reject(rawCodeException("invalid_argument", "invalid v1 backup directory"))
            } catch (_: MigrationTreeTooLarge) {
              promise.reject(rawCodeException("too_large", "v1 backup exceeds the staging limit"))
            } catch (ce: CodedException) {
              promise.reject(ce)
            } catch (t: Throwable) {
              promise.reject(moduleException(t))
            }
          }
        }

        REQ_PICK_IMAGE -> {
          val request = pendingImage ?: return@OnActivityResult
          pendingImage = null
          val uri = request.cameraFile?.let(Uri::fromFile) ?: payload.data?.data
          if (payload.resultCode != Activity.RESULT_OK || uri == null) {
            request.cameraFile?.delete()
            request.promise.resolve("{\"cancelled\":true}")
            return@OnActivityResult
          }
          commitAfterResume(request.promise) {
            try {
              request.promise.resolve(commitPickedImage(uri))
            } finally {
              request.cameraFile?.delete()
            }
          }
        }

        REQ_SAVE_FILES -> {
          val request = pendingExport ?: return@OnActivityResult
          pendingExport = null
          val uri = payload.data?.data
          if (payload.resultCode != Activity.RESULT_OK || uri == null) {
            request.staged.directory.deleteRecursively()
            request.promise.resolve(false)
            return@OnActivityResult
          }
          engineScope.launch {
            try {
              writeDocumentExport(request.staged, uri)
              request.promise.resolve(true)
            } catch (error: Exception) {
              request.promise.reject(if (error is CodedException) error else moduleException(error))
            } finally {
              request.staged.directory.deleteRecursively()
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
          commitAfterResume(promise) {
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
        val result = ensureSetupSession().detectSource(dir)
        val selected = JSONObject(result).optString("source")
        val stagedPath = stagedMigrationSource?.canonicalPath
        val insideStaged = stagedPath != null && (selected == stagedPath || selected.startsWith("$stagedPath${File.separator}"))
        if (selected.isNotEmpty() && !insideStaged) {
          clearStagedMigrationSource()
          stagedMigrationURI = null
        }
        result
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
        UnicEngineHost.shutdownForMigrationOrExplicitStop(appContextOrThrow())
        ensureSetupSession().beginLaterMigration()
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("runSetupMigration") { passphrase: String? ->
      UnicEngineHost.shutdownForMigrationOrExplicitStop(appContextOrThrow())
      setKeepScreenOn(true)
      try {
        val sess = ensureSetupSession()
        if (stagedMigrationSource == null) stagedMigrationURI?.let(::selectMigrationTree)
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
        try {
          val result = sess.run(token, pwBytes, NoStoredSecretProvider(), sink)
          if (JSONObject(result).optString("status") in setOf("complete", "done", "cancelled")) {
            clearStagedMigrationSource()
          }
          result
        } finally {
          pwBytes?.fill(0)
        }
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
        clearStagedMigrationSource()
        stagedMigrationURI = null
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    // ---- Engine Lifecycle ----

    AsyncFunction("initializeEngine") { deviceName: String? ->
      try {
        val ctx = appContextOrThrow()
        UnicEngineHost.attachUi(ctx, uiListener)
        UnicEngineHost.initializeEngine(
          context = ctx,
          dataDir = dataDirectoryPath(),
          cacheDir = cacheDirectoryPath(),
          deviceName = deviceName,
          hasFocusProvider = { appContext.currentActivity?.hasWindowFocus() ?: false },
        )
      } catch (ce: CodedException) {
        throw ce
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("shutdownEngine") { ->
      UnicEngineHost.shutdownForMigrationOrExplicitStop(appContextOrThrow())
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

    AsyncFunction("getTelemetryState") { ->
      val eng = requireEngine()
      try {
        eng.telemetryState
      } catch (error: Exception) {
        throw moduleException(error)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("setTelemetryEnabled") { enabled: Boolean ->
      val eng = requireEngine()
      try {
        eng.setTelemetryEnabled(enabled)
      } catch (error: Exception) {
        throw moduleException(error)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("exportRecentDiagnosticLogs") { ->
      requireEngine().exportRecentDiagnosticLogs()
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

    AsyncFunction("getFileCacheState") { ->
      try { requireEngine().fileCacheState } catch (error: Exception) { throw moduleException(error) }
    }.runOnQueue(engineScope)

    AsyncFunction("setAutomaticCapture") { enabled: Boolean ->
      UnicEngineHost.setAutomaticCapture(appContextOrThrow(), enabled)
    }.runOnQueue(engineScope)

    AsyncFunction("setAutoApplyReceived") { enabled: Boolean ->
      UnicEngineHost.setAutoApplyReceived(appContextOrThrow(), enabled)
    }.runOnQueue(engineScope)

    AsyncFunction("clearHistoryPreviews") { ->
      try { requireEngine().clearHistoryPreviews() } catch (error: Exception) { throw moduleException(error) }
    }.runOnQueue(engineScope)

    AsyncFunction("reconcileHistoryPreviews") { ->
      try { requireEngine().reconcileHistoryPreviews() } catch (error: Exception) { throw moduleException(error) }
    }.runOnQueue(engineScope)

    AsyncFunction("historyFileMetadata") { id: String ->
      try { requireEngine().historyFileMetadata(id) } catch (error: Exception) { throw moduleException(error) }
    }.runOnQueue(engineScope)

    AsyncFunction("listHistoryFolder") { id: String, limit: Int ->
      try { requireEngine().listHistoryFolder(id, limit.toLong()) } catch (error: Exception) { throw moduleException(error) }
    }.runOnQueue(engineScope)

    AsyncFunction("copyPairingTicket") { text: String, expiresAtMs: Double ->
      requireEngine()
      if (System.currentTimeMillis() >= expiresAtMs) {
        throw rawCodeException("expired", "pairing ticket expired")
      }
      UnicEngineHost.markSelfAppliedText(text)
      val context = appContextOrThrow()
      onMainSync {
        val clipboard = context.getSystemService(ClipboardManager::class.java)
        val clip = ClipData.newPlainText("UniClipboard", text)
        clip.description.extras = PersistableBundle().apply { putBoolean("android.content.extra.IS_SENSITIVE", true) }
        clipboard.setPrimaryClip(clip)
      }
      Unit
    }.runOnQueue(engineScope)

    AsyncFunction("copyPlainText") { text: String ->
      requireEngine()
      val ctx = appContextOrThrow()
      UnicEngineHost.markSelfAppliedText(text)
      onMainSync {
        val clipboard = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        clipboard.setPrimaryClip(ClipData.newPlainText("UniClipboard", text))
      }
    }.runOnQueue(engineScope)

    AsyncFunction("pickAndCommitImage") { camera: Boolean, promise: Promise ->
      val open = open@{
        if (pendingImage != null) {
          promise.reject(rawCodeException("invalid_state", "image picker already open"))
          return@open
        }
        val activity = appContext.currentActivity
        if (activity == null) {
          promise.reject(rawCodeException("invalid_state", "no foreground activity"))
          return@open
        }
        var cameraFile: File? = null
        try {
          val intent = if (camera) {
            val folder = File(activity.cacheDir, "unic-cache/camera").apply { mkdirs() }
            val file = File.createTempFile("camera-", ".jpg", folder)
            cameraFile = file
            val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.unic.fileprovider", file)
            Intent(MediaStore.ACTION_IMAGE_CAPTURE).apply {
              putExtra(MediaStore.EXTRA_OUTPUT, uri)
              clipData = ClipData.newRawUri("UniClipboard", uri)
              addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION or Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
          } else if (Build.VERSION.SDK_INT >= 33) {
            Intent(MediaStore.ACTION_PICK_IMAGES).setType("image/*")
          } else {
            Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("image/*")
          }
          pendingImage = ImageRequest(promise, cameraFile)
          activity.startActivityForResult(intent, REQ_PICK_IMAGE)
        } catch (error: android.content.ActivityNotFoundException) {
          pendingImage = null
          cameraFile?.delete()
          promise.reject(rawCodeException("unsupported", "image picker unavailable"))
        } catch (error: Exception) {
          pendingImage = null
          cameraFile?.delete()
          promise.reject(moduleException(error))
        }
      }
      mainHandler.post {
        val permissions = appContext.permissions
        val permission = Manifest.permission.CAMERA
        if (!camera || permissions?.hasGrantedPermissions(permission) == true) {
          open()
        } else if (permissions == null) {
          promise.reject(rawCodeException("permission_denied", "camera access unavailable"))
        } else {
          permissions.askForPermissions({ result ->
            if (result[permission]?.status == PermissionsStatus.GRANTED) open()
            else promise.reject(rawCodeException("permission_denied", "camera access denied"))
          }, permission)
        }
      }
    }

    AsyncFunction("exportHistoryMedia") { id: String, action: String, promise: Promise ->
      val photos = action == "photos"
      val startExport = {
        engineScope.launch {
          try {
            val staged = stageExport(id, photos)
            if (photos) {
              try {
                savePhotos(staged)
                promise.resolve(true)
              } finally {
                staged.directory.deleteRecursively()
              }
            } else {
              presentDocumentExport(staged, promise)
            }
          } catch (error: Exception) {
            promise.reject(if (error is CodedException) error else moduleException(error))
          }
        }
      }
      if (photos && Build.VERSION.SDK_INT < 29) {
        val permission = Manifest.permission.WRITE_EXTERNAL_STORAGE
        mainHandler.post {
          val permissions = appContext.permissions
          if (permissions == null) {
            promise.reject(rawCodeException("permission_denied", "photo access denied"))
          } else if (permissions.hasGrantedPermissions(permission)) {
            startExport()
          } else {
            permissions.askForPermissions({ result ->
              if (result[permission]?.status == PermissionsStatus.GRANTED) {
                startExport()
              } else {
                promise.reject(rawCodeException("permission_denied", "photo access denied"))
              }
            }, permission)
          }
        }
      } else {
        startExport()
      }
    }

    AsyncFunction("shareHistoryMedia") { idsJSON: String, text: String?, promise: Promise ->
      engineScope.launch {
        val staged = mutableListOf<StagedExport>()
        try {
          val ids = JSONArray(idsJSON)
          for (i in 0 until ids.length()) staged.add(stageExport(ids.getString(i), false))
          val files = staged.flatMap { it.files }
          val ctx = appContextOrThrow()
          val uris = ArrayList(files.map { file ->
            FileProvider.getUriForFile(ctx, "${ctx.packageName}.unic.fileprovider", file.source)
          })
          val intent = Intent(when {
            uris.size > 1 -> Intent.ACTION_SEND_MULTIPLE
            else -> Intent.ACTION_SEND
          }).apply {
            type = when {
              files.isEmpty() -> "text/plain"
              files.all { it.mime == files.first().mime } -> files.first().mime
              files.all { it.mime.startsWith("image/") } -> "image/*"
              else -> "*/*"
            }
            if (!text.isNullOrEmpty()) putExtra(Intent.EXTRA_TEXT, text)
            if (uris.isNotEmpty()) {
              if (uris.size == 1) putExtra(Intent.EXTRA_STREAM, uris.first())
              else putParcelableArrayListExtra(Intent.EXTRA_STREAM, uris)
              clipData = ClipData.newRawUri("UniClipboard", uris.first()).also { clip ->
                uris.drop(1).forEach { clip.addItem(ClipData.Item(it)) }
              }
              addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
          }
          onMainSync {
            val activity = appContext.currentActivity
              ?: throw rawCodeException("invalid_state", "no foreground activity")
            activity.startActivity(Intent.createChooser(intent, null))
          }
          promise.resolve(null)
        } catch (error: Exception) {
          staged.forEach { it.directory.deleteRecursively() }
          promise.reject(if (error is CodedException) error else moduleException(error))
        }
      }
    }

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
        val obj = JSONObject(eng.exportHistoryImagePreview(historyID))
        val file = File(obj.getString("path"))
        obj.remove("path")
        obj.put("uri", Uri.fromFile(file).toString())
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
        UnicEngineHost.applyReceivedToClipboard(appContextOrThrow(), receiptID)
      } catch (ce: CodedException) {
        throw ce
      } catch (e: NoSuchElementException) {
        throw rawCodeException("not_found", e.message ?: "received item not found")
      } catch (e: UnsupportedOperationException) {
        throw rawCodeException("unsupported", e.message ?: "received content cannot be applied to clipboard")
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("applyHistoryToClipboard") { historyID: String ->
      try {
        UnicEngineHost.applyHistoryToClipboard(appContextOrThrow(), historyID)
      } catch (ce: CodedException) {
        throw ce
      } catch (e: UnsupportedOperationException) {
        throw rawCodeException("unsupported", e.message ?: "clipboard content could not be applied")
      } catch (t: Throwable) {
        throw moduleException(t)
      }
    }.runOnQueue(engineScope)

    AsyncFunction("readSystemClipboardAndCommit") { ->
      try {
        UnicEngineHost.observeCurrentForegroundClipboard(appContextOrThrow(), explicit = true)
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
