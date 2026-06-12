package expo.modules.foregroundservice

import android.content.Context
import android.content.Intent
import android.os.Build
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class ForegroundServiceModule : Module() {

    companion object {
        private var moduleInstance: ForegroundServiceModule? = null

        /**
         * JS 端主动调用 startService() 后置为 true，表示 JS 业务逻辑确实在运行。
         * 进程重建时静态字段重置为 false；模块销毁时（JS bridge 关闭）也重置为 false。
         * 仅凭 RN bridge 初始化（moduleInstance != null）无法说明 JS 任务在执行，
         * 因为 START_STICKY 重启时 bridge 可能在后台静默初始化却不运行任何业务代码。
         */
        private var jsInitiatedService = false

        fun sendStopEvent() {
            moduleInstance?.sendEvent("onStopRequested", emptyMap<String, Any>())
        }

        fun sendTempStopEvent() {
            moduleInstance?.sendEvent("onTempStopRequested", emptyMap<String, Any>())
        }

        fun isJsRuntimeAlive(): Boolean {
            return jsInitiatedService
        }
    }

    override fun definition() = ModuleDefinition {
        Name("ForegroundServiceModule")

        Events("onStopRequested", "onTempStopRequested")

        OnCreate {
            moduleInstance = this@ForegroundServiceModule
        }

        OnDestroy {
            jsInitiatedService = false
            if (moduleInstance == this@ForegroundServiceModule) {
                moduleInstance = null
            }
        }

        Function("startService") {
            if (SyncForegroundService.isRunning) return@Function true
            val context = appContext.reactContext ?: return@Function false
            jsInitiatedService = true
            val intent = Intent(context, SyncForegroundService::class.java).apply {
                action = SyncForegroundService.ACTION_START
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
            true
        }

        Function("stopService") {
            val context = appContext.reactContext ?: return@Function false
            if (!SyncForegroundService.isRunning) return@Function true
            // 标记为用户主动停止，避免 onDestroy 中误发重启通知
            SyncForegroundService.stoppedByUser = true
            val intent = Intent(context, SyncForegroundService::class.java)
            context.stopService(intent)
            true
        }

        Function("updateNotification") { content: String ->
            val context = appContext.reactContext ?: return@Function false
            // 服务未运行时不发送 startService，避免意外重启前台服务
            if (!SyncForegroundService.isRunning) return@Function false
            val intent = Intent(context, SyncForegroundService::class.java).apply {
                action = SyncForegroundService.ACTION_UPDATE
                putExtra(SyncForegroundService.EXTRA_CONTENT, content)
            }
            context.startService(intent)
            true
        }

        Function("isRunning") {
            SyncForegroundService.isRunning
        }
    }
}
