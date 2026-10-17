package app.marquee.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.unit.dp
import app.marquee.core.client.CoreError
import app.marquee.core.route.Route
import app.marquee.core.session.CoreHealth
import app.marquee.core.session.CoreSession
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

private const val CINEMETA = "https://v3-cinemeta.strem.io/manifest.json"

/** Puts focus on [requester] once the screen is laid out, for D-pad devices (a failed request is harmless). */
@Composable
private fun AutoFocus(requester: FocusRequester) {
    LaunchedEffect(requester) {
        withFrameNanos { }
        runCatching { requester.requestFocus() }
    }
}

/** A new user's first screen: one action that installs the standard catalog. */
@Composable
fun OnboardingScreen(
    session: CoreSession,
    onDone: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val focus = remember { FocusRequester() }
    AutoFocus(focus)
    Column(
        Modifier.fillMaxSize().padding(horizontal = Marquee.safe.side, vertical = Marquee.safe.top),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m, Alignment.CenterVertically),
    ) {
        MqText("Marquee", role = TextRole.Hero)
        MqText(
            "Movies and series from the add-ons you choose.",
            role = TextRole.Headline,
            color = Marquee.colors.muted,
            modifier = Modifier.widthIn(max = 900.dp),
        )
        MqText(
            "Start fresh installs Cinemeta, the standard catalog. You can add more add-ons at any time.",
            role = TextRole.Body,
            color = Marquee.colors.muted,
            modifier = Modifier.widthIn(max = 900.dp),
        )
        PrimaryButton(
            label = if (busy) "Installing" else "Start fresh",
            enabled = !busy,
            focusRequester = focus,
            onClick = {
                busy = true
                error = null
                scope.launch {
                    try {
                        session.ops.installAddon(CINEMETA)
                        onDone()
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: CoreError) {
                        error = e.message
                        busy = false
                    }
                }
            },
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

/** A top-level place whose screen is built in a later phase. */
@Composable
fun ComingSoonScreen(name: String) {
    Box(Modifier.fillMaxSize().padding(horizontal = Marquee.safe.side), contentAlignment = Alignment.CenterStart) {
        StateMessage(title = name, body = "This screen arrives in a later build.")
    }
}

/** The page of a title; the real one arrives in the next phase. */
@Composable
fun TitlePlaceholder(route: Route.Title) {
    Box(Modifier.fillMaxSize().padding(horizontal = Marquee.safe.side), contentAlignment = Alignment.CenterStart) {
        StateMessage(
            title = "Title page",
            body = "${route.kind} ${route.id}. The title page arrives in the next build.",
        )
    }
}

/** Covers the app while the core is not running, with a way back when it does not come up by itself. */
@Composable
fun CoreStoppedOverlay(session: CoreSession) {
    val health = session.health
    if (health != CoreHealth.Restarting && health !is CoreHealth.Stopped) return
    val focus = remember { FocusRequester() }
    Box(
        Modifier.fillMaxSize().background(Marquee.colors.bg).padding(horizontal = Marquee.safe.side),
        contentAlignment = Alignment.CenterStart,
    ) {
        if (health is CoreHealth.Stopped) {
            AutoFocus(focus)
            StateMessage(
                title = "Marquee stopped",
                body = "The engine behind Marquee stopped working. Restart it to carry on.",
                icon = Icons.CircleAlert,
                actionLabel = "Restart",
                onAction = session::restart,
                actionFocus = focus,
            )
        } else {
            StateMessage(title = "Restarting", body = "Marquee is starting its engine again.")
        }
    }
}
