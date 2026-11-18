package app.marquee.android

import android.util.Log
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.ZapCopy
import app.marquee.core.client.CoreError
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.ZapPick
import app.marquee.core.route.Route
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.RoundButton
import app.marquee.design.SecondaryButton
import app.marquee.design.TextRole
import app.marquee.player.LoadingLayer
import app.marquee.player.TitleMark
import kotlinx.coroutines.CancellationException

/** No addon has a catalog to zap from: the words and the action of every page with no source. */
private val NO_SOURCE = StreamChoiceCopy.EMPTY.getValue(EmptyKind.NoSource)

/** Why a zap has nothing to play: the core's two codes, and three zaps in a row that did not play. */
private enum class ZapMiss { NoAddons, NothingCameUp, NothingPlayed }

/**
 * The zap route (plans/zap.md 2.2): the loading screen with "Zap" for the mark, while the core picks; then [onPick]
 * replaces it with the pick's player. A miss says so with Zap again (Settings when no addon has a catalog) and Back.
 * At [ZapCopy.TRIES] zaps in a row that did not play it asks the core nothing and says "Nothing played.".
 */
@Composable
fun ZapScreen(
    route: Route.Zap,
    session: CoreSession,
    onPick: (Route.Player) -> Unit,
    onSettings: () -> Unit,
    onBack: () -> Unit,
) {
    // Zap again here deals anew in place, with the count of tries back at 0.
    var deal by remember { mutableIntStateOf(0) }
    val tries = ZapCopy.tries(route, deal)
    var miss by remember { mutableStateOf(if (ZapCopy.spent(tries)) ZapMiss.NothingPlayed else null) }
    LaunchedEffect(route, deal) {
        if (ZapCopy.spent(tries)) return@LaunchedEffect
        miss = null
        val pick: ZapPick =
            try {
                session.ops.zap(route.kind)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // The miss's cause, for a walk or an error report: the screen words every failure alike.
                Log.w(MarqueeApp.TAG, "zap missed: ${(e as? CoreError)?.code} ${e.message}")
                miss = if ((e as? CoreError)?.code == ZapCopy.NO_ADDONS) ZapMiss.NoAddons else ZapMiss.NothingCameUp
                return@LaunchedEffect
            }
        onPick(ZapCopy.player(pick, route.kind, tries))
    }
    val tv = Marquee.form == Form.Tv
    Box(Modifier.fillMaxSize().background(Color.Black)) {
        LoadingLayer(title = ZapCopy.ZAP, mark = TitleMark.Name, backdrop = null, fill = { 0f }, lines = emptyList())
        miss?.let { why ->
            val (words, primary) =
                when (why) {
                    ZapMiss.NoAddons -> NO_SOURCE.first to NO_SOURCE.second!!
                    ZapMiss.NothingCameUp -> ZapCopy.NOTHING_CAME_UP to ZapCopy.AGAIN
                    ZapMiss.NothingPlayed -> ZapCopy.NOTHING_PLAYED to ZapCopy.AGAIN
                }
            ZapMissCard(
                words,
                primary,
                onPrimary = if (why == ZapMiss.NoAddons) onSettings else ({ deal += 1 }),
                onBack = onBack,
            )
        }
        // The phone's way out while it loads; TV has the remote's Back.
        if (!tv && miss == null) {
            RoundButton(
                Icons.ArrowLeft,
                "Back",
                onBack,
                Modifier.windowInsetsPadding(WindowInsets.displayCutout).padding(16.dp),
            )
        }
    }
}

/** A zap with nothing to play, over the black stage: the words, then the way on (first focus on TV) and Back. */
@Composable
private fun ZapMissCard(
    words: String,
    primary: String,
    onPrimary: () -> Unit,
    onBack: () -> Unit,
) {
    val focus = remember { FocusRequester() }
    // TV: the way on takes the focus, so OK answers at once.
    val tv = Marquee.form == Form.Tv
    LaunchedEffect(words) {
        if (!tv) return@LaunchedEffect
        withFrameNanos { }
        runCatching { focus.requestFocus() }
    }
    Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.72f)), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
            MqText(
                words,
                Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                role = TextRole.Title2,
                textAlign = TextAlign.Center,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                val icon = if (primary == ZapCopy.AGAIN) Icons.Shuffle else null
                PrimaryButton(primary, onPrimary, icon = icon, focusRequester = focus)
                SecondaryButton("Back", onBack, icon = Icons.ArrowLeft)
            }
        }
    }
}
