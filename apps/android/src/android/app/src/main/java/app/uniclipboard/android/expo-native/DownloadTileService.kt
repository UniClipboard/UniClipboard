package app.uniclipboard.android.quicksettings

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.ComponentName
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import expo.modules.nativeutil.NativeLogger

class DownloadTileService : TileService() {

    companion object {
        private const val TAG = "DownloadTileService"
    }

    override fun onStartListening() {
        super.onStartListening()
        qsTile?.apply {
            state = Tile.STATE_INACTIVE
            updateTile()
        }
    }

    override fun onStopListening() {
        super.onStopListening()
    }

    @SuppressLint("StartActivityAndCollapseDeprecated")
    override fun onClick() {
        super.onClick()
        NativeLogger.d(TAG, "Quick Settings Tile clicked")
        try {
            val intent = Intent().apply {
                component = ComponentName(
                    applicationContext,
                    "app.uniclipboard.android.quickaction.QuickActionActivity"
                )
                putExtra("direction", "download")
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                // API 34+ - use PendingIntent
                val pendingIntent = PendingIntent.getActivity(
                    this,
                    0,
                    intent,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
                )
                startActivityAndCollapse(pendingIntent)
            } else {
                // API < 34 - use Intent (deprecated but necessary for older APIs)
                @Suppress("DEPRECATION")
                startActivityAndCollapse(intent)
            }
        } catch (e: Exception) {
            NativeLogger.e(TAG, "Error handling tile click", e)
        }
    }

    override fun onTileAdded() {
        super.onTileAdded()
    }

    override fun onTileRemoved() {
        super.onTileRemoved()
    }
}

