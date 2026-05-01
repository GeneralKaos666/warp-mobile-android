package dev.warp.mobile

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.io.File

class WarpTerminalService : Service() {

    companion object {
        init { System.loadLibrary("warp_mobile_android_host") }
        private const val NOTIFICATION_ID = 1
        private const val CHANNEL_ID = "warp-terminal"
        private const val LOG_TAG = "WarpTerminal"
        private const val PTY_OUTPUT_TAG = "WarpTerminal:PtyOutput"

        const val ACTION_SPAWN  = "dev.warp.mobile.PTY_SPAWN"
        const val ACTION_WRITE  = "dev.warp.mobile.PTY_WRITE"
        const val ACTION_RESIZE = "dev.warp.mobile.PTY_RESIZE"
        const val ACTION_KILL   = "dev.warp.mobile.PTY_KILL"
        const val ACTION_OUTPUT = "dev.warp.mobile.PTY_OUTPUT"
    }

    private val ptyManager = PtyManager()
    private val serviceJob = SupervisorJob()
    private val scope = CoroutineScope(Dispatchers.IO + serviceJob)
    private val readJobs = mutableMapOf<String, Job>()

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            when (intent.action) {
                ACTION_SPAWN  -> handleSpawn(intent)
                ACTION_WRITE  -> handleWrite(intent)
                ACTION_RESIZE -> handleResize(intent)
                ACTION_KILL   -> handleKill(intent)
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        // M3-S06: extract APK-bundled warp assets to the app's internal files
        // directory on first launch. zsh_body.sh is bundled as
        // assets/warp/zsh_body.sh and extracted to
        // /data/data/dev.warp.mobile/files/warp/zsh_body.sh so PTY context
        // (and eventually M5 Termux zsh) can source it directly from the
        // filesystem.
        //
        // Refs:
        //   https://developer.android.com/reference/android/content/res/AssetManager
        //   (AssetManager.open / copyTo pattern)
        extractWarpAssets()

        // M4-S06: write our zsh runtime-config override to $PREFIX/etc/.zshenv.
        // Codex M4-S03 round-7+8 finding: zsh 5.9 IGNORES the inherited
        // MODULE_PATH env var (reinitializes module_path from compile-time
        // default which still points at /data/data/com.termux/...). The
        // canonical fix is shell-array assignment in $ZDOTDIR/.zshenv. We
        // also strip any stale com.termux entries that survived in fpath
        // via `${fpath:#/data/data/com.termux/*}` glob filter.
        // Idempotent: only writes if usr/ is present (M4-S05 extracted) AND
        // the file is missing or has stale content.
        writeWarpZshenv()
        val filter = IntentFilter().apply {
            addAction(ACTION_SPAWN)
            addAction(ACTION_WRITE)
            addAction(ACTION_RESIZE)
            addAction(ACTION_KILL)
        }
        registerReceiver(receiver, filter, RECEIVER_NOT_EXPORTED)
        Log.i(LOG_TAG, "WarpTerminalService created, receivers registered")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        ensureForeground()
        // Dispatch off main thread to avoid ANR on blocking JNI calls
        val action = intent?.action
        val intentCopy = intent
        if (intentCopy != null) scope.launch {
            when (action) {
                ACTION_SPAWN  -> handleSpawn(intentCopy)
                ACTION_WRITE  -> handleWrite(intentCopy)
                ACTION_RESIZE -> handleResize(intentCopy)
                ACTION_KILL   -> handleKill(intentCopy)
            }
        }
        return START_STICKY
    }

