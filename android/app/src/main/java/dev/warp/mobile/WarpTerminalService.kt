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
import kotlinx.coroutines.launch

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
        // Route PTY control intents forwarded by PtyBroadcastReceiver
        when (intent?.action) {
            ACTION_SPAWN  -> handleSpawn(intent)
            ACTION_WRITE  -> handleWrite(intent)
            ACTION_RESIZE -> handleResize(intent)
            ACTION_KILL   -> handleKill(intent)
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
        val cmdId   = intent.getStringExtra("cmd_id") ?: "default"
        val program = intent.getStringExtra("program") ?: "/system/bin/sh"
        val args    = intent.getStringArrayExtra("args") ?: emptyArray()
        val cmd     = intent.getStringExtra("cmd")

        val (resolvedProgram, resolvedArgs) = if (cmd != null) {
            // Convenience: --es cmd "bash" maps to /system/bin/bash with no args
            val bin = if (cmd.startsWith("/")) cmd else "/system/bin/$cmd"
            Pair(bin, emptyArray<String>())
        } else {
            Pair(program, args)
        }

        Log.i(LOG_TAG, "PTY_SPAWN cmdId=$cmdId program=$resolvedProgram args=${resolvedArgs.toList()}")
        // Fix #2: PtyManager.spawn() kills existing session before replacing
        val ok = ptyManager.spawn(cmdId, resolvedProgram, resolvedArgs, emptyMap())
        if (ok) startReadLoop(cmdId)
    }

    private fun handleWrite(intent: Intent) {
        val cmdId = intent.getStringExtra("cmd_id") ?: "default"
        val data  = intent.getByteArrayExtra("data")
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
            while (true) {
                // Fix #1: use readDirect (non-locking) to avoid deadlock with kill()
                val chunk = ptyManager.readDirect(cmdId, buf.size) ?: break
                if (chunk.isEmpty()) {
                    kotlinx.coroutines.delay(20)
                    continue
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
