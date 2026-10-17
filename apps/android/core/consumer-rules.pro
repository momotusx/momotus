# JNA looks classes and fields up by reflection.
-keep class com.sun.jna.* { *; }
-keepclassmembers class * extends com.sun.jna.* { public *; }
-dontwarn java.awt.*

# UniFFI generated bindings and the JNI init the Rust library exports a symbol for.
-keep class app.marquee.core.ffi.** { *; }
-keep class app.marquee.core.PlatformVerifier { *; }

# The verifier's JVM component is reached from Rust through JNI.
-keep, includedescriptorclasses class org.rustls.platformverifier.** { *; }
