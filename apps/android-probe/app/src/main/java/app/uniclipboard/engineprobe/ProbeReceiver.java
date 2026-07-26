package app.uniclipboard.engineprobe;

import android.app.Activity;
import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.util.Base64;

import org.json.JSONObject;

import java.io.File;
import java.nio.charset.StandardCharsets;

public final class ProbeReceiver extends BroadcastReceiver {
    public static final String ACTION = "app.uniclipboard.engineprobe.COMMAND";
    public static final String EXTRA_COMMAND_BASE64 = "command_base64";

    @Override
    public void onReceive(Context context, Intent intent) {
        PendingResult pending = goAsync();
        Thread worker = new Thread(
                () -> execute(context.getApplicationContext(), intent, pending),
                "uc-android-probe-command");
        worker.start();
    }

    private static void execute(Context context, Intent intent, PendingResult pending) {
        try {
            String encoded = intent.getStringExtra(EXTRA_COMMAND_BASE64);
            if (encoded == null || encoded.isEmpty()) {
                complete(pending, Activity.RESULT_CANCELED, error("missing_command"));
                return;
            }
            String command = new String(
                    Base64.decode(encoded, Base64.NO_WRAP),
                    StandardCharsets.UTF_8);
            boolean shutdown = "shutdown".equals(new JSONObject(command).optString("command"));
            ProbeBridge bridge = new ProbeBridge(context);
            String startResult = bridge.command(startCommand(context));
            JSONObject start = new JSONObject(startResult);
            String kind = start.optString("kind");
            if (!start.optBoolean("ok") && !"already_started".equals(kind)) {
                complete(pending, Activity.RESULT_CANCELED, startResult);
                return;
            }
            complete(pending, Activity.RESULT_OK, bridge.command(command));
            if (shutdown) {
                context.stopService(new Intent(context, ProbeService.class));
            }
        } catch (Exception ignored) {
            complete(pending, Activity.RESULT_CANCELED, error("command_failed"));
        }
    }

    private static String startCommand(Context context) throws Exception {
        File privateData = new File(context.getFilesDir(), "engine");
        File temporary = new File(context.getCacheDir(), "temporary");
        return new JSONObject()
                .put("command", "start")
                .put("private_data", privateData.getAbsolutePath())
                .put("cache", context.getCacheDir().getAbsolutePath())
                .put("temporary", temporary.getAbsolutePath())
                .put("app_version", "0.1.0")
                .toString();
    }

    private static String error(String kind) {
        try {
            return new JSONObject().put("ok", false).put("kind", kind).toString();
        } catch (Exception ignored) {
            return "{\"ok\":false,\"kind\":\"command_failed\"}";
        }
    }

    private static void complete(PendingResult pending, int code, String result) {
        pending.setResultCode(code);
        pending.setResultData(result);
        pending.finish();
    }
}
