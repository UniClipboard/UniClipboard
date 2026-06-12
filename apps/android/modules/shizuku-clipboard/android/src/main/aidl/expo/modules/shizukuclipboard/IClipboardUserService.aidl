package expo.modules.shizukuclipboard;

interface IClipboardUserService {
    String getPrimaryClipText() = 1;
    boolean hasPrimaryClipText() = 2;
    boolean hasPrimaryClipImage() = 3;
    String getPrimaryClipImageUri() = 4;
    void init(IBinder callerToken) = 5;
    void destroy() = 16777114;
}
