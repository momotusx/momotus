package app.marquee.android

import android.Manifest
import android.app.Activity
import android.app.AlarmManager
import android.app.KeyguardManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import android.content.pm.PackageManager
import android.hardware.biometrics.BiometricManager.Authenticators
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.os.SystemClock
import android.view.WindowManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import app.marquee.core.SyncRules
import app.marquee.core.client.CoreLog
import app.marquee.design.readQr
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.suspendCancellableCoroutine
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.coroutines.resume

// What sync needs from the device: who is there (the screen lock), the clipboard, and the camera.

/** How asking who is there ended. */
enum class Owner { Confirmed, Cancelled, NoLock }

/**
 * The system's screen-lock prompt (fingerprint, face, or the device's PIN, pattern or password)
 * before removing a device, reporting one lost or making a kit (plans/core-protocol.md). A device
 * with no screen lock has nothing to ask with: [secure] is false, and the page decides (a phone
 * refuses, a TV asks a second time).
 */
class OwnerCheck internal constructor(
    private val context: Context,
    private val legacy: suspend (String) -> Boolean,
) {
    val secure: Boolean
        get() = context.getSystemService(KeyguardManager::class.java)?.isDeviceSecure == true

    suspend fun confirm(title: String): Owner {
        if (!secure) return Owner.NoLock
        return try {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
                if (legacy(title)) Owner.Confirmed else Owner.Cancelled
            } else {
                prompt(title)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // A prompt that cannot show confirms nothing.
            CoreLog.w("owner check did not show: ${e.javaClass.simpleName}")
            Owner.Cancelled
        }
    }

    private suspend fun prompt(title: String): Owner =
        suspendCancellableCoroutine { done ->
            val cancel = CancellationSignal()
            val builder = BiometricPrompt.Builder(context).setTitle(title)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                builder.setAllowedAuthenticators(Authenticators.BIOMETRIC_WEAK or Authenticators.DEVICE_CREDENTIAL)
            } else {
                @Suppress("DEPRECATION")
                builder.setDeviceCredentialAllowed(true)
            }
            builder.build().authenticate(
                cancel,
                ContextCompat.getMainExecutor(context),
                object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                        if (done.isActive) done.resume(Owner.Confirmed)
                    }

                    override fun onAuthenticationError(
                        errorCode: Int,
                        errString: CharSequence,
                    ) {
                        if (done.isActive) done.resume(Owner.Cancelled)
                    }
                },
            )
            done.invokeOnCancellation { cancel.cancel() }
        }
}

/** The owner check for this screen; before Android 9 it goes through the lock screen's own confirm page. */
@Composable
fun rememberOwnerCheck(): OwnerCheck {
    val context = LocalContext.current
    val waiting = remember { arrayOfNulls<CompletableDeferred<Boolean>>(1) }
    val launcher =
        rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
            waiting[0]?.complete(it.resultCode == Activity.RESULT_OK)
        }
    return remember(context) {
        OwnerCheck(context) { title ->
            val keyguard = context.getSystemService(KeyguardManager::class.java)

            @Suppress("DEPRECATION")
            val intent = keyguard?.createConfirmDeviceCredentialIntent(title, null) ?: return@OwnerCheck false
            val answer = CompletableDeferred<Boolean>()
            waiting[0] = answer
            launcher.launch(intent)
            answer.await()
        }
    }
}

/** The clipboard, for a pairing code (public, no secret in it) and a recovery kit (secret). */
object SyncClipboard {
    private const val KIT_LABEL = "Marquee recovery kit"

    fun copyCode(
        context: Context,
        code: String,
    ) {
        context
            .getSystemService(
                ClipboardManager::class.java,
            )?.setPrimaryClip(ClipData.newPlainText("Marquee pairing code", code))
    }

