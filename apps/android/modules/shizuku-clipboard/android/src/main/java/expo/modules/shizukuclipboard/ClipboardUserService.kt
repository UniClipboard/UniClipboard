package expo.modules.shizukuclipboard

import android.content.ClipData
import android.content.ClipDescription
import android.os.IBinder
import java.lang.reflect.Method



/**
 * ClipboardUserService — 运行在 Shizuku 进程（UID 2000/shell）中
 *
 * 在 Shizuku 的进程中，没有隐藏 API 限制，可以自由反射调用 IClipboard 的方法。
 * 通过 ServiceManager 获取 clipboard binder，然后反射调用 getPrimaryClip 等方法。
 */
class ClipboardUserService : IClipboardUserService.Stub() {

    companion object {
        private const val TAG = "ShizukuClipboard.UserService"
        // UserService 以 UID 2000 (shell) 运行，需要使用 shell 的包名
        private const val PACKAGE_NAME = "com.android.shell"

        init {
            // 当 Shizuku 以 root 权限运行时，UserService 进程继承 UID 0。
            // 剪贴板服务会校验 callingPackage 对应的 UID 是否匹配 Binder.getCallingUid()，
            // "com.android.shell"(UID 2000) 与 root(UID 0) 不匹配，导致访问被拒绝。
            // 修复：将进程身份从 root 切换到 shell，使 UID 与 PACKAGE_NAME 一致。
            // UserService 运行在独立进程（processNameSuffix="clipboard"）中，不影响 Shizuku 主进程。
            if (android.os.Process.myUid() == 0) {
                try {
                    // setgid 必须在 setuid 之前调用，因为 setuid 后将失去 root 权限
                    android.system.Os.setgid(2000)
                    android.system.Os.setuid(2000)
                    android.util.Log.i(TAG, "Switched UID/GID from root(0) to shell(2000) for clipboard access")
                } catch (e: Exception) {
                    android.util.Log.e(TAG, "Failed to switch UID from root to shell", e)
                }
            }
        }

        private var clipboardService: Any? = null

        private fun getClipboardService(): Any? {
            if (clipboardService != null) return clipboardService
            return try {
                // 在 Shizuku 进程中，通过 ServiceManager 获取 clipboard service
                val serviceManager = Class.forName("android.os.ServiceManager")
                val getService = serviceManager.getMethod("getService", String::class.java)
                val binder = getService.invoke(null, "clipboard") as? IBinder
                if (binder == null) {
                    android.util.Log.e(TAG, "ServiceManager returned null for clipboard")
                    return null
                }
                val iClipboardStub = Class.forName("android.content.IClipboard\$Stub")
                val asInterface = iClipboardStub.getMethod("asInterface", IBinder::class.java)
                clipboardService = asInterface.invoke(null, binder)
                android.util.Log.d(TAG, "Got clipboard service: ${clipboardService?.javaClass?.name}")
                clipboardService
            } catch (e: Exception) {
                android.util.Log.e(TAG, "Failed to get clipboard service", e)
                null
            }
        }

        /**
         * 动态查找并调用剪贴板方法
         */
        private fun findAndInvokeMethod(clipboard: Any, methodName: String): Any? {
            val clazz = clipboard.javaClass
            val methods = clazz.methods
                .filter { it.name == methodName }
                .sortedByDescending { it.parameterCount }

            for (method in methods) {
                val params = method.parameterTypes
                val args = buildArgs(params) ?: continue
                // 将第一个 String 参数设为 packageName
                for (i in params.indices) {
                    if (params[i] == String::class.java) {
                        args[i] = PACKAGE_NAME
                        break
                    }
                }
                return try {
                    android.util.Log.d(TAG, "Calling ${method.name}(${params.joinToString { it.simpleName }}) with args=${args.joinToString()}")
                    method.invoke(clipboard, *args)
                } catch (e: Exception) {
                    android.util.Log.e(TAG, "Failed to invoke $methodName with ${params.size} params", e)
                    if (e is java.lang.reflect.InvocationTargetException) {
                        android.util.Log.e(TAG, "  cause: ${e.cause}", e.cause)
                    }
                    null
                }
            }
            android.util.Log.e(TAG, "No suitable method found: $methodName")
            return null
        }

        private fun buildArgs(paramTypes: Array<Class<*>>): Array<Any?>? {
            return try {
                paramTypes.map { type ->
                    when {
                        type == String::class.java -> null
                        type == Int::class.javaPrimitiveType || type == Int::class.java -> 0
                        type == Long::class.javaPrimitiveType || type == Long::class.java -> 0L
                        type == Boolean::class.javaPrimitiveType || type == Boolean::class.java -> false
                        else -> return null
                    }
                }.toTypedArray()
            } catch (e: Exception) {
                null
            }
        }
    }

    override fun getPrimaryClipText(): String {
        return try {
            val clipboard = getClipboardService() ?: return ""
            val clip = findAndInvokeMethod(clipboard, "getPrimaryClip") as? ClipData
            if (clip != null && clip.itemCount > 0) {
                val text = clip.getItemAt(0).text?.toString() ?: ""
                android.util.Log.d(TAG, "getPrimaryClipText: length=${text.length}")
                text
            } else {
                android.util.Log.d(TAG, "getPrimaryClipText: clip is null or empty")
                ""
            }
        } catch (e: Exception) {
            android.util.Log.e(TAG, "getPrimaryClipText failed", e)
            ""
        }
    }

    override fun hasPrimaryClipText(): Boolean {
        return try {
            val clipboard = getClipboardService() ?: return false
            val desc = findAndInvokeMethod(clipboard, "getPrimaryClipDescription") as? ClipDescription
            desc?.hasMimeType("text/*") ?: false
        } catch (e: Exception) {
            android.util.Log.e(TAG, "hasPrimaryClipText failed", e)
            false
        }
    }

    override fun hasPrimaryClipImage(): Boolean {
        return try {
            val clipboard = getClipboardService() ?: return false
            val desc = findAndInvokeMethod(clipboard, "getPrimaryClipDescription") as? ClipDescription
            desc?.let { it.hasMimeType("image/*") || it.hasMimeType("application/octet-stream") } ?: false
        } catch (e: Exception) {
            android.util.Log.e(TAG, "hasPrimaryClipImage failed", e)
            false
        }
    }

    override fun getPrimaryClipImageUri(): String {
        return try {
            val clipboard = getClipboardService() ?: return ""
            val clip = findAndInvokeMethod(clipboard, "getPrimaryClip") as? ClipData
            if (clip != null && clip.itemCount > 0) {
                clip.getItemAt(0).uri?.toString() ?: ""
            } else {
                ""
            }
        } catch (e: Exception) {
            android.util.Log.e(TAG, "getPrimaryClipImageUri failed", e)
            ""
        }
    }

    override fun init(callerToken: IBinder) {
        try {
            callerToken.linkToDeath({
                android.util.Log.i(TAG, "Caller process died, exiting UserService")
                System.exit(0)
            }, 0)
            android.util.Log.i(TAG, "Linked to caller token for death detection")
        } catch (e: Exception) {
            android.util.Log.e(TAG, "Failed to link to caller token death", e)
        }
    }

    override fun destroy() {
        android.util.Log.i(TAG, "UserService destroy called, exiting process")
        clipboardService = null
        System.exit(0)
    }
}
