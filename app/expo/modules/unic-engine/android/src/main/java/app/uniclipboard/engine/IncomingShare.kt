package app.uniclipboard.engine

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.util.UUID

private const val MAX_SHARE_ITEMS = 64
private const val MAX_SHARE_BYTES = 256L * 1024 * 1024
private const val MAX_SHARE_TEXT = 256 * 1024

internal class IncomingShare(private val context: Context) {
  private val root = File(context.cacheDir, "unic-incoming-share")
  private val resolver = context.contentResolver

  @Suppress("DEPRECATION")
  fun stage(intent: Intent): Boolean {
    if (intent.action != Intent.ACTION_SEND && intent.action != Intent.ACTION_SEND_MULTIPLE) return false
    val streams = linkedSetOf<Uri>()
    if (intent.action == Intent.ACTION_SEND_MULTIPLE) {
      intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.let(streams::addAll)
    } else {
      intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)?.let(streams::add)
    }
    val clip = intent.clipData
    if (clip != null) {
      for (i in 0 until minOf(clip.itemCount, MAX_SHARE_ITEMS)) {
        clip.getItemAt(i).uri?.let(streams::add)
      }
    }
    val ready = entries()
    var bytesLeft = MAX_SHARE_BYTES - ready.sumOf { File(it, "payload").length() + File(it, "entry.json").length() }
    var itemsLeft = MAX_SHARE_ITEMS - ready.size
    var staged = false
    val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
    if (!text.isNullOrBlank() && itemsLeft > 0 && text.length <= MAX_SHARE_TEXT && text.length * 6L <= bytesLeft) {
      val metadata = JSONObject().put("kind", "text").put("text", text).toString()
      try {
        publish { dir -> File(dir, "entry.json").writeText(metadata) }
        bytesLeft -= metadata.toByteArray().size
        itemsLeft--
        staged = true
      } catch (_: IOException) {
        return false
      }
    }
    val buffer = ByteArray(64 * 1024)
    for (uri in streams.take(itemsLeft)) {
      if (uri.scheme != "content") continue
      try {
        var newBytes = 0L
        publish { dir ->
          val mime = resolver.getType(uri) ?: intent.type?.takeUnless { it.contains('*') } ?: "application/octet-stream"
          val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) it.getString(0) else null
          }?.substringAfterLast('/')?.substringAfterLast('\\')?.take(255)?.takeIf { it.isNotBlank() && it != "." && it != ".." }
            ?: "shared-file"
          val input = resolver.openInputStream(uri) ?: throw IOException("Cannot read shared file")
          input.use { source ->
            File(dir, "payload").outputStream().use { output ->
              var n = source.read(buffer)
              while (n != -1) {
                newBytes += n
                if (newBytes > bytesLeft) throw IOException("Incoming share exceeds staging capacity")
                output.write(buffer, 0, n)
                n = source.read(buffer)
              }
            }
          }
          val fields = JSONObject().put("kind", "file").put("name", name).put("mime", mime)
          File(dir, "entry.json").writeText(fields.toString())
        }
        bytesLeft -= newBytes
        staged = true
      } catch (_: IOException) {
        // A denied, unavailable or oversized item has no published entry.
      } catch (_: SecurityException) {
        // Only the access granted by the sending activity is available.
      }
    }
    return staged
  }

  private fun publish(write: (File) -> Unit) {
    if (!root.isDirectory && !root.mkdirs()) throw IOException("Cannot create incoming share inbox")
    val id = UUID.randomUUID().toString()
    val staging = File(root, "staging-$id")
    if (!staging.mkdir()) throw IOException("Cannot stage incoming share")
    try {
      write(staging)
      if (!staging.renameTo(File(root, id))) throw IOException("Cannot publish incoming share")
    } finally {
      staging.deleteRecursively()
    }
  }

  private fun entries(): List<File> = root.listFiles()
    ?.filter { it.isDirectory && isShareID(it.name) }
    ?.sortedWith(compareBy<File> { it.lastModified() }.thenBy { it.name })
    .orEmpty()

  fun list(): String = JSONArray(entries().map { it.name }).toString()

  fun item(id: String): File? = if (isShareID(id)) File(root, id).takeIf { it.isDirectory } else null

  fun remove(id: String) {
    val path = item(id) ?: return
    val trash = File(root, "staging-remove-$id")
    if (!path.renameTo(trash)) throw IOException("Cannot remove consumed share")
    trash.deleteRecursively()
  }

  private fun isShareID(id: String): Boolean = runCatching { UUID.fromString(id).toString() == id }.getOrDefault(false)
}
