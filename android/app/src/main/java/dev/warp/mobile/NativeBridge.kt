package dev.warp.mobile

object NativeBridge {
    init {
        System.loadLibrary("warp_mobile_android_host")
    }

    external fun ping(): String

    external fun ptySpawn(program: String, args: Array<String>, envFlat: Array<String>): Long
    external fun ptyRead(ptr: Long, maxBytes: Int): ByteArray?
    external fun ptyWrite(ptr: Long, data: ByteArray): Int
    external fun ptyResize(ptr: Long, rows: Short, cols: Short): Int
    external fun ptyKill(ptr: Long)
}
