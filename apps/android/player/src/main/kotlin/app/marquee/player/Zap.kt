package app.marquee.player

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.marquee.core.ZapCopy
import app.marquee.core.model.Dial
import app.marquee.design.Chip
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.RoundButton
import app.marquee.design.SecondaryButton
import app.marquee.design.SegmentTrack
import app.marquee.design.TextRole
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

/**
 * A zap play's ways on (plans/zap.md 2.3); null for an ordinary play. The name and [line] are for What is this?
 * alone: everything else reads the controller, which hides them until it is [PlaybackController.revealed].
 */
data class ZapControls(
    val name: String,
    /** "S2:E5 · Episode name", or null for a film. */
    val line: String?,
    val dial: Dial,
    val onAgain: () -> Unit,
    val onDial: (Dial) -> Unit,
    val onFromStart: () -> Unit,
    val onOpenTitle: () -> Unit,
)

/**
 * Whether a zap play did not play (plans/zap.md 2.3), which zaps on by itself: before its first frame it failed, or
 * the core offers Choose another version. After the first frame a stall shows the bar's Zap again, a failure its card.
 */
fun zapMissed(
    failed: Boolean,
    chooseAnother: Boolean,
    firstFrame: Boolean,
): Boolean = !firstFrame && (failed || chooseAnother)

/**
 * A dial press (plans/zap.md 2.3): cancels the press before it ([previous]), [write]s the step as the profile's
 * `zapDial`, then [then] zaps again. Only the last of fast presses zaps, so the saved dial and the dealt pick agree.
 * A write that fails calls [failed] and zaps nothing.
 */
fun CoroutineScope.dialPress(
    previous: Job?,
    write: suspend () -> Unit,
    failed: (Exception) -> Unit,
    then: () -> Unit,
): Job {
    previous?.cancel()
    return launch {
        try {
            write()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            failed(e)
            return@launch
        }
        then()
    }
}

/** Subtitle rows as shown: until the name is [revealed], without the release text, which carries the title. */
fun shownRows(
    rows: List<TrackChoice>,
    revealed: Boolean,
): List<TrackChoice> = if (revealed) rows else rows.map { it.copy(release = null) }

/**
 * The dial (plans/zap.md 2.3): Same, Near and Far as a segmented control, each spoken with what it means. It changes
 * on a press only (OK on TV), never on focus.
 */
@Composable
fun ZapDial(
    dial: Dial,
    onDial: (Dial) -> Unit,
    modifier: Modifier = Modifier,
    chipModifier: Modifier = Modifier,
) {
    SegmentTrack(modifier.testTag(ZAP_DIAL_TAG).semantics { contentDescription = ZapCopy.DIAL }) {
        for ((step, word, spoken) in ZapCopy.DIALS) {
            Chip(word, step == dial, { if (step != dial) onDial(step) }, chipModifier, inTrack = true, spoken = spoken)
        }
    }
}

/**
 * What is this? (plans/zap.md 2.3): a small glass panel over the picture with the name and, for an episode, its
 * line, then Start from the beginning and Open title. Its first button takes the focus on open (TV); Close and Back
 * close it.
 */
@Composable
fun ZapPanel(
    zap: ZapControls,
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.m)
    val first = remember { FocusRequester() }
    LaunchedEffect(tv) {
        if (!tv) return@LaunchedEffect
        withFrameNanos { }
        runCatching { first.requestFocus() }
    }
    Column(
        modifier
            .testTag(ZAP_PANEL_TAG)
            .widthIn(max = if (tv) 720.dp else 420.dp)
            .clip(shape)
            .background(colors.glass)
            .border(1.dp, colors.edge, shape)
            .padding(if (tv) 28.dp else 16.dp),
        verticalArrangement = Arrangement.spacedBy(if (tv) 12.dp else 8.dp),
    ) {
        Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f, fill = false), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                MqText(zap.name, role = TextRole.Title3, maxLines = 2)
                zap.line?.let { MqText(it, role = TextRole.Callout, color = colors.muted, maxLines = 2) }
            }
            // TV closes with Back alone: a remote never needs a Close to reach.
            if (!tv) RoundButton(Icons.X, "Close", onClose)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(if (tv) 16.dp else 8.dp)) {
            SecondaryButton(ZapCopy.FROM_START, zap.onFromStart, icon = Icons.RotateCcw, focusRequester = first)
            SecondaryButton(ZapCopy.OPEN_TITLE, zap.onOpenTitle, icon = Icons.Info)
        }
    }
}

/** For tests: the dial and the panel. */
const val ZAP_DIAL_TAG = "zap-dial"
const val ZAP_PANEL_TAG = "zap-panel"
