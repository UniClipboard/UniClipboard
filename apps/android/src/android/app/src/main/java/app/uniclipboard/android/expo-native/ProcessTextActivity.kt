package app.uniclipboard.android.processtext

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import expo.modules.nativeutil.NativeLogger

/**
 * Trampoline Activity for Android "Process Text" floating toolbar action.
 * Receives the selected text via PROCESS_TEXT intent, encodes it in a deep link URL,
 * and forwards it to the main React Native Activity.
 */
class ProcessTextActivity : Activity() {

    companion object {
        private const val TAG = "ProcessTextActivity"
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val text = intent?.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT)?.toString()
        if (text.isNullOrEmpty()) {
            NativeLogger.w(TAG, "No text received in PROCESS_TEXT intent")
            finish()
            return
        }

        NativeLogger.d(TAG, "Received process text: ${text.take(50)}")

        val encodedText = Uri.encode(text)
        val url = "uniclipboard://process-text?text=$encodedText"

        val mainIntent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP)
            addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }

        try {
            startActivity(mainIntent)
        } catch (e: Exception) {
            NativeLogger.e(TAG, "Failed to start main activity", e)
        }

        finish()
    }
}
