package app.marquee.core

import android.content.Context
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
    ): MarqueeCore {
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
