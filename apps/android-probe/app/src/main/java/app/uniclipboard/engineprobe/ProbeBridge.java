package app.uniclipboard.engineprobe;

import android.content.Context;
import android.content.SharedPreferences;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;

import java.nio.ByteBuffer;
import java.security.KeyStore;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

final class ProbeBridge {
    private static final String KEYSTORE = "AndroidKeyStore";
    private static final String KEY_ALIAS = "app.uniclipboard.engineprobe.storage";
    private static final String CIPHER = "AES/GCM/NoPadding";
    private static final String PREFERENCES = "encrypted_secure_storage";

    static {
        System.loadLibrary("uc_mobile_probe_core");
    }

    private final SharedPreferences preferences;

    ProbeBridge(Context context) {
        preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE);
        nativeInstallHost(this, context.getApplicationContext());
    }

    String command(String command) {
        return nativeCommand(command);
    }

    public byte[] secureGet(String key) throws Exception {
        String encoded = preferences.getString(key, null);
        if (encoded == null) {
            return null;
        }
        byte[] payload = Base64.decode(encoded, Base64.NO_WRAP);
        ByteBuffer buffer = ByteBuffer.wrap(payload);
        int ivLength = buffer.getInt();
        if (ivLength <= 0 || ivLength > 32 || buffer.remaining() <= ivLength) {
            throw new IllegalStateException("invalid encrypted value");
        }
        byte[] iv = new byte[ivLength];
        buffer.get(iv);
        byte[] ciphertext = new byte[buffer.remaining()];
        buffer.get(ciphertext);

        Cipher cipher = Cipher.getInstance(CIPHER);
        cipher.init(Cipher.DECRYPT_MODE, getOrCreateKey(), new GCMParameterSpec(128, iv));
        return cipher.doFinal(ciphertext);
    }

    public void secureSet(String key, byte[] value) throws Exception {
        Cipher cipher = Cipher.getInstance(CIPHER);
        cipher.init(Cipher.ENCRYPT_MODE, getOrCreateKey());
        byte[] ciphertext = cipher.doFinal(value);
        byte[] iv = cipher.getIV();
        ByteBuffer payload = ByteBuffer.allocate(Integer.BYTES + iv.length + ciphertext.length);
        payload.putInt(iv.length);
        payload.put(iv);
        payload.put(ciphertext);
        boolean saved = preferences
                .edit()
                .putString(key, Base64.encodeToString(payload.array(), Base64.NO_WRAP))
                .commit();
        if (!saved) {
            throw new IllegalStateException("encrypted value was not saved");
        }
    }

    public void secureDelete(String key) {
        if (!preferences.edit().remove(key).commit()) {
            throw new IllegalStateException("encrypted value was not deleted");
        }
    }

    private SecretKey getOrCreateKey() throws Exception {
        KeyStore keyStore = KeyStore.getInstance(KEYSTORE);
        keyStore.load(null);
        java.security.Key existing = keyStore.getKey(KEY_ALIAS, null);
        if (existing instanceof SecretKey) {
            return (SecretKey) existing;
        }

        KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE);
        generator.init(new KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .build());
        return generator.generateKey();
    }

    private static native void nativeInstallHost(ProbeBridge bridge, Context context);

    private static native String nativeCommand(String command);
}
