package app.uniclipboard.engine

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import org.json.JSONArray
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileInputStream
import java.io.FileOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

internal class UpdateDownloadCancelledException : IOException("update download cancelled")

internal class AppUpdateManager {
  companion object {
    private const val DEFAULT_BASE_URL = "https://release.uniclipboard.app"
    private const val CONNECT_TIMEOUT_MS = 15_000
    private const val READ_TIMEOUT_MS = 30_000
    private const val MAX_MANIFEST_BYTES = 1 * 1024 * 1024
    private const val MAX_APK_BYTES = 512L * 1024L * 1024L
    private const val MAX_REDIRECTS = 5
    private const val BUFFER_SIZE = 32 * 1024
    private val SAFE_APK_FILENAME = Regex("^[A-Za-z0-9._-]+\\.apk$")
    private val SAFE_VERSION_KEY = Regex("^[A-Za-z0-9._-]+$")
  }

  private val cancelFlag = AtomicBoolean(false)
  private val activeConnection = AtomicReference<HttpURLConnection?>(null)

  private fun normalizeBaseUrl(overrideUrl: String?): String {
    val trimmed = overrideUrl?.trim()?.trimEnd('/')
    if (trimmed.isNullOrEmpty()) return DEFAULT_BASE_URL
    val parsed = URL(trimmed)
    val allowedHost = parsed.host == "127.0.0.1" || parsed.host == "localhost" || parsed.host == "10.0.2.2"
    if (parsed.protocol != "https" && !(parsed.protocol == "http" && allowedHost)) {
      throw IllegalArgumentException("unsupported update base URL")
    }
    return trimmed
  }

  private fun isAllowedDownloadUrl(target: URL, allowedHttpOrigin: String?): Boolean {
    if (target.protocol == "https") return true
    if (target.protocol == "http" && allowedHttpOrigin != null) {
      val origin = URL(allowedHttpOrigin)
      return origin.protocol == "http" && origin.host == target.host && origin.port == target.port
    }
    return false
  }

  private fun openWithRedirects(
    initialUrl: String,
    acceptHeader: String?,
    allowedHttpOrigin: String?,
  ): HttpURLConnection {
    var currentUrl = initialUrl
    var redirects = 0
    while (true) {
      val parsed = URL(currentUrl)
      if (!isAllowedDownloadUrl(parsed, allowedHttpOrigin)) {
        throw IOException("refused insecure update URL")
      }
      val conn = (parsed.openConnection() as HttpURLConnection).apply {
        instanceFollowRedirects = false
        connectTimeout = CONNECT_TIMEOUT_MS
        readTimeout = READ_TIMEOUT_MS
        requestMethod = "GET"
        setRequestProperty("User-Agent", "UniClipboard-Android")
        if (acceptHeader != null) {
          setRequestProperty("Accept", acceptHeader)
        }
      }
      activeConnection.set(conn)
      if (cancelFlag.get()) {
        conn.disconnect()
        throw UpdateDownloadCancelledException()
      }
      val status = conn.responseCode
      if (status in listOf(
          HttpURLConnection.HTTP_MOVED_PERM,
          HttpURLConnection.HTTP_MOVED_TEMP,
          HttpURLConnection.HTTP_SEE_OTHER,
          307,
          308,
        )
      ) {
        val location = conn.getHeaderField("Location")
        conn.disconnect()
        if (location.isNullOrBlank()) {
          throw IOException("redirect missing Location header")
        }
        redirects += 1
        if (redirects > MAX_REDIRECTS) {
          throw IOException("too many redirects")
        }
        currentUrl = URL(parsed, location).toString()
        continue
      }
      return conn
    }
  }

