package app.marquee.android

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.CalendarCopy
import app.marquee.core.model.CalendarDay
import app.marquee.core.model.CalendarItem
import app.marquee.core.session.CoreSession
import app.marquee.core.views.CalendarView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.PosterTile
import app.marquee.design.RowHeader
import app.marquee.design.SecondaryButton
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.laneHeight
import kotlinx.coroutines.delay

// Calendar (plans/android-design-v2.md 5.4): episodes of the library's series from a week back to
// a month ahead, by air date, as on the web. One layout for TV and phone.

class CalendarHolder(
    session: CoreSession,
) : ViewModel() {
    val view: CalendarView = session.calendar(CalendarCopy.localDay())

    /** Set by the first key press or touch: from then on the screen stops placing today at the top. */
    var acted = false

    override fun onCleared() = view.close()
}

@Composable
fun CalendarScreen(
    session: CoreSession,
    onOpen: (CalendarItem) -> Unit,
    onDiscover: () -> Unit,
) {
    val holder =
        viewModel<CalendarHolder>(
            key = "calendar",
            factory = viewModelFactory { initializer { CalendarHolder(session) } },
        )
    val view = holder.view
    KeepToday(view)
    val snapshot = view.snapshot
    val today = snapshot?.today ?: CalendarCopy.localDay()
    val days = snapshot?.days.orEmpty()
    // The first day on or after today goes to the top. Series keep arriving while the schedule
    // loads and add days above it, so it is placed again on every change until the viewer acts.
    val target = days.indexOfFirst { it.date >= today }
    val list = rememberLazyListState()
    LaunchedEffect(target, days.size) {
        if (target > 0 && !holder.acted) list.scrollToItem(HEAD_ITEMS + target)
    }
    LazyColumn(
        Modifier
            .fillMaxSize()
            .onPreviewKeyEvent {
                holder.acted = true
                false
            }.pointerInput(Unit) {
                awaitEachGesture {
                    awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                    holder.acted = true
                }
            },
        state = list,
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        // HEAD_ITEMS counts this one item: everything above the days.
        item(key = "head") { Head(view, today, onDiscover) }
        items(days, key = { it.date }) { Day(it, today, onOpen) }
    }
}

/** The items of the list above the first day. */
private const val HEAD_ITEMS = 1

/** The title, the window it covers, and every state of the schedule. */
@Composable
private fun Head(
    view: CalendarView,
    today: String,
    onDiscover: () -> Unit,
) {
    val snapshot = view.snapshot
    val status = view.status
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        BrowseHeader("Coming up", "Calendar")
        MqText(
            CalendarCopy.lede(today),
            modifier = Modifier.padding(horizontal = Marquee.safe.side),
            role = TextRole.Callout,
            color = Marquee.colors.muted,
        )
        when {
            status is ViewStatus.Failed -> {
                Note("Calendar could not open: ${status.reason}", failed = true)
            }

            snapshot == null -> {
                Note("Loading the schedule")
            }

            else -> {
                if (snapshot.pending > 0) Note("Loading the schedule of ${snapshot.pending} series")
                if (snapshot.failed.isNotEmpty()) Note(CalendarCopy.failed(snapshot.failed), failed = true)
                if (snapshot.noSchedule.isNotEmpty()) Note(CalendarCopy.noSchedule(snapshot.noSchedule))
                if (snapshot.days.isEmpty() && snapshot.pending == 0) {
                    Note("Nothing airs in these weeks for the series in your library.")
                    SecondaryButton(
                        "Find series to follow",
                        onDiscover,
                        Modifier.padding(horizontal = Marquee.safe.side),
                        icon = Icons.Compass,
                    )
                }
            }
        }
    }
}

/** One day: its name and date, then its episodes as 16:9 tiles. */
@Composable
private fun Day(
    day: CalendarDay,
    today: String,
    onOpen: (CalendarItem) -> Unit,
) {
    val metrics = Marquee.tiles
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        RowHeader(
            CalendarCopy.dayName(day.date, today),
            CalendarCopy.dayDate(day.date),
            Modifier.padding(horizontal = Marquee.safe.side),
        )
        LazyRow(
            Modifier.height(metrics.laneHeight(TileShape.Landscape)),
            contentPadding = PaddingValues(horizontal = Marquee.safe.side),
            horizontalArrangement = Arrangement.spacedBy(metrics.gap),
        ) {
            items(day.items, key = { it.videoId }) { item ->
                PosterTile(
                    title = item.name,
                    imageUrl = item.still ?: item.poster,
                    fallbackImageUrl = item.poster.takeIf { item.still != null },
                    onClick = { onOpen(item) },
                    modifier = Modifier.padding(top = metrics.pad),
                    shape = TileShape.Landscape,
                    caption = CalendarCopy.episodeLine(item),
                    watched = item.watched,
                )
            }
        }
    }
}

/**
 * The core works from the local date, so it is sent again at every local midnight, on every return
 * to the app (which also refreshes a stale schedule), and when the clock or the time zone changes.
 */
@Composable
private fun KeepToday(view: CalendarView) {
    val context = LocalContext.current
    var changed by remember { mutableIntStateOf(0) }
    LifecycleResumeEffect(view) {
        view.setToday(CalendarCopy.localDay())
        onPauseOrDispose { }
    }
    DisposableEffect(context) {
        val receiver =
            object : BroadcastReceiver() {
                override fun onReceive(
                    context: Context,
                    intent: Intent,
                ) {
                    changed++
                }
            }
        val filter =
            IntentFilter().apply {
                addAction(Intent.ACTION_TIMEZONE_CHANGED)
                addAction(Intent.ACTION_TIME_CHANGED)
                addAction(Intent.ACTION_DATE_CHANGED)
            }
        // System broadcasts reach a receiver that is not exported.
        ContextCompat.registerReceiver(context, receiver, filter, ContextCompat.RECEIVER_NOT_EXPORTED)
        onDispose { context.unregisterReceiver(receiver) }
    }
    // A change sends the new date and sets the timer again from the new clock.
    LaunchedEffect(view, changed) {
        if (changed > 0) view.setToday(CalendarCopy.localDay())
        while (true) {
            delay(CalendarCopy.millisToMidnight())
            view.setToday(CalendarCopy.localDay())
        }
    }
}
