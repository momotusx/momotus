package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.runtime.snapshots.Snapshot
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.views.PlaybackView
import app.marquee.player.PlaybackController
import app.marquee.player.SubtitleDelay
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.robolectric.Shadows.shadowOf
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext

/**
 * A play for the bar's tests (plans/player-bar.md): a controller on [PlayingPlayer], attached to a scripted core whose
 * stream lists an English and a Turkish subtitle (core tracks, which can be timed); their files load from [SRT].
 */
internal class BarPlay(
    trakt: String = "off",
    /** The English track's language name as the core gives it (a long one for the fit). */
    english: String = "English",
) {
    private val main =
        object : CoroutineDispatcher() {
            private val handler = Handler(Looper.getMainLooper())

            override fun dispatch(
                context: CoroutineContext,
                block: Runnable,
            ) {
                handler.post(block)
            }
        }
    private val scope = CoroutineScope(SupervisorJob() + main)
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-core") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    val player = PlayingPlayer()
    val controller =
        PlaybackController(
            player,
            scope,
            "Sintel",
            null,
            null,
            SystemClock.elapsedRealtime(),
            SubtitleDelay(),
            false,
        ) { SRT.toByteArray() }
    private val snapshot =
        SNAPSHOT
            .replace("\"trakt\":\"off\"", "\"trakt\":\"$trakt\"")
            .replace("\"languageName\":\"English\"", "\"languageName\":\"$english\"")

    /** Attaches the core's view and waits for the stream to load. */
    fun attach(waitUntil: (String, () -> Boolean) -> Unit) {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") """{"sub":7,"snapshot":$snapshot}""" else "{}"
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        controller.attach(PlaybackView(client, scope, JsonObject(emptyMap())).also { it.open() })
        waitUntil("the stream loads") {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            controller.stage == PlaybackController.Stage.Buffering
        }
    }

    /** Turns the core's English track on, as its chip would: a subtitle that can be timed. */
    fun english() = controller.selectText(controller.text.first { it.language == "en" }.id)

    fun close() {
        controller.release()
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }
}

private const val SRT = "1\n00:00:01,000 --> 00:59:00,000\nWho are you\n"

private const val SNAPSHOT =
    """{"phase":"ready","url":"http://127.0.0.1:7780/video.mp4","key":"k1","attempt":1,"manifest":false,""" +
        """"subtitlesPending":false,"trakt":"off","subtitles":[""" +
        """{"url":"http://127.0.0.1:7781/en.srt","lang":"eng","language":"en","languageName":"English",""" +
        """"nativeName":"English","name":"CAKES, WEB 1080p","source":"OpenSubtitles v3","forced":false,""" +
        """"hearingImpaired":false},""" +
        """{"url":"http://127.0.0.1:7781/tr.srt","lang":"tur","language":"tr","languageName":"Turkish",""" +
        """"nativeName":"Türkçe","name":"Turkish 1","source":"SubSense","forced":false,"hearingImpaired":false}]}"""
