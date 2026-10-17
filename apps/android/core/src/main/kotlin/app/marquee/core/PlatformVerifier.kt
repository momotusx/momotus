package app.marquee.core

import android.content.Context

/**
 * The Rust core verifies TLS through the platform certificate verifier, which calls back into
 * the JVM. The symbol lives in `libmarquee_core.so` (crates/mq-ffi, module `android`). Load the
 * library first, then call [init] once with the application context, before opening the core.
 */
object PlatformVerifier {
    @JvmStatic
    external fun init(context: Context)
}
