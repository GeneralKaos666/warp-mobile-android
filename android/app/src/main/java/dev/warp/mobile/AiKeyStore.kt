package dev.warp.mobile

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey

/**
 * M6-S02: Keystore-backed Anthropic API key storage.
 *
 * Wraps `EncryptedSharedPreferences` so the BYOK API key never lands in
 * plaintext on disk. The wrapping AES-256-GCM key is held in the Android
 * Keystore — on Knox-equipped devices (S24 Ultra), Keystore keys are
 * backed by the secure-element (Strongbox / TrustZone) and cannot be
 * extracted even with root.
 *
 * Schema:
 *   alias  : "warp-ai-key-v1" (master-key alias; bumping → key invalidation)
 *   prefs  : "warp-ai-prefs.enc"
 *   field  : "anthropic-api-key" (the Bearer token user pasted)
 *
 * Threading: SharedPreferences operations are thread-safe per Android docs;
 * but EncryptedSharedPreferences should NOT be created on the main thread
 * (key generation can take a few hundred ms). Use [getOrCreate] from a
 * background thread + cache the result.
 *
 * Refs:
 *   https://developer.android.com/reference/androidx/security/crypto/EncryptedSharedPreferences
 *   https://developer.android.com/topic/security/data
 */
object AiKeyStore {
    private const val LOG_TAG = "WarpAiKeyStore"
    private const val MASTER_KEY_ALIAS = "warp-ai-key-v1"
    private const val PREFS_NAME = "warp-ai-prefs.enc"
    private const val KEY_API_KEY = "anthropic-api-key"

    @Volatile private var cached: SharedPreferences? = null

    /**
     * Get or create the encrypted SharedPreferences instance. Generates
     * the Keystore-backed master key on first call (~100-300 ms; do not
     * call from main thread).
     *
     * @throws java.security.GeneralSecurityException if Keystore generation
     *   fails (e.g. user has not set up device lock, hardware unavailable)
     */
    @Synchronized
    fun getOrCreate(context: Context): SharedPreferences {
        cached?.let { return it }
        val masterKey = MasterKey.Builder(context, MASTER_KEY_ALIAS)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            // Knox / TEE backing where available; fall back to software
            // Keystore on devices without secure element. minSdk 31 +
            // S24U has TEE.
            .setUserAuthenticationRequired(false)
            .build()
        val prefs = EncryptedSharedPreferences.create(
            context,
            PREFS_NAME,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
        cached = prefs
        Log.i(LOG_TAG, "EncryptedSharedPreferences ready (alias=$MASTER_KEY_ALIAS)")
        return prefs
    }

    /**
     * Returns the saved API key or null. Call from background thread on
     * first invocation per session (cached after that).
     */
    fun load(context: Context): String? {
        return getOrCreate(context).getString(KEY_API_KEY, null)
    }

    /** Save / replace the API key. */
    fun save(context: Context, key: String) {
        getOrCreate(context).edit().putString(KEY_API_KEY, key).apply()
    }

    /** Forget the saved key (e.g. user "Sign out" or rotation). */
    fun clear(context: Context) {
        getOrCreate(context).edit().remove(KEY_API_KEY).apply()
    }

    /**
     * Redacted form for logs. Returns "Bearer sk-ant-***...XXXX" where
     * XXXX is the last 4 chars of the key. Never log the full key.
     */
    fun redact(key: String?): String {
        if (key.isNullOrEmpty()) return "(no key)"
        val tail = if (key.length >= 4) key.takeLast(4) else "?"
        return "Bearer ${key.take(8)}***...$tail"
    }
}
