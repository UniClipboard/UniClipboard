package app.uniclipboard.engineprobe;

import android.app.Activity;
import android.content.Intent;
import android.os.Build;
import android.os.Bundle;

public final class ProbeActivity extends Activity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        Intent service = new Intent(this, ProbeService.class);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(service);
        } else {
            startService(service);
        }
        finish();
    }
}
