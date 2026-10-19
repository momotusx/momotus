package app.marquee.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.DefaultsCopy
import app.marquee.core.model.ImportSnapshot
import app.marquee.core.model.ImportSummary
import app.marquee.core.session.CoreSession
import app.marquee.core.views.ImportView
import app.marquee.core.views.ViewStatus
import app.marquee.design.CodeText
import app.marquee.design.FactsGrid
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.Qr
import app.marquee.design.SecondaryButton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.openLink

// Import from Stremio (plans/android-design-v2.md slice 13, the web's import screen): a code to
// enter at link.stremio.com, then the core brings addons, library and progress over once.

/** Opening the view starts a sign-in; a new attempt opens a new view. */
class ImportHolder(
    private val session: CoreSession,
) : ViewModel() {
    var view: ImportView by mutableStateOf(session.import())
        private set

    fun retry() {
        view.close()
        view = session.import()
    }

    override fun onCleared() = view.close()
}

@Composable
fun ImportScreen(
    session: CoreSession,
    onDone: () -> Unit,
    /** First run goes on to the recommended addons, not straight Home. */
    toDefaults: Boolean = false,
) {
    val holder =
        viewModel<ImportHolder>(
            key = "import",
            factory = viewModelFactory { initializer { ImportHolder(session) } },
        )
    val view = holder.view
    val tv = Marquee.form == Form.Tv
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                BrowseHeader("Settings", "Import from Stremio")
                MqText(
                    "Brings your addons, library, Continue Watching and watched marks over once. Nothing is sent " +
                        "back to Stremio." + if (tv) " Nothing is typed on this screen." else "",
                    modifier = Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
            }
        }
        item(key = "state") {
            Column(Modifier.padding(horizontal = Marquee.safe.side)) {
                Attempt(view, holder::retry, onDone, toDefaults)
            }
        }
    }
}

@Composable
private fun Attempt(
    view: ImportView,
    onRetry: () -> Unit,
    onDone: () -> Unit,
    toDefaults: Boolean,
) {
    val status = view.status
    if (status is ViewStatus.Failed) {
        StateMessage(
            "Import could not start",
            status.reason,
            icon = Icons.CircleAlert,
            actionLabel = "Try again",
            onAction = onRetry,
        )
        return
    }
    when (val s = view.snapshot) {
        null, ImportSnapshot.Starting -> {
            StatusWord("Starting the sign-in", Status.Warn)
        }

        is ImportSnapshot.WaitingForLink -> {
            Waiting(s, onRetry)
        }

        is ImportSnapshot.Importing -> {
            StatusWord(s.step, Status.Warn)
        }

        is ImportSnapshot.Done -> {
            Done(s.summary, onDone, toDefaults)
        }

        ImportSnapshot.Expired -> {
            StateMessage(
                "The code expired",
                "Codes last a few minutes. Get a new one and enter it on the other device.",
                icon = Icons.Clock,
                actionLabel = "Get a new code",
                onAction = onRetry,
            )
        }

        is ImportSnapshot.Failed -> {
            StateMessage(
                "Import stopped",
                s.reason,
                icon = Icons.CircleAlert,
                actionLabel = "Get a new code",
                onAction = onRetry,
            )
        }
    }
}

/** "link.stremio.com" from "https://link.stremio.com/". */
private fun hostOf(url: String) = url.removePrefix("https://").removePrefix("http://").trimEnd('/')

/**
 * The code and where to enter it. A TV shows the QR beside the steps (nothing is typed on it); a
 * phone signs in on itself: the link opens here, and the QR is only for another device.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Waiting(
    w: ImportSnapshot.WaitingForLink,
    onRetry: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val context = LocalContext.current
    val host = hostOf(w.linkUrl)
    var other by rememberSaveable { mutableStateOf(false) }
    val qr = @Composable { Qr(w.qrUrl ?: w.linkUrl, "QR code that opens $host", if (tv) 320.dp else 220.dp) }
    val waiting = @Composable { StatusWord("Waiting for the code to be entered", Status.Warn) }
    if (tv) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xl),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            qr()
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                Step("1.", "Scan the code, or open $host on a phone or computer where you are signed in to Stremio.")
                Step("2.", "Enter this code there:")
                CodeText(w.code)
                waiting()
                SecondaryButton("Get a new code", onRetry, icon = Icons.RefreshCw)
            }
        }
    } else {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            Step("1.", "Open $host where you are signed in to Stremio.")
            PrimaryButton("Open $host", { openLink(context, w.linkUrl) }, icon = Icons.Link)
            Step("2.", "Enter this code there:")
            CodeText(w.code)
            waiting()
            // Wraps on a narrow phone rather than cutting a label.
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                SecondaryButton("Use another device", { other = !other }, icon = Icons.QrCode, selected = other)
                SecondaryButton("Get a new code", onRetry, icon = Icons.RefreshCw)
            }
            if (other) qr()
        }
    }
}

@Composable
private fun Step(
    number: String,
    text: String,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        MqText(number, role = TextRole.Callout, weight = FontWeight.Bold)
        MqText(text, role = TextRole.Callout, modifier = Modifier.widthIn(max = 720.dp))
    }
}

/** What came over, and the way home. */
@Composable
private fun Done(
    summary: ImportSummary,
    onDone: () -> Unit,
    toDefaults: Boolean,
) {
    val colors = Marquee.colors
    // On a TV the remote lands on the way on as soon as the import is done.
    val home = remember { FocusRequester() }
    val tv = Marquee.form == Form.Tv
    LaunchedEffect(Unit) {
        if (tv) {
            withFrameNanos { }
            runCatching { home.requestFocus() }
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MqIcon(Icons.Check, tint = colors.ok)
            MqText("Imported from Stremio", role = TextRole.Title3)
        }
        FactsGrid(
            listOf(
                "Addons" to "${summary.addonsImported}",
                "Titles" to "${summary.titles}",
                "In progress" to "${summary.inProgress}",
                "Watched" to "${summary.watched}",
            ),
        )
        if (summary.addonsSkipped.isNotEmpty()) {
            MqText(
                "Not imported: ${summary.addonsSkipped.joinToString("; ")}.",
                role = TextRole.Caption,
                color = colors.muted,
            )
        }
        if (summary.seriesUnmatched > 0) {
            MqText(
                "${summary.seriesUnmatched} series kept their watched marks unmatched: their episode lists could not be loaded.",
                role = TextRole.Caption,
                color = colors.muted,
            )
        }
        if (toDefaults) {
            PrimaryButton(DefaultsCopy.CONTINUE, onDone, icon = Icons.ChevronRight, focusRequester = home)
        } else {
            PrimaryButton("Go to Home", onDone, icon = Icons.House, focusRequester = home)
        }
    }
}
