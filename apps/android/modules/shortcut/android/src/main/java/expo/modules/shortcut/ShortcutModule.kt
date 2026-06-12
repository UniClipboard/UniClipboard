package expo.modules.shortcut

import android.content.ComponentName
import android.content.Intent
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.drawable.Icon
import android.os.Build
import androidx.core.content.res.ResourcesCompat
import expo.modules.kotlin.Promise
import expo.modules.kotlin.exception.CodedException
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class ShortcutModule : Module() {
    companion object {
        private const val DOWNLOAD_SHORTCUT_ID = "shortcut_download"
        private const val DOWNLOAD_LABEL = "下载剪贴板"
        private const val DOWNLOAD_DIRECTION = "download"
        private const val DOWNLOAD_ICON_RES = "ic_tile_download"
        private const val DOWNLOAD_BG_COLOR = "#007AFF"

        private const val UPLOAD_SHORTCUT_ID = "shortcut_upload"
        private const val UPLOAD_LABEL = "上传剪贴板"
        private const val UPLOAD_DIRECTION = "upload"
        private const val UPLOAD_ICON_RES = "ic_tile_upload"
        private const val UPLOAD_BG_COLOR = "#007AFF"

        private const val QUICK_ACTION_ACTIVITY = "com.jericx.syncclipboardmobile.quickaction.QuickActionActivity"

    }

    override fun definition() = ModuleDefinition {
        Name("ShortcutModule")

        AsyncFunction("requestPinDownloadShortcut") { promise: Promise ->
            requestPinShortcutInternal(DOWNLOAD_SHORTCUT_ID, DOWNLOAD_LABEL, DOWNLOAD_DIRECTION, DOWNLOAD_ICON_RES, DOWNLOAD_BG_COLOR, promise)
        }

        AsyncFunction("requestPinUploadShortcut") { promise: Promise ->
            requestPinShortcutInternal(UPLOAD_SHORTCUT_ID, UPLOAD_LABEL, UPLOAD_DIRECTION, UPLOAD_ICON_RES, UPLOAD_BG_COLOR, promise)
        }

        Function("setDynamicShortcuts") {
            try {
                val reactContext = appContext.reactContext ?: return@Function false
                val shortcutManager = reactContext.getSystemService(ShortcutManager::class.java)
                    ?: return@Function false

                val downloadShortcut = createShortcutInfo(reactContext, DOWNLOAD_SHORTCUT_ID, DOWNLOAD_LABEL, DOWNLOAD_DIRECTION, DOWNLOAD_ICON_RES, DOWNLOAD_BG_COLOR)
                val uploadShortcut = createShortcutInfo(reactContext, UPLOAD_SHORTCUT_ID, UPLOAD_LABEL, UPLOAD_DIRECTION, UPLOAD_ICON_RES, UPLOAD_BG_COLOR)

                shortcutManager.dynamicShortcuts = listOf(uploadShortcut, downloadShortcut)
                true
            } catch (e: Exception) {
                false
            }
        }
    }

    private fun requestPinShortcutInternal(
        shortcutId: String,
        label: String,
        direction: String,
        iconResName: String,
        bgColorHex: String,
        promise: Promise
    ) {
        try {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
                promise.reject(UnsupportedError("Pinned shortcuts require Android 8.0 (API 26) or higher"))
                return
            }

            val reactContext = appContext.reactContext ?: run {
                promise.reject(ReactContextMissingError())
                return
            }

            val shortcutManager = reactContext.getSystemService(ShortcutManager::class.java)
            if (shortcutManager == null || !shortcutManager.isRequestPinShortcutSupported) {
                promise.reject(UnsupportedError("The current launcher does not support pinned shortcuts"))
                return
            }

            val shortcutInfo = createShortcutInfo(reactContext, shortcutId, label, direction, iconResName, bgColorHex)
            shortcutManager.requestPinShortcut(shortcutInfo, null)
            promise.resolve(true)
        } catch (e: Exception) {
            promise.reject(CodedException("${e.javaClass.simpleName}: ${e.message}"))
        }
    }

    private fun createShortcutInfo(
        reactContext: android.content.Context,
        shortcutId: String,
        label: String,
        direction: String,
        iconResName: String,
        bgColorHex: String
    ): ShortcutInfo {
        val launchIntent = Intent(Intent.ACTION_MAIN).apply {
            component = ComponentName(reactContext, QUICK_ACTION_ACTIVITY)
            putExtra("direction", direction)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP)
            addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }

        val icon = buildIcon(reactContext, iconResName, bgColorHex)

        return ShortcutInfo.Builder(reactContext, shortcutId)
            .setShortLabel(label)
            .setLongLabel(label)
            .setIcon(icon)
            .setIntent(launchIntent)
            .build()
    }

    private fun buildIcon(
        reactContext: android.content.Context,
        iconResName: String,
        bgColorHex: String
    ): Icon {
        val density = reactContext.resources.displayMetrics.density
        val size = (108 * density + 0.5f).toInt()

        val bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)

        val bgPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            color = try {
                Color.parseColor(bgColorHex)
            } catch (_: IllegalArgumentException) {
                Color.parseColor("#007AFF")
            }
        }
        val radius = size / 2f
        canvas.drawCircle(radius, radius, radius, bgPaint)

        val iconResId = reactContext.resources.getIdentifier(
            iconResName, "drawable", reactContext.packageName
        )
        if (iconResId != 0) {
            val drawable = ResourcesCompat.getDrawable(
                reactContext.resources, iconResId, null
            )
            drawable?.let {
                val pad = (size * 0.33f).toInt()
                it.setBounds(pad, pad, size - pad, size - pad)
                it.setTint(Color.WHITE)
                it.draw(canvas)
            }
        }

        return Icon.createWithAdaptiveBitmap(bitmap)
    }

    private class UnsupportedError(message: String) : CodedException(message)
    private class ReactContextMissingError : CodedException("React context is not available")
}
