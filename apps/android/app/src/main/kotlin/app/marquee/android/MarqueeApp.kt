package app.marquee.android

import android.app.Application
import android.os.SystemClock
import android.util.Log
import app.marquee.core.NativeCore
import app.marquee.core.ffi.CoreListener
import app.marquee.core.ffi.MarqueeCore
import java.util.concurrent.CompletableFuture

/** P0 spike: opens the core on a boot thread as early as possible. Replaced by `CoreSession` in P1. */
class MarqueeApp : Application() {
    val inbox = java.util.concurrent.LinkedBlockingQueue<Pair<Long, String>>()
    val core: CompletableFuture<MarqueeCore> = CompletableFuture()
    var openMs: Long = -1
    var keepAttached: Boolean = true

    override fun onCreate() {
        super.onCreate()
        // P0 measurement switch: `adb shell run-as app.marquee.android touch files/p0-nodetach`.
        keepAttached = !filesDir.resolve("p0-nodetach").exists()
        val started = SystemClock.elapsedRealtimeNanos()
        Thread({
            try {
                val listener = object : CoreListener {
                    override fun onMessage(json: String) {
                        inbox.put(System.nanoTime() to json)
                    }
                }
                val dir = filesDir.resolve("marquee")
                val deviceId = 0x1234_5678_9ABCUL
                val opened = NativeCore.open(this, dir, deviceId, listener, keepAttached)
                openMs = (SystemClock.elapsedRealtimeNanos() - started) / 1_000_000
                Log.i(TAG, "core open ${openMs} ms")
                core.complete(opened)
            } catch (t: Throwable) {
                Log.e(TAG, "core open failed", t)
                core.completeExceptionally(t)
            }
        }, "marquee-boot").start()
    }

    companion object {
        const val TAG = "marquee-p0"
    }
}
