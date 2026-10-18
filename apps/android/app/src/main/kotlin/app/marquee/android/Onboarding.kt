package app.marquee.android

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.marquee.core.client.CoreError
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

private const val CINEMETA = "https://v3-cinemeta.strem.io/manifest.json"

/**
 * A new user's first screen, one decision: how to begin. "I use Stremio" brings a library over (only
 * offered when [onImport] is given), "Start fresh" installs the standard catalog, and "I already use
 * Marquee" joins that device's sync account (only offered when [onPair] is given). "Browse
 * first" skips them. Nothing is typed here: adding a source is a later step, and on TV it never asks
 * for a keyboard.
 */
@Composable
fun OnboardingScreen(
    session: CoreSession,
    onDone: () -> Unit,
    onImport: (() -> Unit)? = null,
    onPair: (() -> Unit)? = null,
) {
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val first = remember { FocusRequester() }
    AutoFocus(first)

    fun startFresh() {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            try {
                // Installed and turned off (an empty Home sends people here): turning it on needs no
                // network, so it is tried first; an addon that is not installed refuses it.
                val turnedOn =
                    try {
                        session.ops.setAddonEnabled(CINEMETA, true)
                        true
                    } catch (e: CoreError) {
                        false
                    }
                if (!turnedOn) session.ops.installAddon(CINEMETA)
                onDone()
            } catch (e: CancellationException) {
                throw e
            } catch (e: CoreError) {
                error = e.message
                busy = false
            } catch (e: Exception) {
                error = "The answer could not be read"
                busy = false
            }
        }
    }

    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.systemBars)
            .padding(horizontal = Marquee.safe.side, vertical = Marquee.safe.top),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.l),
    ) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            MqText("Marquee", role = TextRole.Title2, modifier = Modifier.weight(1f))
            SkipLink("Browse first", onDone)
        }
        // Scrolls when it does not fit (a small phone at a large font scale); centered when it does.
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            Column(
                Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).heightIn(min = maxHeight),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.m, Alignment.CenterVertically),
            ) {
                MqText("How would you like to begin?", role = if (tv) TextRole.Title1 else TextRole.Title2)
                MqText(
                    "No account needed. Everything stays on this device until you turn on sync.",
                    role = TextRole.Body,
                    color = Marquee.colors.muted,
                    modifier = Modifier.widthIn(max = 720.dp),
                )
                val doors: @Composable (Modifier) -> Unit = { door ->
                    onImport?.let {
                        Door(
                            "I use Stremio",
                            "Bring your addons, library and Continue Watching over with a code.",
                            Icons.Link,
                            it,
                            door,
                            first,
                        )
                    }
                    Door(
                        if (busy) "Installing" else "Start fresh",
                        "Add the standard catalog, then add a source to play.",
                        Icons.Plus,
                        ::startFresh,
                        door,
                        first.takeIf { onImport == null },
                    )
                    onPair?.let {
                        Door(
                            "I already use Marquee",
                            "Join your other device's sync account with a code. Library and addons follow.",
                            Icons.QrCode,
                            it,
                            door,
                            null,
                        )
                    }
                }
                val single = onImport == null && onPair == null
                if (tv) {
                    // The doors share the width; a single one keeps the width of a card, not of the screen.
                    Row(
                        Modifier.height(IntrinsicSize.Min),
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.l),
                    ) {
                        doors(if (single) Modifier.widthIn(max = 640.dp) else Modifier.weight(1f).fillMaxHeight())
                    }
                } else {
                    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                        doors(Modifier.fillMaxWidth())
                    }
                }
                MqText(
                    "A source is an addon that finds a playable stream for a title. Marquee does not include any.",
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                    modifier = Modifier.widthIn(max = 720.dp),
                )
                error?.let {
                    StateMessage(
                        title = "Cinemeta could not be installed",
                        body = "$it. Check the network and try again.",
                        icon = Icons.CircleAlert,
                    )
                }
            }
        }
    }
}

/** One of the ways to begin: a card with an icon, a title and one line. */
@Composable
private fun Door(
    title: String,
    body: String,
    icon: ImageVector,
    onClick: () -> Unit,
    modifier: Modifier,
    focusRequester: FocusRequester?,
) {
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.l)
    Column(
        modifier
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .marqueeFocus(Marquee.radius.l)
            .clip(shape)
            .background(colors.surface)
            .border(2.dp, colors.control, shape)
            .focusClickable(onClick, Role.Button)
            .padding(Marquee.space.m),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        MqIcon(icon)
        MqText(title, role = TextRole.Title3, maxLines = 2)
        MqText(body, role = TextRole.Callout, color = colors.muted)
    }
}

/** A quiet text action in a screen's corner. */
@Composable
private fun SkipLink(
    label: String,
    onClick: () -> Unit,
) {
    Row(
        Modifier
            .heightIn(min = 48.dp)
            .marqueeFocus(Marquee.radius.m)
            .clip(RoundedCornerShape(Marquee.radius.m))
            .focusClickable(onClick, Role.Button)
            .padding(horizontal = Marquee.space.s),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqText(label, role = TextRole.Callout, weight = FontWeight.SemiBold, color = Marquee.colors.muted)
        MqIcon(Icons.ChevronRight, tint = Marquee.colors.muted)
    }
}
