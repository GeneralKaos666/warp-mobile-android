package dev.warp.mobile

object NativeBridge {
    init {
        System.loadLibrary("warp_mobile_android_host")
    }

    external fun ping(): String
}
