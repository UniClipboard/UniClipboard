package app.uniclipboard.engine

import android.app.Activity
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.View
import android.view.animation.AccelerateInterpolator
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import java.lang.ref.WeakReference

/**
 * Coordinates the Android native launch splash screen so it stays visible
 * until React Native and the Go Engine complete bootstrap, then transitions
 * cleanly into the first rendered screen without flicker.
 */
object AndroidSplashController {
  private const val LEGACY_FADE_OUT_DURATION_MS = 220L
  private const val SAFETY_TIMEOUT_MS = 4500L

  private val mainHandler = Handler(Looper.getMainLooper())
  @Volatile private var keepOnScreen = true
  @Volatile private var jsReady = false
  private var activeActivityRef: WeakReference<Activity>? = null
  private var timeoutRunnable: Runnable? = null

  fun install(activity: Activity) {
    val splashScreen = activity.installSplashScreen()
    activeActivityRef = WeakReference(activity)

    // If JS already completed bootstrap in this process (e.g. Activity recreation),
    // release immediately instead of waiting for a one-shot effect that already ran.
    keepOnScreen = !jsReady
    splashScreen.setKeepOnScreenCondition { keepOnScreen }

    // On Android 12+ (API 31+), letting SurfaceFlinger/SystemUI perform the native
    // exit transition avoids the 1-2 frame transfer flicker and API 31-33
    // SurfaceControl race associated with custom SplashScreenView exit listeners.
    // On API < 31, fade out the compat splash overlay smoothly in-process.
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
      splashScreen.setOnExitAnimationListener { provider ->
        provider.view
          .animate()
          .setDuration(LEGACY_FADE_OUT_DURATION_MS)
          .alpha(0f)
          .setInterpolator(AccelerateInterpolator())
          .withEndAction {
            runCatching { provider.remove() }
          }
          .start()
      }
    }

    scheduleSafetyTimeout()
  }

  private fun scheduleSafetyTimeout() {
    mainHandler.post {
      timeoutRunnable?.let { mainHandler.removeCallbacks(it) }
      timeoutRunnable = null
      if (!keepOnScreen) return@post
      val runnable = Runnable {
        keepOnScreen = false
        activeActivityRef?.get()?.findViewById<View>(android.R.id.content)?.invalidate()
      }
      timeoutRunnable = runnable
      mainHandler.postDelayed(runnable, SAFETY_TIMEOUT_MS)
    }
  }

  fun hide() {
    jsReady = true
    keepOnScreen = false
    mainHandler.post {
      timeoutRunnable?.let { mainHandler.removeCallbacks(it) }
      timeoutRunnable = null
      activeActivityRef?.get()?.findViewById<View>(android.R.id.content)?.invalidate()
    }
  }
}
