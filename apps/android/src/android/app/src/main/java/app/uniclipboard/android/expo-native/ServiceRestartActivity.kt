package app.uniclipboard.android.servicerestart

import android.os.Bundle

import com.facebook.react.ReactActivity
import com.facebook.react.ReactActivityDelegate
import com.facebook.react.defaults.DefaultNewArchitectureEntryPoint.fabricEnabled
import com.facebook.react.defaults.DefaultReactActivityDelegate

import android.content.res.Configuration
import app.uniclipboard.android.BuildConfig
import expo.modules.ReactActivityDelegateWrapper

/**
 * 轻量级透明 Activity，用于后台服务被系统重启后引导 JS 运行时启动。
 * 显示一个简短的"服务已恢复"提示，0.5 秒后自动关闭。
 */
class ServiceRestartActivity : ReactActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(null)
    }

    override fun getMainComponentName(): String = "serviceRestart"

    override fun createReactActivityDelegate(): ReactActivityDelegate {
        return ReactActivityDelegateWrapper(
            this,
            BuildConfig.IS_NEW_ARCHITECTURE_ENABLED,
            object : DefaultReactActivityDelegate(
                this,
                mainComponentName,
                fabricEnabled
            ) {
                override fun getLaunchOptions(): Bundle? {
                    val isDarkMode = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
                    return Bundle().apply {
                        putString("systemTheme", if (isDarkMode) "dark" else "light")
                    }
                }
            }
        )
    }
}
