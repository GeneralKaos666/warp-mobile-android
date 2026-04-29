package dev.warp.mobile

import android.content.Intent
import android.os.Bundle
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val tv = TextView(this).apply {
            text = NativeBridge.ping()
            textSize = 18f
            setPadding(32, 32, 32, 32)
        }
        setContentView(tv)
        startForegroundService(Intent(this, WarpTerminalService::class.java))
    }
}
