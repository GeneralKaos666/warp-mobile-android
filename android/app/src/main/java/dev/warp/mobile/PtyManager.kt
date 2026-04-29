package dev.warp.mobile

import android.util.Log

class PtyManager {

    private val sessions = mutableMapOf<String, Long>()

    @Synchronized
    fun spawn(cmdId: String, program: String, args: Array<String>, env: Map<String, String>): Boolean {
        // Kill any existing session with same cmdId before replacing (Fix #2: no orphan)
        sessions[cmdId]?.let { oldPtr ->
            NativeBridge.ptyKill(oldPtr)
            Log.i(LOG_TAG, "spawn: killed existing session cmdId=$cmdId ptr=$oldPtr")
        }
        val envFlat = env.map { (k, v) -> "$k=$v" }.toTypedArray()
        val ptr = NativeBridge.ptySpawn(program, args, envFlat)
        return if (ptr != 0L) {
            sessions[cmdId] = ptr
            Log.i(LOG_TAG, "spawn ok cmdId=$cmdId ptr=$ptr")
            true
        } else {
            sessions.remove(cmdId)
            Log.e(LOG_TAG, "spawn failed cmdId=$cmdId")
            false
        }
    }

    @Synchronized
    fun write(cmdId: String, data: ByteArray): Int {
        val ptr = sessions[cmdId] ?: return -1
        return NativeBridge.ptyWrite(ptr, data)
    }

    // Fix #1: read is NOT @Synchronized — libc::read blocks and holding the
    // monitor while blocked would deadlock kill()/killAll() on the same monitor.
    // The fd is per-session; concurrent read+kill is safe because close(fd)
    // causes read to return EBADF immediately.
    fun readDirect(cmdId: String, maxBytes: Int): ByteArray? {
        val ptr = synchronized(this) { sessions[cmdId] } ?: return null
        return NativeBridge.ptyRead(ptr, maxBytes)
    }

    @Synchronized
    fun resize(cmdId: String, rows: Short, cols: Short): Int {
        val ptr = sessions[cmdId] ?: return -1
        return NativeBridge.ptyResize(ptr, rows, cols)
    }

    @Synchronized
    fun kill(cmdId: String) {
        val ptr = sessions.remove(cmdId) ?: return
        NativeBridge.ptyKill(ptr)
        Log.i(LOG_TAG, "kill ok cmdId=$cmdId")
    }

    @Synchronized
    fun killAll() {
        for ((cmdId, ptr) in sessions) {
            NativeBridge.ptyKill(ptr)
            Log.i(LOG_TAG, "killAll: killed cmdId=$cmdId")
        }
        sessions.clear()
    }

    companion object {
        private const val LOG_TAG = "WarpTerminal"
    }
}
