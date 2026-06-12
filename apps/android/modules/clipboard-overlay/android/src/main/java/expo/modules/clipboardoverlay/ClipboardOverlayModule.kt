package expo.modules.clipboardoverlay

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.PixelFormat
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Base64
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import expo.modules.kotlin.Promise
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream

class ClipboardOverlayModule : Module() {

    companion object {
        private const val RETRY_DELAY_MS = 10L  // Short interval like AutoJs6 to minimize focus steal time
        private const val DEFAULT_MAX_RETRIES = 5
    }

    private var debugMode = false
    private var maxRetries = DEFAULT_MAX_RETRIES

    // Persistent overlay state
    private var persistentView: View? = null
    private var persistentWindowManager: WindowManager? = null
    private var persistentParams: WindowManager.LayoutParams? = null
    private val mainHandler = Handler(Looper.getMainLooper())

    override fun definition() = ModuleDefinition {
        Name("ClipboardOverlayModule")

        Function("setDebugMode") { enabled: Boolean ->
            debugMode = enabled
            // Update persistent overlay appearance if showing
            mainHandler.post { updatePersistentOverlayAppearance() }
            true
        }

        Function("setMaxRetries") { retries: Int ->
            maxRetries = retries.coerceIn(1, 50)
            true
        }

        Function("hasOverlayPermission") {
            val context = appContext.reactContext ?: return@Function false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                Settings.canDrawOverlays(context)
            } else {
                true
            }
        }

