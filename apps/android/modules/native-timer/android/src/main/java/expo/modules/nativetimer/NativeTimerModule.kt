package expo.modules.nativetimer

import android.os.Handler
import android.os.Looper
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class NativeTimerModule : Module() {

    private val handler = Handler(Looper.getMainLooper())
    private val timerRunnables = mutableMapOf<String, Runnable>()

    override fun definition() = ModuleDefinition {
        Name("NativeTimerModule")

        Events("onTick")

        Function("startTimer") { tag: String, intervalMs: Int ->
            stopTimerInternal(tag)
            val runnable = object : Runnable {
                override fun run() {
                    sendEvent("onTick", mapOf("tag" to tag))
                    handler.postDelayed(this, intervalMs.toLong())
                }
            }
            timerRunnables[tag] = runnable
            handler.postDelayed(runnable, intervalMs.toLong())
        }

        Function("stopTimer") { tag: String ->
            stopTimerInternal(tag)
        }

        Function("stopAllTimers") {
            for (entry in timerRunnables) {
                handler.removeCallbacks(entry.value)
            }
            timerRunnables.clear()
        }

        OnDestroy {
            for (entry in timerRunnables) {
                handler.removeCallbacks(entry.value)
            }
            timerRunnables.clear()
        }
    }

    private fun stopTimerInternal(tag: String) {
        timerRunnables[tag]?.let {
            handler.removeCallbacks(it)
            timerRunnables.remove(tag)
        }
    }
}
