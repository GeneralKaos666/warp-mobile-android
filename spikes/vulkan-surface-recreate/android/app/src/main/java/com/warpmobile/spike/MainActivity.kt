package com.warpmobile.spike

import android.os.Bundle
import android.os.SystemClock
import android.util.Log
import android.view.Choreographer
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity(), SurfaceHolder.Callback {

    private var waitingForRecovery = false

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (waitingForRecovery) {
                val ts = SystemClock.uptimeMillis()
                // Parsed by run-vulkan-spike.sh: "firstNonStaleFrame_ts=<ms>"
                Log.i(TAG, "firstNonStaleFrame_ts=$ts")
                waitingForRecovery = false
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val surfaceView = SurfaceView(this)
        surfaceView.holder.addCallback(this)
        setContentView(surfaceView)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        nativeSurfaceCreated(holder.surface)
        if (waitingForRecovery) {
            Choreographer.getInstance().postFrameCallback(frameCallback)
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        val ts = SystemClock.uptimeMillis()
        // Parsed by run-vulkan-spike.sh: "surfaceDestroyed_ts=<ms>"
        Log.i(TAG, "surfaceDestroyed_ts=$ts")
        waitingForRecovery = true
        nativeSurfaceDestroyed()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        nativeSurfaceChanged(holder.surface, width, height)
    }

    private external fun nativeSurfaceCreated(surface: Surface)
    private external fun nativeSurfaceDestroyed()
    private external fun nativeSurfaceChanged(surface: Surface, width: Int, height: Int)

    companion object {
        private const val TAG = "VulkanSpike"

        init {
            System.loadLibrary("vulkan_surface_recreate")
        }
    }
}