        Function("requestOverlayPermission") {
            val context = appContext.reactContext ?: return@Function false
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                val intent = Intent(
                    Settings.ACTION_MANAGE_OVERLAY_PERMISSION,
                    Uri.parse("package:${context.packageName}")
                ).apply {
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                }
                context.startActivity(intent)
            }
            true
        }

        Function("isOverlayShowing") {
            persistentView != null
        }

        AsyncFunction("showOverlayWindow") { promise: Promise ->
            val context = appContext.reactContext
            if (context == null) {
                promise.reject("ERR_NO_CONTEXT", "React context is null", null)
                return@AsyncFunction
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M && !Settings.canDrawOverlays(context)) {
                promise.reject("ERR_NO_PERMISSION", "Overlay permission not granted", null)
                return@AsyncFunction
            }
            mainHandler.post {
                try {
                    if (persistentView != null) {
                        // Already showing
                        promise.resolve(true)
                        return@post
                    }
                    val wm = context.getSystemService(Context.WINDOW_SERVICE) as WindowManager
                    val view = View(context).apply {
                        isFocusable = true
                        isFocusableInTouchMode = true
                    }

                    val layoutType = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                        WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
                    } else {
                        @Suppress("DEPRECATION")
                        WindowManager.LayoutParams.TYPE_PHONE
                    }

                    val overlaySize = if (debugMode) 200 else 1

                    if (debugMode) {
                        view.setBackgroundColor(0xFFFF0000.toInt())
                    }

                    val params = WindowManager.LayoutParams(
                        overlaySize, overlaySize,
                        layoutType,
                        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                            WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
                        PixelFormat.TRANSLUCENT
                    ).apply {
                        alpha = if (debugMode) 0.7f else 0f
                        gravity = Gravity.START or Gravity.TOP
                        x = 0
                        y = 0
                    }

                    wm.addView(view, params)

                    persistentView = view
                    persistentWindowManager = wm
                    persistentParams = params

                    promise.resolve(true)
                } catch (e: Exception) {
                    promise.reject("ERR_OVERLAY_SHOW", e.message ?: "Unknown error", e)
                }
            }
        }

        AsyncFunction("hideOverlayWindow") { promise: Promise ->
            mainHandler.post {
                try {
                    removePersistentOverlay()
                    promise.resolve(true)
                } catch (e: Exception) {
                    promise.reject("ERR_OVERLAY_HIDE", e.message ?: "Unknown error", e)
                }
            }
        }

        AsyncFunction("getStringViaOverlay") { promise: Promise ->
            withOverlayClipboard("getStringViaOverlay", promise) { context, clip ->
                if (clip != null && clip.itemCount > 0) {
                    val text = clip.getItemAt(0).coerceToText(context)?.toString() ?: ""
                    promise.resolve(text)
                } else {
                    promise.resolve("")
                }
            }
        }

        AsyncFunction("hasStringViaOverlay") { promise: Promise ->
            withOverlayClipboard("hasStringViaOverlay", promise) { _, clip ->
                if (clip != null && clip.itemCount > 0) {
                    val desc = clip.description
                    val hasText = desc.hasMimeType("text/*") ||
                        clip.getItemAt(0).text != null
                    promise.resolve(hasText)
                } else {
                    promise.resolve(false)
                }
            }
        }

        AsyncFunction("hasImageViaOverlay") { promise: Promise ->
            withOverlayClipboard("hasImageViaOverlay", promise) { _, clip ->
                if (clip != null && clip.itemCount > 0) {
                    val desc = clip.description
                    val hasImage = desc.hasMimeType("image/*")
                    promise.resolve(hasImage)
                } else {
                    promise.resolve(false)
                }
            }
        }

        AsyncFunction("getImageViaOverlay") { promise: Promise ->
            withOverlayClipboard("getImageViaOverlay", promise) { context, clip ->
                if (clip == null || clip.itemCount == 0) {
                    promise.resolve(null)
                    return@withOverlayClipboard
                }

                val item = clip.getItemAt(0)
                val uri = item.uri
                if (uri == null) {
                    promise.resolve(null)
                    return@withOverlayClipboard
                }

                try {
                    val mimeType = context.contentResolver.getType(uri)
                    if (mimeType == null || !mimeType.startsWith("image/")) {
                        promise.resolve(null)
                        return@withOverlayClipboard
                    }

                    val inputStream = context.contentResolver.openInputStream(uri)
                    if (inputStream == null) {
                        promise.resolve(null)
                        return@withOverlayClipboard
                    }

                    val bitmap = BitmapFactory.decodeStream(inputStream)
                    inputStream.close()

                    if (bitmap == null) {
                        promise.resolve(null)
                        return@withOverlayClipboard
                    }

                    val width = bitmap.width
                    val height = bitmap.height
                    val baos = ByteArrayOutputStream()
                    bitmap.compress(Bitmap.CompressFormat.PNG, 100, baos)
                    val base64Data = Base64.encodeToString(baos.toByteArray(), Base64.NO_WRAP)
                    bitmap.recycle()

                    val result = mapOf(
                        "data" to base64Data,
                        "size" to mapOf(
                            "width" to width,
                            "height" to height
                        )
                    )
                    promise.resolve(result)
                } catch (e: Exception) {
                    promise.resolve(null)
                }
            }
        }

        AsyncFunction("saveImageToFileViaOverlay") { destDirPath: String, promise: Promise ->
            withOverlayClipboard("saveImageToFileViaOverlay", promise) { context, clip ->
                if (clip == null || clip.itemCount == 0) {
                    promise.resolve(null)
                    return@withOverlayClipboard
                }

                val item = clip.getItemAt(0)
                val uri = item.uri
                if (uri == null) {
                    promise.resolve(null)
                    return@withOverlayClipboard
                }

                try {
                    val mimeType = context.contentResolver.getType(uri)
                    if (mimeType == null || !mimeType.startsWith("image/")) {
                        promise.resolve(null)
                        return@withOverlayClipboard
                    }

                    val inputStream = context.contentResolver.openInputStream(uri)
                    if (inputStream == null) {
                        promise.resolve(null)
                        return@withOverlayClipboard
                    }

                    // 根据 mimeType 确定扩展名
                    val ext = when {
                        mimeType.contains("png") -> "png"
                        mimeType.contains("jpeg") || mimeType.contains("jpg") -> "jpg"
                        mimeType.contains("gif") -> "gif"
                        mimeType.contains("webp") -> "webp"
                        mimeType.contains("bmp") -> "bmp"
                        else -> "png"
                    }
                    val path = if (destDirPath.startsWith("file://", ignoreCase = true)) {
                        Uri.parse(destDirPath).path ?: destDirPath.removePrefix("file://")
                    } else {
                        destDirPath
                    }
                    val dir = File(path)
                    dir.mkdirs()
                    val fileName = "tmp_${System.currentTimeMillis()}_${(Math.random() * 100000).toInt()}.$ext"
                    val file = File(dir, fileName)
                    FileOutputStream(file).use { fos ->
                        inputStream.copyTo(fos, bufferSize = 8192)
                    }
                    inputStream.close()

                    // 仅读取尺寸（不分配像素内存）
                    val opts = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                    BitmapFactory.decodeFile(file.absolutePath, opts)

                    val result = mapOf(
                        "width" to (opts.outWidth),
                        "height" to (opts.outHeight),
                        "filePath" to "file://" + file.absolutePath,
                        "mimeType" to mimeType
                    )
                    promise.resolve(result)
                } catch (e: Exception) {
                    promise.resolve(null)
                }
            }
        }
    }

    /**
     * Update the persistent overlay appearance based on current debug mode.
     * Must be called on main thread.
     */
    private fun updatePersistentOverlayAppearance() {
        val view = persistentView ?: return
        val wm = persistentWindowManager ?: return
        val params = persistentParams ?: return

        val overlaySize = if (debugMode) 200 else 1
        params.width = overlaySize
        params.height = overlaySize
        params.alpha = if (debugMode) 0.7f else 0f

        if (debugMode) {
            view.setBackgroundColor(0xFFFF0000.toInt())
        } else {
            view.setBackgroundColor(0x00000000)
        }

        try {
            wm.updateViewLayout(view, params)
        } catch (_: Exception) {}
    }

    /**
     * Remove the persistent overlay window.
     * Must be called on main thread.
     */
    private fun removePersistentOverlay() {
        val view = persistentView ?: return
        val wm = persistentWindowManager ?: return
        try {
            wm.removeView(view)
        } catch (_: Exception) {}
        persistentView = null
        persistentWindowManager = null
        persistentParams = null
    }

    /**
     * Reads the primary clip with fast retry logic.
     * Uses short intervals (10ms like AutoJs6) to minimize time the window has focus.
     */
    private fun readClipWithRetry(
        context: Context,
        handler: Handler,
        attempt: Int,
        callback: (ClipData?) -> Unit
    ) {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = cm.primaryClip
        if (clip != null || attempt >= maxRetries) {
            callback(clip)
        } else {
            handler.postDelayed({
                readClipWithRetry(context, handler, attempt + 1, callback)
            }, RETRY_DELAY_MS)
        }
    }

    /**
     * Reads the clipboard using the overlay window focus trick.
     *
     * If a persistent overlay is showing, reuses it by toggling focus flags.
     * Otherwise, falls back to creating a temporary overlay (legacy behavior).
     *
     * Persistent overlay flow:
     * 1. Remove FLAG_NOT_FOCUSABLE from existing overlay (gains window focus)
     * 2. Read clipboard with retry
     * 3. Re-add FLAG_NOT_FOCUSABLE (releases focus back to foreground app)
     *
     * Legacy flow (no persistent overlay):
     * 1. Create temporary 1px overlay with FLAG_NOT_FOCUSABLE
     * 2. Remove FLAG_NOT_FOCUSABLE
     * 3. Read clipboard
     * 4. Destroy temporary overlay
     */
    private fun withOverlayClipboard(
        tag: String,
        promise: Promise,
        action: (Context, ClipData?) -> Unit
    ) {
        val context = appContext.reactContext
        if (context == null) {
            promise.reject("ERR_NO_CONTEXT", "React context is null", null)
            return
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M && !Settings.canDrawOverlays(context)) {
            promise.reject("ERR_NO_PERMISSION", "Overlay permission not granted", null)
            return
        }

        mainHandler.post {
            val view = persistentView
            val wm = persistentWindowManager
            val params = persistentParams

            if (view != null && wm != null && params != null) {
                // Persistent overlay path: toggle focus on existing window
                try {
                    // Step 1: Remove FLAG_NOT_FOCUSABLE to gain window focus
                    params.flags = params.flags and WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE.inv()
                    wm.updateViewLayout(view, params)
                    view.requestLayout()

                    // Step 2: Read clipboard with retry
                    readClipWithRetry(context, mainHandler, 0) { clip ->
                        // Step 3: Re-add FLAG_NOT_FOCUSABLE to release focus
                        try {
                            params.flags = params.flags or WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                            wm.updateViewLayout(view, params)
                        } catch (_: Exception) {}

                        try {
                            action(context, clip)
                        } catch (e: Exception) {
                            promise.reject("ERR_OVERLAY_$tag", e.message ?: "Unknown error", e)
                        }
                    }
                } catch (e: Exception) {
                    // Restore FLAG_NOT_FOCUSABLE on error
                    try {
                        params.flags = params.flags or WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                        wm.updateViewLayout(view, params)
                    } catch (_: Exception) {}
                    promise.reject("ERR_OVERLAY_$tag", e.message ?: "Unknown error", e)
                }
            } else {
                // Legacy path: create temporary overlay
                var overlayView: View? = null
                var tempWm: WindowManager? = null
                try {
                    tempWm = context.getSystemService(Context.WINDOW_SERVICE) as WindowManager
                    overlayView = View(context).apply {
                        isFocusable = true
                        isFocusableInTouchMode = true
                    }

                    val layoutType = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                        WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY
                    } else {
                        @Suppress("DEPRECATION")
                        WindowManager.LayoutParams.TYPE_PHONE
                    }

                    val overlaySize = if (debugMode) 200 else 1
                    if (debugMode) {
                        overlayView.setBackgroundColor(0xFFFF0000.toInt())
                    }

                    val tempParams = WindowManager.LayoutParams(
                        overlaySize, overlaySize,
                        layoutType,
                        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or
                            WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
                        PixelFormat.TRANSLUCENT
                    ).apply {
                        alpha = if (debugMode) 0.7f else 0f
                        gravity = Gravity.START or Gravity.TOP
                        x = 0
                        y = 0
                    }

                    tempWm.addView(overlayView, tempParams)

                    val finalWm = tempWm
                    val finalView = overlayView

                    finalView.post {
                        tempParams.flags = tempParams.flags and WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE.inv()
                        finalWm.updateViewLayout(finalView, tempParams)
                        finalView.requestLayout()

                        readClipWithRetry(context, mainHandler, 0) { clip ->
                            try {
                                action(context, clip)
                            } catch (e: Exception) {
                                promise.reject("ERR_OVERLAY_$tag", e.message ?: "Unknown error", e)
                            } finally {
                                try {
                                    finalWm.removeView(finalView)
                                } catch (_: Exception) {}
                            }
                        }
                    }
                } catch (e: Exception) {
                    try {
                        if (overlayView != null && tempWm != null) {
                            tempWm.removeView(overlayView)
                        }
                    } catch (_: Exception) {}
                    promise.reject("ERR_OVERLAY_$tag", e.message ?: "Unknown error", e)
                }
            }
        }
    }
}
