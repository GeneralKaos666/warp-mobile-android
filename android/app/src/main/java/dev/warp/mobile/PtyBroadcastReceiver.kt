package dev.warp.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

// Exported receiver so `adb shell am broadcast` can reach WarpTerminalService.
// Forwards PTY control intents to the service which holds the PtyManager.
class PtyBroadcastReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val forward = Intent(intent).apply { setClass(context, WarpTerminalService::class.java) }
        context.startForegroundService(forward)
    }
}