  fun checkUpdate(channel: String, baseUrlOverride: String?): String {
    if (channel != "alpha" && channel != "stable") {
      throw IllegalArgumentException("unsupported update channel: $channel")
    }
    val base = normalizeBaseUrl(baseUrlOverride)
    val allowedHttp = if (base.startsWith("http://")) base else null
    val endpoint = "$base/v1/android/$channel.json"
    val conn = openWithRedirects(endpoint, "application/json", allowedHttp)
    try {
      val status = conn.responseCode
      val abis = JSONArray()
      Build.SUPPORTED_ABIS.forEach { abis.put(it) }
      if (status == HttpURLConnection.HTTP_NOT_FOUND) {
        return JSONObject()
          .put("found", false)
          .put("channel", channel)
          .put("supported_abis", abis)
          .toString()
      }
      if (status !in 200..299) {
        throw IOException("update feed returned HTTP $status")
      }
      val out = ByteArrayOutputStream()
      conn.inputStream.use { input ->
        val buf = ByteArray(8192)
        var total = 0
        while (true) {
          val n = input.read(buf)
          if (n <= 0) break
          total += n
          if (total > MAX_MANIFEST_BYTES) {
            throw IOException("update manifest exceeds size limit")
          }
          out.write(buf, 0, n)
        }
      }
      val manifest = JSONObject(out.toString(Charsets.UTF_8.name()))
      return JSONObject()
        .put("found", true)
        .put("channel", channel)
        .put("manifest", manifest)
        .put("supported_abis", abis)
        .toString()
    } finally {
      activeConnection.compareAndSet(conn, null)
      conn.disconnect()
    }
  }

  fun cancelDownload() {
    cancelFlag.set(true)
    runCatching { activeConnection.getAndSet(null)?.disconnect() }
  }

  private fun sha256Hex(file: File): String {
    val digest = MessageDigest.getInstance("SHA-256")
    FileInputStream(file).use { input ->
      val buf = ByteArray(BUFFER_SIZE)
      while (true) {
        val n = input.read(buf)
        if (n <= 0) break
        digest.update(buf, 0, n)
      }
    }
    return digest.digest().joinToString("") { "%02x".format(it) }
  }

  private fun updatesRoot(context: Context): File {
    return File(File(context.cacheDir, "unic-cache"), "updates")
  }

  fun cleanCache(context: Context, keepVersionKey: String?) {
    val root = updatesRoot(context)
    if (!root.exists() || !root.isDirectory) return
    val keep = keepVersionKey?.trim()?.takeIf { it.isNotEmpty() && SAFE_VERSION_KEY.matches(it) }
    root.listFiles()?.forEach { child ->
      if (keep != null && child.name == keep) return@forEach
      runCatching { child.deleteRecursively() }
    }
  }

