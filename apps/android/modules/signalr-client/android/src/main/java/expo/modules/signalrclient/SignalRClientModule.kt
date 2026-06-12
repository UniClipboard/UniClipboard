package expo.modules.signalrclient

import android.os.Handler
import android.os.Looper
import android.util.Base64
import expo.modules.nativeutil.NativeLogger
import com.google.gson.JsonObject
import com.microsoft.signalr.HubConnection
import com.microsoft.signalr.HubConnectionBuilder
import com.microsoft.signalr.HubConnectionState
import com.microsoft.signalr.TransportEnum
import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class SignalRClientModule : Module() {

    private val handler = Handler(Looper.getMainLooper())

    private var hubConnection: HubConnection? = null
    private var isConnecting = false
    private val reconnectHandler = Handler(Looper.getMainLooper())
    private var reconnectRunnable: Runnable? = null
    private var reconnectAttempt = 0
    private val maxReconnectAttempts = 10
    private var currentUrl: String? = null
    private var currentUsername: String? = null
    private var currentPassword: String? = null

    companion object {
        private const val TAG = "SignalRClientModule"
    }

    override fun definition() = ModuleDefinition {
        Name("SignalRClientModule")

        Events("onProfileChanged", "onHistoryChanged", "onStateChanged")

        Function("connect") { url: String, username: String, password: String ->
            connectSignalR(url, username, password)
        }

        Function("disconnect") {
            disconnectSignalR()
        }

        Function("isConnected") {
            hubConnection?.connectionState == HubConnectionState.CONNECTED
        }

        Function("getState") {
            hubConnection?.connectionState?.name ?: "DISCONNECTED"
        }

        OnDestroy {
            disconnectSignalR()
        }
    }

    private fun connectSignalR(url: String, username: String, password: String) {
        if (isConnecting) return
        if (hubConnection?.connectionState == HubConnectionState.CONNECTED &&
            currentUrl == url && currentUsername == username) {
            NativeLogger.d(TAG, "Already connected to $url")
            return
        }

        disconnectSignalRInternal()

        currentUrl = url
        currentUsername = username
        currentPassword = password
        isConnecting = true

        val hubUrl = url.trimEnd('/') + "/SyncClipboardHub"
        val credentials = "$username:$password"
        val encodedCredentials = Base64.encodeToString(credentials.toByteArray(), Base64.NO_WRAP)

        NativeLogger.d(TAG, "Connecting to SignalR hub: $hubUrl")

        try {
            val connection = HubConnectionBuilder
                .create(hubUrl)
                .withHeader("Authorization", "Basic $encodedCredentials")
                .withTransport(TransportEnum.WEBSOCKETS)
                .shouldSkipNegotiate(false)
                .build()

            connection.on("RemoteProfileChanged", { profileJson: JsonObject ->
                NativeLogger.d(TAG, "RemoteProfileChanged received")
                handler.post {
                    sendEvent("onProfileChanged", mapOf(
                        "type" to getJsonString(profileJson, "Type", "Text"),
                        "hash" to getJsonString(profileJson, "Hash", ""),
                        "text" to getJsonString(profileJson, "Text", ""),
                        "hasData" to getJsonBoolean(profileJson, "HasData", false),
                        "dataName" to getJsonStringOrNull(profileJson, "DataName"),
                        "size" to getJsonLong(profileJson, "Size", 0L)
                    ))
                }
            }, JsonObject::class.java)

            connection.on("RemoteHistoryChanged", { historyJson: JsonObject ->
                NativeLogger.d(TAG, "RemoteHistoryChanged received")
                handler.post {
                    sendEvent("onHistoryChanged", mapOf(
                        "hash" to getJsonString(historyJson, "Hash", ""),
                        "text" to getJsonString(historyJson, "Text", ""),
                        "type" to getJsonString(historyJson, "Type", "Text"),
                        "hasData" to getJsonBoolean(historyJson, "HasData", false),
                        "size" to getJsonLong(historyJson, "Size", 0L),
                        "starred" to getJsonBoolean(historyJson, "Starred", false),
                        "pinned" to getJsonBoolean(historyJson, "Pinned", false),
                        "version" to getJsonInt(historyJson, "Version", 0),
                        "isDeleted" to getJsonBoolean(historyJson, "IsDeleted", false),
                        "createTime" to getJsonStringOrNull(historyJson, "CreateTime"),
                        "lastModified" to getJsonStringOrNull(historyJson, "LastModified"),
                        "lastAccessed" to getJsonStringOrNull(historyJson, "LastAccessed")
                    ))
                }
            }, JsonObject::class.java)

            connection.onClosed { error ->
                NativeLogger.d(TAG, "SignalR connection closed: ${error?.message}")
                handler.post {
                    sendEvent("onStateChanged", mapOf("state" to "DISCONNECTED"))
                }
                if (currentUrl != null) {
                    scheduleReconnect()
                }
            }

            hubConnection = connection

            Thread {
                try {
                    connection.start().blockingAwait()
                    reconnectAttempt = 0
                    isConnecting = false
                    NativeLogger.d(TAG, "SignalR connected successfully")
                    handler.post {
                        sendEvent("onStateChanged", mapOf("state" to "CONNECTED"))
                    }
                } catch (e: Exception) {
                    isConnecting = false
                    NativeLogger.e(TAG, "SignalR connection failed", e)
                    handler.post {
                        sendEvent("onStateChanged", mapOf("state" to "DISCONNECTED"))
                    }
                    scheduleReconnect()
                }
            }.start()

        } catch (e: Exception) {
            isConnecting = false
            NativeLogger.e(TAG, "Failed to create SignalR connection", e)
        }
    }

    private fun disconnectSignalR() {
        currentUrl = null
        currentUsername = null
        currentPassword = null
        cancelReconnect()
        disconnectSignalRInternal()
    }

    private fun disconnectSignalRInternal() {
        hubConnection?.let { conn ->
            try {
                if (conn.connectionState != HubConnectionState.DISCONNECTED) {
                    Thread {
                        try {
                            conn.stop().blockingAwait()
                        } catch (e: Exception) {
                            NativeLogger.e(TAG, "Error stopping SignalR", e)
                        }
                    }.start()
                }
            } catch (e: Exception) {
                NativeLogger.e(TAG, "Error during SignalR disconnect", e)
            }
        }
        hubConnection = null
        isConnecting = false
    }

    private fun scheduleReconnect() {
        if (currentUrl == null) return
        if (reconnectAttempt >= maxReconnectAttempts) {
            NativeLogger.d(TAG, "Max reconnect attempts reached")
            return
        }

        cancelReconnect()
        val delayMs = minOf(2000L * (1L shl reconnectAttempt), 60000L)
        reconnectAttempt++
        NativeLogger.d(TAG, "Scheduling SignalR reconnect attempt $reconnectAttempt in ${delayMs}ms")

        val runnable = Runnable {
            val url = currentUrl ?: return@Runnable
            val user = currentUsername ?: return@Runnable
            val pass = currentPassword ?: return@Runnable
            isConnecting = false
            connectSignalR(url, user, pass)
        }
        reconnectRunnable = runnable
        reconnectHandler.postDelayed(runnable, delayMs)
    }

    private fun cancelReconnect() {
        reconnectRunnable?.let {
            reconnectHandler.removeCallbacks(it)
        }
        reconnectRunnable = null
        reconnectAttempt = 0
    }

    private fun getJsonString(json: JsonObject, key: String, default: String): String {
        val element = json.get(key) ?: json.get(key.replaceFirstChar { it.lowercase() })
        return if (element != null && !element.isJsonNull) element.asString else default
    }

    private fun getJsonStringOrNull(json: JsonObject, key: String): String? {
        val element = json.get(key) ?: json.get(key.replaceFirstChar { it.lowercase() })
        return if (element != null && !element.isJsonNull) element.asString else null
    }

    private fun getJsonBoolean(json: JsonObject, key: String, default: Boolean): Boolean {
        val element = json.get(key) ?: json.get(key.replaceFirstChar { it.lowercase() })
        return if (element != null && !element.isJsonNull) element.asBoolean else default
    }

    private fun getJsonLong(json: JsonObject, key: String, default: Long): Long {
        val element = json.get(key) ?: json.get(key.replaceFirstChar { it.lowercase() })
        return if (element != null && !element.isJsonNull) element.asLong else default
    }

    private fun getJsonInt(json: JsonObject, key: String, default: Int): Int {
        val element = json.get(key) ?: json.get(key.replaceFirstChar { it.lowercase() })
        return if (element != null && !element.isJsonNull) element.asInt else default
    }
}