    private fun ensureForeground() {
        val nm = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        if (nm.getNotificationChannel(CHANNEL_ID) == null) {
            nm.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Warp Terminal", NotificationManager.IMPORTANCE_LOW)
            )
        }
        val notification = NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_menu_manage)
            .setContentTitle("Warp terminal")
            .setOngoing(true)
            .build()
        startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
    }

    // ── M3-S06: asset extraction ─────────────────────────────────────────────

    /**
     * Extract APK-bundled warp assets to the app's internal files dir.
     *
     * Currently extracts:
     *   assets/warp/zsh_body.sh → filesDir/warp/zsh_body.sh
     *
     * The file is skipped if it already exists (idempotent). Extraction happens
     * at service creation so the path is available before any PTY session
     * spawns. A PTY shell can `cat` the file at:
     *   /data/data/dev.warp.mobile/files/warp/zsh_body.sh
     *
     * Hook execution is DEFERRED to M5 Termux: the S24 Ultra ships only mksh;
     * zsh_body.sh's precmd/preexec hooks require zsh which ships in M5.
     *
     * Refs:
     *   https://developer.android.com/reference/android/content/res/AssetManager
     *   https://wiki.termux.com/wiki/Zsh (zsh availability in Termux; M5 target)
     *   AGPL-3.0 §5: source-form script shipped verbatim in APK satisfies §5
     *     (corresponding source = the script itself; no additional obligation).
     */
    private fun extractWarpAssets() {
        val warpDir = File(filesDir, "warp")
        val target = File(warpDir, "zsh_body.sh")
        val temp = File(warpDir, "zsh_body.sh.tmp")
        // Read canonical bytes from the asset stream. `openFd` would let us
        // skip the buffer but it only works for uncompressed assets; AGP
        // compresses .sh files by default. The file is 66KB so buffering
        // is cheap, and reading once gives us the size for the integrity check.
        val canonicalBytes = try {
            assets.open("warp/zsh_body.sh").use { it.readBytes() }
        } catch (e: Exception) {
            Log.e(LOG_TAG, "failed to read zsh_body.sh from APK assets: ${e.message}", e)
            return
        }
        val expectedSize = canonicalBytes.size.toLong()
        // Codex M3-S06 round-1 finding #1: validate existing file by size
        // before treating as already-extracted. A partial copy from a prior
        // launch (process killed mid-write) leaves a truncated file that
        // would otherwise be skipped forever.
        if (target.exists() && target.length() == expectedSize) {
            Log.i(LOG_TAG, "zsh_body.sh already extracted at ${target.absolutePath} (${target.length()} bytes); skipping")
            return
        }
        if (target.exists()) {
            Log.w(LOG_TAG, "zsh_body.sh size mismatch (target=${target.length()} expected=$expectedSize); re-extracting")
        }
        // Atomic-replace pattern: write to a same-dir temp file, verify size,
        // then rename. If any step fails the temp is deleted and target stays
        // either absent (first launch) or untouched (corrupt-detect re-extract).
        warpDir.mkdirs()
        if (temp.exists()) temp.delete()
        try {
            temp.writeBytes(canonicalBytes)
            if (temp.length() != expectedSize) {
                throw java.io.IOException("size mismatch after write: temp=${temp.length()} expected=$expectedSize")
            }
            if (target.exists() && !target.delete()) {
                throw java.io.IOException("could not remove stale target ${target.absolutePath}")
            }
            if (!temp.renameTo(target)) {
                throw java.io.IOException("rename ${temp.absolutePath} → ${target.absolutePath} failed")
            }
            Log.i(LOG_TAG, "extracted zsh_body.sh to ${target.absolutePath} (${target.length()} bytes)")
        } catch (e: Exception) {
            temp.delete()
            Log.e(LOG_TAG, "failed to extract zsh_body.sh: ${e.message}", e)
        }
    }

    /**
     * M4-S06: write our canonical $ZDOTDIR/.zshenv.
     *
     * Why this file exists:
     *   - zsh 5.9 IGNORES inherited MODULE_PATH env var (verified by codex M4-S03
     *     round-7+8) — it reinitializes module_path from the COMPILE-TIME default
     *     which still points at /data/data/com.termux/lib/zsh/5.9 because Termux
     *     pre-built debs were compiled against the upstream prefix.
     *   - FPATH IS honored as env var (imported into fpath shell array), but
     *     zsh's compile-time default ALSO seeds fpath with stale com.termux/...
     *     entries that survive the env-var preset.
     *   - The canonical fix is shell-array assignment in $ZDOTDIR/.zshenv:
     *       module_path=(...)              # full replace
     *       fpath=(...new... ${fpath:#/data/data/com.termux/asterisk}) # filter stale
     *
     * Idempotency: only writes if usr/ has been extracted (M4-S05 done) AND
     * the file is missing or has different content. Safe to call on every
     * service startup.
     *
     * Format chosen: ZDOTDIR=$PREFIX/etc → zsh sources $PREFIX/etc/.zshenv
     * automatically when ZDOTDIR is set in spawn env. We control that env
     * (handleSpawn below) so the path is reliable.
     */
    private fun writeWarpZshenv() {
        val prefix = "${applicationInfo.dataDir}/files/usr"
        val zsh = File("$prefix/bin/zsh")
        if (!zsh.exists()) {
            Log.i(LOG_TAG, "writeWarpZshenv: $prefix/bin/zsh not present (M4-S05 not run yet); skipping")
            return
        }
        val zshenvPath = File("$prefix/etc/.zshenv")
        // Canonical content (per M4-S06 AC #6/#7 from prd.json round-7).
        // The HEREDOC-style multiline string is a single Kotlin string with
        // explicit \n; embedded $ are escaped for Kotlin (\$).
        val canonical = """
            |# Warp Mobile zsh env override (M4-S06).
            |# Generated by WarpTerminalService.writeWarpZshenv on app launch.
            |# DO NOT edit by hand — content is reproducible at every service start.
            |#
            |# Codex M4-S03 round-7+8 finding: zsh 5.9 ignores inherited MODULE_PATH
            |# env var (reinitializes module_path from compile-time default which
            |# points at /data/data/com.termux/lib/zsh/5.9). Canonical fix: shell-
            |# array assignment here. fpath gets the same treatment plus a glob
            |# filter to strip any stale com.termux/* entries that survived from
            |# the compile-time default + env var preset.
            |
            |# 1. module_path: full replace with dev.warp.mobile-rooted path.
            |module_path=(/data/data/dev.warp.mobile/files/usr/lib/zsh/5.9)
            |
            |# 2. fpath: prepend dev.warp.mobile entries; strip ANY stale
            |#    com.termux/* entries from whatever zsh seeded the array with.
            |fpath=(
            |    /data/data/dev.warp.mobile/files/usr/share/zsh/5.9/functions
            |    /data/data/dev.warp.mobile/files/usr/share/zsh/site-functions
            |    ${"$"}{fpath:#/data/data/com.termux/*}
            |)
            |
            |# 3. Sanity-check sentinel for M4-S06 acceptance verification:
            |#    `print -rl -- ${"$"}WARP_ZSHENV_LOADED` returns "1" iff this
            |#    file was sourced. M4-S10 acceptance test asserts this.
            |export WARP_ZSHENV_LOADED=1
        """.trimMargin().trimStart() + "\n"

        // Idempotent write: only update if content differs.
        val needsWrite = !zshenvPath.exists() || zshenvPath.readText() != canonical
        if (!needsWrite) {
            Log.i(LOG_TAG, "writeWarpZshenv: ${zshenvPath.absolutePath} already current; skipping")
            return
        }
        try {
            zshenvPath.parentFile?.mkdirs()
            zshenvPath.writeText(canonical)
            Log.i(LOG_TAG, "writeWarpZshenv: wrote ${zshenvPath.absolutePath} (${canonical.length} bytes)")
        } catch (e: Exception) {
            Log.e(LOG_TAG, "writeWarpZshenv: failed: ${e.message}", e)
        }
    }

    /**
     * M4-S06: build the spawn-time environment for the PTY child.
     *
     * Rust's pty.rs uses execve, NOT execvpe — so the env we pass IS the
     * complete env of the child process. We must include EVERYTHING the
     * child needs: PATH (for command lookup), HOME (for shell + git config
     * defaults), TMPDIR (for everything that writes /tmp).
     *
     * Per `.omc/prd.json` M4-S06 round-7 ACs:
     *   - HOME, ZDOTDIR (M4-S06 AC #2): standard env, work as zsh inherited
     *   - GIT_EXEC_PATH (round-7 AC #7): override git's compile-time
     *     /data/data/com.termux/files/usr/libexec/git-core default
     *   - TERMINFO, LOCPATH (round-7 AC #8): broader compile-time default
     *     coverage (terminal database + locale data)
     *   - SSL_CERT_FILE, SSL_CERT_DIR (M4-S07 round-7 AC): TLS CA path
     *     override (libcurl looks here before falling back to compile-time
     *     com.termux defaults)
     *
     * Note: MODULE_PATH/FPATH env vars NOT set — zsh ignores MODULE_PATH
     * and the compile-time fpath default carries stale com.termux entries
     * regardless. We rely on the $ZDOTDIR/.zshenv shell-array fix above.
     */
    private fun buildPrefixEnv(extra: Map<String, String> = emptyMap()): Map<String, String> {
        val prefix = "${applicationInfo.dataDir}/files/usr"
        val home = "${applicationInfo.dataDir}/files/home"
        // Make sure the home dir exists; first launch zsh will write history
        // and configs into it.
        File(home).mkdirs()
        File("$prefix/tmp").mkdirs()

        return buildMap {
            // PATH: $PREFIX/bin first so binaries from the bootstrap zip
            // shadow any system tools. /system/bin retained as fallback for
            // Android-only utilities (am, pm, settings, getprop) that may
            // be useful from the shell.
            put("PATH", "$prefix/bin:/system/bin")
            put("HOME", home)
            put("PREFIX", prefix)
            put("TERMUX_PREFIX", prefix) // termux-tools scripts read this
            put("TMPDIR", "$prefix/tmp")
            put("TERM", "xterm-256color")
            put("LANG", "en_US.UTF-8")
            put("LC_ALL", "en_US.UTF-8")
            put("SHELL", "$prefix/bin/zsh")

            // M4-S06 AC #6: ZDOTDIR points at $PREFIX/etc so zsh sources our
            // canonical .zshenv (written above by writeWarpZshenv).
            put("ZDOTDIR", "$prefix/etc")

            // M4-S06 AC #7: git compile-time exec-path default override.
            put("GIT_EXEC_PATH", "$prefix/libexec/git-core")
            put("GIT_TEMPLATE_DIR", "$prefix/share/git-core/templates")

            // M4-S06 AC #8: terminfo + locale.
            put("TERMINFO", "$prefix/share/terminfo")
            put("LOCPATH", "$prefix/share/locale")

            // M4-S07 round-7 AC #7: TLS CA path override (libcurl looks
            // here first; without this it falls back to the compile-time
            // com.termux default and certificate validation breaks).
            put("SSL_CERT_FILE", "$prefix/etc/tls/cert.pem")
            put("SSL_CERT_DIR", "$prefix/etc/tls/certs")

            // Caller-supplied overrides win (e.g., test scripts wanting a
            // specific TERM or TMPDIR for isolation).
            putAll(extra)
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        // Cancel coroutines FIRST so read loops stop before killAll closes fds (Fix #1)
        serviceJob.cancel()
        unregisterReceiver(receiver)
        ptyManager.killAll()
        Log.i(LOG_TAG, "WarpTerminalService destroyed, all PTY sessions killed")
    }

    override fun onBind(intent: Intent?): IBinder? = null

    // ── Intent handlers ──────────────────────────────────────────────────────

    private fun handleSpawn(intent: Intent) {
        val cmdId = intent.getStringExtra("cmd_id") ?: "default"
        // M4-S06: default program changes from /system/bin/sh to $PREFIX/bin/zsh
        // (closes M3-S06 hook execution deferral — zsh now available post-M4-S05
        // extraction). Falls back to /system/bin/sh if zsh isn't extracted yet
        // (M4-S05 not run, e.g., race during first-launch coroutine startup);
        // the M3 PTY pipeline still works on mksh for that fallback.
        val prefixZsh = "${applicationInfo.dataDir}/files/usr/bin/zsh"
        val defaultProgram = if (File(prefixZsh).exists()) prefixZsh else "/system/bin/sh"
        val program = intent.getStringExtra("program") ?: defaultProgram
        val args    = intent.getStringArrayExtra("args") ?: emptyArray()
        val cmd     = intent.getStringExtra("cmd")

        val (resolvedProgram, resolvedArgs) = if (cmd != null) {
            // Convenience: --es cmd "bash" maps to /system/bin/bash with no args
            val bin = if (cmd.startsWith("/")) cmd else "/system/bin/$cmd"
            Pair(bin, emptyArray<String>())
        } else {
            Pair(program, args)
        }

        // M4-S06: build the canonical $PREFIX env. Caller can override any
        // var via --esa env_pairs ["K=V","K2=V2"] or via Intent extra
        // env_<KEY>=value. (Defer custom env override to M4-S07 if needed.)
        val env = buildPrefixEnv()

        Log.i(LOG_TAG, "PTY_SPAWN cmdId=$cmdId program=$resolvedProgram args=${resolvedArgs.toList()} env_keys=${env.keys.sorted()}")
        // Fix #2: PtyManager.spawn() kills existing session before replacing
        val ok = ptyManager.spawn(cmdId, resolvedProgram, resolvedArgs, env)
        if (ok) startReadLoop(cmdId)
    }

    private fun handleWrite(intent: Intent) {
        val cmdId = intent.getStringExtra("cmd_id") ?: "default"
        // Decoder precedence:
        //   1. `data` byte-array extra (rare via adb; intra-process broadcasts).
        //   2. `data_b64` base64-encoded string extra (M3-S08 — sidesteps the
        //      `am broadcast` argument parser that treats any value containing
        //      a `-l` / `-a`-shaped token as a flag).
        //   3. `data` plain string extra (legacy / simple ASCII).
        val data: ByteArray = intent.getByteArrayExtra("data")
            ?: intent.getStringExtra("data_b64")?.let {
                try {
                    android.util.Base64.decode(it, android.util.Base64.DEFAULT)
                } catch (e: IllegalArgumentException) {
                    Log.e(LOG_TAG, "PTY_WRITE: invalid data_b64 ${e.message}")
                    return
                }
            }
            ?: intent.getStringExtra("data")?.let {
                val s = it.replace("\\n", "\n").replace("\\r", "\r")
                val bytes = s.toByteArray()
                if (bytes.isNotEmpty() && bytes.last() != '\n'.code.toByte()) bytes + "\n".toByteArray() else bytes
            }
            ?: return
        Log.d(LOG_TAG, "PTY_WRITE cmdId=$cmdId bytes=${data.size}")
        ptyManager.write(cmdId, data)
    }

    private fun handleResize(intent: Intent) {
        val cmdId = intent.getStringExtra("cmd_id") ?: "default"
        val rows  = intent.getIntExtra("rows", 24).toShort()
        val cols  = intent.getIntExtra("cols", 80).toShort()
        Log.i(LOG_TAG, "PTY_RESIZE cmdId=$cmdId rows=$rows cols=$cols")
        ptyManager.resize(cmdId, rows, cols)
    }

    private fun handleKill(intent: Intent) {
        val cmdId = intent.getStringExtra("cmd_id") ?: "default"
        Log.i(LOG_TAG, "PTY_KILL cmdId=$cmdId")
        readJobs.remove(cmdId)?.cancel()
        ptyManager.kill(cmdId)
    }

    // ── PTY read loop ────────────────────────────────────────────────────────

    private fun startReadLoop(cmdId: String) {
        // Fix #2: cancel existing read job before replacing to avoid competing loops
        readJobs.remove(cmdId)?.cancel()
        val job = scope.launch {
            val buf = ByteArray(4096)
            while (isActive) {
                // Fix #1: use readDirect (non-locking) to avoid deadlock with kill()
                val chunk = ptyManager.readDirect(cmdId, buf.size) ?: break
                if (chunk.isEmpty()) {
                    kotlinx.coroutines.delay(20)
                    continue
                }
                // M3-S04: forward each PTY chunk to the Rust terminal model.
                // Fire-and-forget: the model handles its own dirty bit. The
                // MainActivity Choreographer per-vsync callback consumes the
                // bit and pushes a frame.
                //
                // Refs:
                //   * Choreographer.FrameCallback / View.invalidate dirty
                //     pattern: https://developer.android.com/reference/android/view/Choreographer.FrameCallback
                //   * JNI byte-array passing perf guidance:
                //     https://developer.android.com/training/articles/perf-jni
                val ingested = NativeBridge.terminalInputBytes(cmdId, chunk)
                if (ingested < 0) {
                    Log.w(LOG_TAG, "terminalInputBytes failed cmdId=$cmdId chunk_size=${chunk.size}")
                }

                val text = chunk.toString(Charsets.UTF_8)
                // Log each line tagged WarpTerminal:PtyOutput as expected by test drivers
                for (line in text.lines()) {
                    if (line.isNotEmpty()) {
                        Log.i(PTY_OUTPUT_TAG, line)
                    }
                }
                // Fix #4: restrict PTY_OUTPUT to our own package (no data leak)
                val out = Intent(ACTION_OUTPUT).apply {
                    setPackage(packageName)
                    putExtra("cmd_id", cmdId)
                    putExtra("data", chunk)
                }
                sendBroadcast(out)
            }
            Log.i(LOG_TAG, "read loop ended cmdId=$cmdId")
        }
        readJobs[cmdId] = job
    }
}