  fun downloadAndInstallApk(
    context: Context,
    activity: Activity?,
    url: String,
    filename: String,
    expectedSha256: String?,
    versionKey: String,
    baseUrlOverride: String?,
    onProgress: (progress: Double, bytesReceived: Long, totalBytes: Long) -> Unit,
  ): String {
    if (!SAFE_APK_FILENAME.matches(filename)) {
      throw IllegalArgumentException("unsafe APK filename")
    }
    if (!SAFE_VERSION_KEY.matches(versionKey)) {
      throw IllegalArgumentException("unsafe update version key")
    }
    val expectedHash = expectedSha256?.trim()?.lowercase()?.takeIf { it.isNotEmpty() }
    if (expectedHash != null && !Regex("^[a-f0-9]{64}$").matches(expectedHash)) {
      throw IllegalArgumentException("invalid expected SHA-256 digest")
    }

    val base = normalizeBaseUrl(baseUrlOverride)
    val allowedHttp = if (base.startsWith("http://")) base else null

    cancelFlag.set(false)

    val versionDir = File(updatesRoot(context), versionKey)
    if (!versionDir.exists() && !versionDir.mkdirs()) {
      throw IOException("failed to create update cache directory")
    }
    val targetFile = File(versionDir, filename)

    // Reuse cached verified APK when available.
    var cacheHit = false
    if (targetFile.isFile && targetFile.length() > 0) {
      if (expectedHash != null) {
        val existingHash = runCatching { sha256Hex(targetFile) }.getOrNull()
        if (existingHash == expectedHash) {
          cacheHit = true
        } else {
          targetFile.delete()
        }
      } else {
        targetFile.delete()
      }
    }

    if (!cacheHit) {
      val partFile = File(versionDir, "$filename.part")
      if (partFile.exists()) partFile.delete()

      val conn = openWithRedirects(url, "application/vnd.android.package-archive, */*", allowedHttp)
      try {
        val status = conn.responseCode
        if (status !in 200..299) {
          throw IOException("APK download returned HTTP $status")
        }
        val totalBytes = conn.contentLengthLong
        if (totalBytes > MAX_APK_BYTES) {
          throw IOException("APK exceeds maximum allowed size")
        }
        val digest = MessageDigest.getInstance("SHA-256")
        var bytesReceived = 0L
        var lastEmitAt = 0L
        var lastPercent = -1

        conn.inputStream.use { input ->
          FileOutputStream(partFile).use { output ->
            val buf = ByteArray(BUFFER_SIZE)
            while (true) {
              if (cancelFlag.get()) {
                throw UpdateDownloadCancelledException()
              }
              val n = input.read(buf)
              if (n <= 0) break
              bytesReceived += n
              if (bytesReceived > MAX_APK_BYTES) {
                throw IOException("APK exceeds maximum allowed size")
              }
              output.write(buf, 0, n)
              digest.update(buf, 0, n)

              val now = System.currentTimeMillis()
              val progress = if (totalBytes > 0) {
                (bytesReceived.toDouble() / totalBytes.toDouble()).coerceIn(0.0, 1.0)
              } else {
                0.0
              }
              val pct = (progress * 100.0).toInt()
              if (pct != lastPercent && (now - lastEmitAt >= 120 || pct >= 100)) {
                lastEmitAt = now
                lastPercent = pct
                onProgress(progress, bytesReceived, totalBytes)
              }
            }
            output.flush()
          }
        }

        if (cancelFlag.get()) {
          throw UpdateDownloadCancelledException()
        }

        val actualHash = digest.digest().joinToString("") { "%02x".format(it) }
        if (expectedHash != null && actualHash != expectedHash) {
          partFile.delete()
          throw IOException("APK SHA-256 mismatch")
        }
        if (targetFile.exists()) targetFile.delete()
        if (!partFile.renameTo(targetFile)) {
          partFile.copyTo(targetFile, overwrite = true)
          partFile.delete()
        }
        onProgress(1.0, bytesReceived, if (totalBytes > 0) totalBytes else bytesReceived)
      } catch (t: Throwable) {
        partFile.delete()
        if (cancelFlag.get()) {
          throw UpdateDownloadCancelledException()
        }
        throw t
      } finally {
        activeConnection.compareAndSet(conn, null)
        conn.disconnect()
      }
    }

    val installStatus = launchApkInstaller(context, activity, targetFile)
    return JSONObject()
      .put("status", installStatus)
      .put("cached", cacheHit)
      .put("path", targetFile.canonicalPath)
      .toString()
  }

  private fun launchApkInstaller(context: Context, activity: Activity?, apkFile: File): String {
    val host: Context = activity ?: context
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !context.packageManager.canRequestPackageInstalls()) {
      val settingsIntent = Intent(
        Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
        Uri.parse("package:${context.packageName}"),
      ).apply {
        if (activity == null) addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      }
      host.startActivity(settingsIntent)
      return "needs_install_permission"
    }

    val authority = "${context.packageName}.unic.fileprovider"
    val contentUri = FileProvider.getUriForFile(context, authority, apkFile)
    val installIntent = Intent(Intent.ACTION_VIEW).apply {
      setDataAndType(contentUri, "application/vnd.android.package-archive")
      addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      if (activity == null) addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    host.startActivity(installIntent)
    return "installer_launched"
  }
}
