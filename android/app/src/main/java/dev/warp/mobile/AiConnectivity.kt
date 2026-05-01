package dev.warp.mobile

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.util.Log
import java.util.concurrent.CopyOnWriteArrayList

/**
 * M6-S05: lightweight network-state wrapper for AI features.
 *
 * Watches [ConnectivityManager] for the default network's reachability
 * and exposes:
 *   - [isOnline]: synchronous probe (cheap; reads cached state)
 *   - [register]: subscribe to onChange callbacks (for UI grey-out
 *     when network drops mid-stream)
 *
 * Why a thin wrapper rather than calling ConnectivityManager directly
 * from each consumer:
 *   - registerDefaultNetworkCallback requires API 24+; project minSdk
 *     31 covers it but we still wrap for testability
 *   - State changes arrive on Binder threads; we marshal to a
 *     CopyOnWriteArrayList<Listener> for thread-safe iteration
 *   - Single source of truth means the AccessoryRow `💡` / `🤖`
 *     buttons + SettingsActivity Test button + future ghost-text IME
 *     hook all read the SAME online/offline value
 *
 * Usage:
 *   val cn = AiConnectivity.get(context)
 *   if (cn.isOnline()) { /* show AI buttons enabled */ }
 *   cn.register(myListener)  // onChange callbacks
 */
object AiConnectivity {
    private const val LOG_TAG = "WarpAiConnectivity"

    interface Listener {
        fun onConnectivityChanged(online: Boolean)
    }

    @Volatile private var instance: AiConnectivity.State? = null

    @Synchronized
    fun get(context: Context): State {
        instance?.let { return it }
        val s = State(context.applicationContext)
        s.start()
        instance = s
        return s
    }

    class State internal constructor(private val app: Context) {
        @Volatile private var online: Boolean = true  // optimistic default
        private val listeners = CopyOnWriteArrayList<Listener>()
        private val cm = app.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager

        private val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                update(true, "available")
            }
            override fun onLost(network: Network) {
                update(false, "lost")
            }
            override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
                // VALIDATED capability is the strict "this network can
                // reach the public Internet" check — no captive portal,
                // no DNS-only-but-no-route. Update the cached state on
                // every cap change so a captive-portal-detected → real-
                // internet transition flips us back online cleanly.
                val validated = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) &&
                                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                update(validated, "caps validated=$validated")
            }
        }

        internal fun start() {
            // Seed initial state. activeNetwork can be null briefly
            // during boot; treat null as offline (UI shows banner).
            val active = cm.activeNetwork
            val caps = active?.let { cm.getNetworkCapabilities(it) }
            online = caps != null &&
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
                caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
            Log.i(LOG_TAG, "initial online=$online (active=$active)")

            // Subscribe to default-network changes. NetworkRequest.Builder
            // with NET_CAPABILITY_INTERNET filters out e.g. WiFi-Direct
            // peer connections that don't carry public Internet.
            val req = NetworkRequest.Builder()
                .addCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET)
                .build()
            cm.registerNetworkCallback(req, callback)
        }

        private fun update(newOnline: Boolean, reason: String) {
            if (online == newOnline) return
            online = newOnline
            Log.i(LOG_TAG, "online=$newOnline ($reason); ${listeners.size} listener(s)")
            for (l in listeners) {
                try {
                    l.onConnectivityChanged(newOnline)
                } catch (t: Throwable) {
                    Log.e(LOG_TAG, "listener threw: ${t.message}")
                }
            }
        }

        fun isOnline(): Boolean = online

        fun register(l: Listener) {
            listeners.add(l)
            // Fire immediately with current state so caller doesn't
            // need to also call isOnline() right after register.
            l.onConnectivityChanged(online)
        }

        fun unregister(l: Listener) {
            listeners.remove(l)
        }
    }
}
