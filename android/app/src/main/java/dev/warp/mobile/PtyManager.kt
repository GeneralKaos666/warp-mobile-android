package dev.warp.mobile

import android.util.Log

class PtyManager {

    private val sessions = mutableMapOf<String, Long>()

    @Synchronized
    fun spawn(cmdId: String, program: String, args: Array<String>, env: Map<String, String>): Boolean {
        val envFlat = env.map { (k, v) -> "$k=$v" }.toTypedArray()
        val ptr = NativeBridge.ptySpawn(program, args, envFlat)
        return if (ptr != 0L) {
            sessions[cmdId] = ptr
            Log.i(LOG_TAG, "spawn ok cmdId=$cmdId ptr=$ptr")
            true
        } else {
            Log.e(LOG_TAG, "spawn failed cmdId=$cmdId")
            false
        }
    }

    @Synchronized
    fun write(cmdId: String, data: ByteArray): Int {
        val ptr = sessions[cmdId] ?: return -1
        return NativeBridge.ptyWrite(ptr, data)
    }

    @Synchronized
    fun read(cmdId: String, maxBytes: Int): ByteArray? {
        val ptr = sessions[cmdId] ?: return null
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
