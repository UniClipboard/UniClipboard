package expo.modules.shizukuclipboard

import android.content.ComponentName
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Binder
import android.os.IBinder
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import expo.modules.kotlin.Promise
import expo.modules.nativeutil.NativeLogger
import rikka.shizuku.Shizuku

/**
 * ShizukuClipboardModule
 *
 * 通过 Shizuku UserService 在后台读取剪贴板内容，绕过 Android 10+ 前台限制。
 * UserService 运行在 Shizuku 的进程（UID 2000/shell）中，
 * 没有隐藏 API 限制，可以自由访问 IClipboard 的方法。
 */
class ShizukuClipboardModule : Module() {

    companion object {
        private const val TAG = "ShizukuClipboard"
        private const val REQUEST_CODE_PERMISSION = 10086
    }

    private var permissionGranted = false
    private var clipboardService: IClipboardUserService? = null
    private var serviceConnected = false
    private var isBinding = false

    // 用于 linkToDeath：UserService 进程监听此 token，当主进程死亡时自动退出
    private val callerToken = Binder()

    private val userServiceArgs by lazy {
        Shizuku.UserServiceArgs(
            ComponentName(
                appContext.reactContext?.packageName ?: "com.jericx.syncclipboardmobile",
                ClipboardUserService::class.java.name
            )
        )
            .daemon(false)
            .processNameSuffix("clipboard")
            .debuggable(true)
            .version(1)
    }

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            NativeLogger.i(TAG, "UserService connected: ${name?.className}")
            isBinding = false
            if (service != null && service.pingBinder()) {
                clipboardService = IClipboardUserService.Stub.asInterface(service)
                serviceConnected = true
                NativeLogger.i(TAG, "UserService bound successfully")
                // 传入本进程 token，使 UserService 在主进程死亡时能自动退出
                try {
                    clipboardService?.init(callerToken)
                } catch (e: Exception) {
                    NativeLogger.e(TAG, "Failed to init UserService with caller token", e)
                }
            } else {
                NativeLogger.e(TAG, "UserService binder is null or dead")
            }
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            NativeLogger.w(TAG, "UserService disconnected")
            isBinding = false
            clipboardService = null
            serviceConnected = false
        }
    }

    private val permissionResultListener =
        Shizuku.OnRequestPermissionResultListener { requestCode, grantResult ->
            if (requestCode == REQUEST_CODE_PERMISSION) {
                permissionGranted = grantResult == PackageManager.PERMISSION_GRANTED
                if (permissionGranted) {
                    bindUserService()
                }
            }
        }

    private val binderReceivedListener = Shizuku.OnBinderReceivedListener {
        NativeLogger.i(TAG, "Shizuku binder received")
        // 当 Shizuku 连接时，如果已有权限，自动绑定 UserService
        if (hasPermission()) {
            bindUserService()
        }
    }

    private val binderDeadListener = Shizuku.OnBinderDeadListener {
        NativeLogger.w(TAG, "Shizuku binder dead")
        permissionGranted = false
        clipboardService = null
        serviceConnected = false
    }

    private fun hasPermission(): Boolean {
        return try {
            if (!Shizuku.pingBinder()) false
            else if (Shizuku.isPreV11()) {
                val context = appContext.reactContext ?: return false
                context.checkSelfPermission("moe.shizuku.manager.permission.API_V23") ==
                    PackageManager.PERMISSION_GRANTED
            } else {
                Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED
            }
        } catch (e: Exception) {
            false
        }
    }

    private fun bindUserService() {
        if (serviceConnected || isBinding) return
        isBinding = true
        try {
            NativeLogger.i(TAG, "Binding UserService...")
            Shizuku.bindUserService(userServiceArgs, serviceConnection)
        } catch (e: Exception) {
            isBinding = false
            NativeLogger.e(TAG, "Failed to bind UserService", e)
        }
    }

    private fun unbindUserService() {
        if (!serviceConnected) return
        try {
            clipboardService?.destroy()
        } catch (e: Exception) {
            NativeLogger.e(TAG, "Failed to call destroy on UserService", e)
        }
        try {
            Shizuku.unbindUserService(userServiceArgs, serviceConnection, true)
        } catch (e: Exception) {
            NativeLogger.e(TAG, "Failed to unbind UserService", e)
        }
        clipboardService = null
        serviceConnected = false
    }

    private fun ensureServiceConnected(): IClipboardUserService? {
        if (clipboardService != null && serviceConnected) {
            return clipboardService
        }
        // 尝试重新绑定
        bindUserService()
        // 等待短暂时间让连接建立
        val startTime = System.currentTimeMillis()
        while (clipboardService == null && System.currentTimeMillis() - startTime < 3000) {
            Thread.sleep(100)
        }
        return clipboardService
    }

    override fun definition() = ModuleDefinition {
        Name("ShizukuClipboardModule")

        OnCreate {
            try {
                Shizuku.addRequestPermissionResultListener(permissionResultListener)
                Shizuku.addBinderReceivedListenerSticky(binderReceivedListener)
                Shizuku.addBinderDeadListener(binderDeadListener)
            } catch (e: Exception) {
                NativeLogger.e(TAG, "Failed to register Shizuku listeners", e)
            }
        }

        OnDestroy {
            try {
                unbindUserService()
                Shizuku.removeRequestPermissionResultListener(permissionResultListener)
                Shizuku.removeBinderReceivedListener(binderReceivedListener)
                Shizuku.removeBinderDeadListener(binderDeadListener)
            } catch (e: Exception) {
                NativeLogger.e(TAG, "Failed to cleanup Shizuku listeners", e)
            }
        }

        Function("isShizukuAvailable") {
            try {
                return@Function Shizuku.pingBinder()
            } catch (e: Exception) {
                return@Function false
            }
        }

        Function("hasShizukuPermission") {
            return@Function hasPermission()
        }

        Function("requestShizukuPermission") {
            try {
                if (!Shizuku.pingBinder()) return@Function false
                if (Shizuku.isPreV11()) return@Function false
                Shizuku.requestPermission(REQUEST_CODE_PERMISSION)
                return@Function true
            } catch (e: Exception) {
                NativeLogger.e(TAG, "Failed to request Shizuku permission", e)
                return@Function false
            }
        }

        AsyncFunction("getStringViaShizuku") { promise: Promise ->
            try {
                NativeLogger.i(TAG, "getStringViaShizuku: called")
                val service = ensureServiceConnected()
                if (service == null) {
                    NativeLogger.e(TAG, "getStringViaShizuku: UserService not connected")
                    promise.resolve("")
                    return@AsyncFunction
                }
                val text = service.primaryClipText ?: ""
                NativeLogger.i(TAG, "getStringViaShizuku: result length=${text.length}")
                promise.resolve(text)
            } catch (e: Exception) {
                NativeLogger.e(TAG, "getStringViaShizuku failed", e)
                promise.resolve("")
            }
        }

        AsyncFunction("hasStringViaShizuku") { promise: Promise ->
            try {
                val service = ensureServiceConnected()
                if (service == null) {
                    promise.resolve(false)
                    return@AsyncFunction
                }
                promise.resolve(service.hasPrimaryClipText())
            } catch (e: Exception) {
                NativeLogger.e(TAG, "hasStringViaShizuku failed", e)
                promise.resolve(false)
            }
        }

        AsyncFunction("hasImageViaShizuku") { promise: Promise ->
            try {
                val service = ensureServiceConnected()
                if (service == null) {
                    promise.resolve(false)
                    return@AsyncFunction
                }
                promise.resolve(service.hasPrimaryClipImage())
            } catch (e: Exception) {
                NativeLogger.e(TAG, "hasImageViaShizuku failed", e)
                promise.resolve(false)
            }
        }

        AsyncFunction("getImageUriViaShizuku") { promise: Promise ->
            try {
                val service = ensureServiceConnected()
                if (service == null) {
                    promise.resolve(null)
                    return@AsyncFunction
                }
                val uri = service.primaryClipImageUri
                promise.resolve(if (uri.isNullOrEmpty()) null else uri)
            } catch (e: Exception) {
                NativeLogger.e(TAG, "getImageUriViaShizuku failed", e)
                promise.resolve(null)
            }
        }
    }
}
