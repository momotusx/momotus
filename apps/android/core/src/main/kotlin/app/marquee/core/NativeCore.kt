package app.marquee.core

import android.content.Context
import android.system.Os
import app.marquee.core.ffi.CoreListener
import app.marquee.core.ffi.MarqueeCore
import app.marquee.core.ffi.uniffiCallbackInterfaceCoreListener
import com.sun.jna.CallbackThreadInitializer
import com.sun.jna.Native
import java.io.File

/** Loads the Rust library and opens the core. Blocking: call from a background thread. */
object NativeCore {
    private const val LIBRARY = "marquee_core"

    fun open(
        context: Context,
        dataDir: File,
        deviceId: ULong,
        listener: CoreListener,
        keepCallbackThreadsAttached: Boolean = true,
        logFilter: String? = null,
    ): MarqueeCore {
        // The core installs its logcat subscriber (tag `marquee`) on the first open and reads the
        // filter then: `debug`, or `info,mq_sync=trace`. Unset means info.
        if (logFilter != null) Os.setenv("MARQUEE_LOG", logFilter, true)
        // Order matters: the JNI symbol lives in the same library JNA registers below.
        System.loadLibrary(LIBRARY)
        PlatformVerifier.init(context.applicationContext)
        if (keepCallbackThreadsAttached) {
            // Core threads stay attached to the JVM after the first callback instead of
            // attaching and detaching around every message (architecture 4.2).
            Native.setCallbackThreadInitializer(
                uniffiCallbackInterfaceCoreListener.onMessage,
                CallbackThreadInitializer(true, false, "marquee-core-callback"),
            )
        }
        dataDir.mkdirs()
        return MarqueeCore.open(dataDir.path, "default", deviceId, listener)
    }
}
