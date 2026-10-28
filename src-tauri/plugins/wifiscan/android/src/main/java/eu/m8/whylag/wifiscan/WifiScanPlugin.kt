package eu.m8.whylag.wifiscan

import android.Manifest
import android.app.Activity
import android.content.Context
import android.net.wifi.WifiManager
import android.os.Build
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class ScanArgs {
    var trigger: Boolean = false
}

@TauriPlugin(
    permissions = [
        Permission(strings = [Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION], alias = "location"),
    ],
)
class WifiScanPlugin(private val activity: Activity) : Plugin(activity) {
    private val wifi: WifiManager
        get() = activity.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager

    @Command
    fun scan(invoke: Invoke) {
        if (getPermissionState("location") != PermissionState.GRANTED) {
            requestPermissionForAlias("location", invoke, "afterPermission")
            return
        }
        doScan(invoke)
    }

    @PermissionCallback
    private fun afterPermission(invoke: Invoke) {
        if (getPermissionState("location") == PermissionState.GRANTED) {
            doScan(invoke)
        } else {
            invoke.reject("locationPermission")
        }
    }

    @Suppress("DEPRECATION")
    private fun doScan(invoke: Invoke) {
        val args = invoke.parseArgs(ScanArgs::class.java)
        val manager = wifi
        // Android limits apps to a few scans per two minutes; the cached
        // results are still returned when a request is refused.
        val started = if (args.trigger) manager.startScan() else false
        val networks = JSArray()
        for (r in manager.scanResults) {
            val n = JSObject()
            n.put("bssid", r.BSSID ?: "")
            n.put("ssid", r.SSID ?: "")
            n.put("level", r.level)
            n.put("frequency", r.frequency)
            n.put("channelWidth", r.channelWidth)
            n.put("centerFreq0", r.centerFreq0)
            n.put("capabilities", r.capabilities ?: "")
            n.put("timestampUs", r.timestamp)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                n.put("wifiStandard", r.wifiStandard)
            }
            networks.put(n)
        }
        val result = JSObject()
        result.put("networks", networks)
        result.put("scanStarted", started)
        result.put("enabled", manager.isWifiEnabled)
        val info = manager.connectionInfo
        if (info != null && info.networkId != -1 && info.bssid != null) {
            val c = JSObject()
            c.put("bssid", info.bssid)
            c.put("ssid", (info.ssid ?: "").trim('"'))
            c.put("rssi", info.rssi)
            c.put("linkSpeedMbps", info.linkSpeed)
            c.put("frequency", info.frequency)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                c.put("txLinkSpeedMbps", info.txLinkSpeedMbps)
                c.put("rxLinkSpeedMbps", info.rxLinkSpeedMbps)
            }
            result.put("connected", c)
        }
        // Milliseconds since boot, to age the results like the desktop scanners do.
        result.put("nowUs", android.os.SystemClock.elapsedRealtime() * 1000)
        invoke.resolve(result)
    }
}
