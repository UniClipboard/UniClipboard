package app.uniclipboard.engine

import android.content.Context
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import java.io.File
import java.io.IOException
import java.util.UUID

private const val MAX_BACKUP_BYTES = 32L * 1024 * 1024 * 1024
private const val MAX_BACKUP_ENTRIES = 250_000
private const val MAX_BACKUP_DEPTH = 64

internal class InvalidMigrationTree : IOException("The selected directory is not a v1 backup")
internal class MigrationTreeTooLarge : IOException("The v1 backup exceeds the staging limit")

internal fun stageMigrationTree(context: Context, uri: Uri, stagingRoot: File): File {
  val root = DocumentFile.fromTreeUri(context, uri)?.takeIf { it.isDirectory }
    ?: throw InvalidMigrationTree()
  val vault = root.findFile("vault")?.takeIf { it.isDirectory } ?: throw InvalidMigrationTree()
  if (vault.findFile("keyslot.json")?.isFile != true) throw InvalidMigrationTree()

  if (!stagingRoot.isDirectory && !stagingRoot.mkdirs()) throw IOException("Cannot create backup staging")
  val staging = File(stagingRoot, "backup-${UUID.randomUUID()}")
  if (!staging.mkdir()) throw IOException("Cannot create backup staging")
  try {
    val resolver = context.contentResolver
    val visited = mutableSetOf(root.uri)
    var entries = 0
    var remaining = minOf(MAX_BACKUP_BYTES, staging.usableSpace)
    val buffer = ByteArray(64 * 1024)

    fun copy(directory: DocumentFile, destination: File, depth: Int) {
      if (depth >= MAX_BACKUP_DEPTH) throw MigrationTreeTooLarge()
      for (child in directory.listFiles()) {
        if (++entries > MAX_BACKUP_ENTRIES) throw MigrationTreeTooLarge()
        val name = child.name ?: throw InvalidMigrationTree()
        if (name.isBlank() || name == "." || name == ".." || name.any { it in "/\\\u0000" }) {
          throw InvalidMigrationTree()
        }
        val target = File(destination, name)
        if (target.canonicalFile.parentFile != destination.canonicalFile || target.exists()) {
          throw InvalidMigrationTree()
        }
        // SAF exposes documents rather than filesystem links. Only create new
        // plain files and directories, and reject repeated provider IDs.
        if (!visited.add(child.uri)) throw InvalidMigrationTree()
        when {
          child.isDirectory -> {
            if (!target.mkdir()) throw IOException("Cannot stage a backup directory")
            copy(child, target, depth + 1)
          }
          child.isFile && !child.isVirtual -> {
            if (child.length() > remaining) throw MigrationTreeTooLarge()
            val input = resolver.openInputStream(child.uri) ?: throw IOException("Cannot read a backup document")
            input.use { source ->
              target.outputStream().use { output ->
                var count = source.read(buffer)
                while (count != -1) {
                  if (count > remaining) throw MigrationTreeTooLarge()
                  output.write(buffer, 0, count)
                  remaining -= count
                  count = source.read(buffer)
                }
              }
            }
          }
        }
      }
    }

    copy(root, staging, 0)
    return staging
  } catch (error: Exception) {
    staging.deleteRecursively()
    throw error
  }
}