    /**
     * A kit, marked sensitive so the system's copy preview and keyboards that honour it do not show
     * it. The page says to paste it into a password manager at once, and clears it when it closes.
     */
    fun copyKit(
        context: Context,
        kit: String,
    ) {
        val clip = ClipData.newPlainText(KIT_LABEL, kit)
        clip.description.extras =
            PersistableBundle().apply {
                val key =
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        ClipDescription.EXTRA_IS_SENSITIVE
                    } else {
                        "android.content.extra.IS_SENSITIVE"
                    }
                putBoolean(key, true)
            }
        context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(clip)
        later(context.applicationContext)
    }

    /**
     * About a minute after a copy the kit leaves the clipboard: a timer while the app runs, and an alarm
     * that runs even when the system ended the app meanwhile.
     */
    private fun later(app: Context) {
        main.removeCallbacks(timer)
        timer = Runnable { clearKit(app, onTimer = true) }
        main.postDelayed(timer, CLEAR_MS)
        app.getSystemService(AlarmManager::class.java)?.setAndAllowWhileIdle(
            AlarmManager.ELAPSED_REALTIME,
            SystemClock.elapsedRealtime() + CLEAR_MS,
            alarm(app),
        )
    }

    private val main = Handler(Looper.getMainLooper())
    private var timer = Runnable {}

    private fun alarm(app: Context) =
        PendingIntent.getBroadcast(
            app,
            0,
            Intent(app, KitClipReceiver::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    /**
     * Clears the clipboard when it still holds the kit. Only the app in front can read the clipboard: the
     * timer, which may run behind another app, clears what it cannot read (ponytail: it may then clear
     * something copied within that minute, as password managers do).
     */
    fun clearKit(
        context: Context,
        onTimer: Boolean = false,
    ) {
        val board = context.getSystemService(ClipboardManager::class.java) ?: return
        val label = board.primaryClipDescription?.label
        if (label != KIT_LABEL && !(onTimer && label == null)) return
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            board.clearPrimaryClip()
        } else {
            board.setPrimaryClip(ClipData.newPlainText("", ""))
        }
        // Cleared: neither timer may clear what the user copies next.
        main.removeCallbacks(timer)
        context.applicationContext.getSystemService(AlarmManager::class.java)?.cancel(alarm(context.applicationContext))
    }

    private const val CLEAR_MS = 60_000L
}

/** The alarm half of [SyncClipboard]'s minute: the app may no longer run when it fires. */
class KitClipReceiver : BroadcastReceiver() {
    override fun onReceive(
        context: Context,
        intent: Intent,
    ) = SyncClipboard.clearKit(context, onTimer = true)
}

/** Keeps the page out of screenshots, screen recordings and the recents thumbnail while it shows. */
@Composable
fun SecureWindow() {
    val context = LocalContext.current
    DisposableEffect(context) {
        val window = context.findActivity()?.window
        window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        onDispose { window?.clearFlags(WindowManager.LayoutParams.FLAG_SECURE) }
    }
}

private tailrec fun Context.findActivity(): Activity? =
    when (this) {
        is Activity -> this
        is ContextWrapper -> baseContext.findActivity()
        else -> null
    }

fun Context.hasCamera() = packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)

fun Context.cameraAllowed() =
    ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED

/**
 * The back camera, looking for a pairing code. [onCode] runs once, on the main thread, with the
 * first pairing code seen (any other QR is ignored); nothing runs after the scanner leaves the
 * screen, and the page checks again that it still wants a code. [onFailed] when the camera cannot
 * start.
 */
@Composable
fun QrScanner(
    onCode: (String) -> Unit,
    onFailed: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val view = remember { PreviewView(context) }
    val code by rememberUpdatedState(onCode)
    val failed by rememberUpdatedState(onFailed)
    AndroidView({ view }, modifier)
    DisposableEffect(owner) {
        val done = AtomicBoolean(false)
        val frames = Executors.newSingleThreadExecutor()
        val main = ContextCompat.getMainExecutor(context)
        val future = ProcessCameraProvider.getInstance(context)
        var provider: ProcessCameraProvider? = null
        future.addListener(
            {
                if (done.get()) return@addListener
                try {
                    val cameras = future.get().also { provider = it }
                    val preview = Preview.Builder().build().also { it.setSurfaceProvider(view.surfaceProvider) }
                    val analysis =
                        ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
                    analysis.setAnalyzer(frames) { image ->
                        image.use {
                            if (done.get()) return@use
                            val plane = it.planes[0]
                            val buffer = plane.buffer
                            val bytes = ByteArray(buffer.remaining()).also(buffer::get)
                            val text = readQr(bytes, it.width, it.height, plane.rowStride)
                            if (text != null && SyncRules.isPairingCode(text) && done.compareAndSet(false, true)) {
                                main.execute { code(text.trim()) }
                            }
                        }
                    }
                    cameras.unbindAll()
                    cameras.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
                } catch (e: Exception) {
                    done.set(true)
                    failed()
                }
            },
            main,
        )
        onDispose {
            done.set(true)
            provider?.unbindAll()
            frames.shutdown()
        }
    }
}
