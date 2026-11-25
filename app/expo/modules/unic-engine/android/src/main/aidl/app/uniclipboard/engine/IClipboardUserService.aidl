package app.uniclipboard.engine;

import android.os.IBinder;
import android.os.ParcelFileDescriptor;

interface IClipboardUserService {
    void init(in IBinder callerToken);
    String getPrimaryClipStamp();
    String getPrimaryClipJson();
    boolean copyPrimaryClipToFile(in ParcelFileDescriptor destination);
    boolean setPrimaryClipText(String text);
    boolean resolveBackgroundClipboardRestriction();
    void destroy();
}
