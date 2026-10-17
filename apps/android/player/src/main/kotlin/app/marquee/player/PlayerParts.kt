package app.marquee.player

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.model.Attempt
import app.marquee.core.model.NextEpisode
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LocalForm
import app.marquee.design.LocalPalette
import app.marquee.design.LocalReducedMotion
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.RemoteImage
import app.marquee.design.SecondaryButton
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus

/** Over the picture the colors are the dark ones in both themes: light on dark, never the other way. */
@Composable
fun PlayerTheme(content: @Composable () -> Unit) =
    CompositionLocalProvider(LocalPalette provides Tokens.dark, content = content)

/** 3725 s as "1:02:05". */
fun clockText(ms: Long): String = Format.clock(ms / 1000)

/** The time of day the film ends at the current speed, or null while the length is unknown. */
fun endsAt(
    nowMs: Long,
    positionMs: Long,
    durationMs: Long,
    speed: Float,
): Long? =
    if (durationMs <=
        0
    ) {
        null
    } else {
        nowMs + ((durationMs - positionMs).coerceAtLeast(0) / speed.coerceAtLeast(0.1f)).toLong()
    }

/** A turning arc: the player waits for something (a source, the buffer). Still when animations are off. */
@Composable
fun Spinner(
    modifier: Modifier = Modifier,
    size: Dp = if (LocalForm.current == Form.Tv) 40.dp else 28.dp,
) {
    val color = Marquee.colors.text
    val turn =
        rememberInfiniteTransition(label = "spinner").animateFloat(
            initialValue = 0f,
            targetValue = 360f,
            animationSpec = infiniteRepeatable(tween(1200, easing = LinearEasing), RepeatMode.Restart),
            label = "turn",
        )
    val reduced = LocalReducedMotion.current
    Canvas(modifier.size(size)) {
        val stroke = size.toPx() / 9f
        rotate(if (reduced) 0f else turn.value) {
            drawArc(
                color = color,
                startAngle = 0f,
                sweepAngle = 270f,
                useCenter = false,
                topLeft = Offset(stroke / 2, stroke / 2),
                size = Size(this.size.width - stroke, this.size.height - stroke),
                style = Stroke(width = stroke, cap = StrokeCap.Round),
            )
        }
    }
}

/**
 * The position bar: what is buffered, what is played, and a knob. The values are read while
 * drawing, so a tick of the clock redraws this bar and recomposes nothing.
 *
 * @param preview where a D-pad scrub or a drag would land, drawn instead of the position.
 * @param emphasized focus is on the bar: the knob grows.
 */
@Composable
fun Scrubber(
    position: () -> Long,
    duration: () -> Long,
    buffered: () -> Long,
    modifier: Modifier = Modifier,
    preview: () -> Long? = { null },
    emphasized: Boolean = false,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val track = if (tv) 8.dp else 4.dp
    val knob = (if (tv) 11.dp else 7.dp) * (if (emphasized) 1.4f else 1f)
    Canvas(modifier.height(knob * 2)) {
        val total = duration().coerceAtLeast(1L).toFloat()

        fun at(ms: Long) = size.width * (ms / total).coerceIn(0f, 1f)
        val y = (size.height - track.toPx()) / 2
        val corner =
            androidx.compose.ui.geometry
                .CornerRadius(track.toPx() / 2)
        val bar = Size(size.width, track.toPx())
        drawRoundRect(colors.text.copy(alpha = 0.25f), Offset(0f, y), bar, corner)
        drawRoundRect(colors.text.copy(alpha = 0.42f), Offset(0f, y), Size(at(buffered()), track.toPx()), corner)
        val here = preview() ?: position()
        drawRoundRect(colors.text, Offset(0f, y), Size(at(here), track.toPx()), corner)
        drawCircle(colors.text, knob.toPx(), Offset(at(here), size.height / 2))
    }
}

/**
 * The audio or subtitles list: one row per track with its full name and what it is under it, the
 * chosen one filled and checked. On a TV focus starts on the chosen row.
 */
@Composable
fun TrackPanel(
    title: String,
    rows: List<TrackChoice>,
    onPick: (String) -> Unit,
    modifier: Modifier = Modifier,
    note: String? = null,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val requesters = remember(rows.size) { List(rows.size) { FocusRequester() } }
    if (tv) {
        LaunchedEffect(Unit) {
            withFrameNanos { }
            runCatching { requesters.getOrNull(rows.indexOfFirst { it.selected }.coerceAtLeast(0))?.requestFocus() }
        }
    }
    Column(
        modifier
            .then(if (tv) Modifier.width(640.dp) else Modifier)
            .clip(RoundedCornerShape(Marquee.radius.l))
            .background(colors.surface)
            .border(2.dp, colors.line, RoundedCornerShape(Marquee.radius.l))
            .padding(vertical = if (tv) 24.dp else 12.dp, horizontal = if (tv) 20.dp else 8.dp),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        MqText(title, role = TextRole.Headline, modifier = Modifier.padding(horizontal = 12.dp), maxLines = 1)
        LazyColumn(
            Modifier.heightIn(max = if (tv) 520.dp else 230.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            itemsIndexed(rows, key = { _, row -> row.id }) { index, row ->
                val shape = RoundedCornerShape(Marquee.radius.m)
                // The chosen row is a filled row with a check; the ring is for focus alone.
                Row(
                    Modifier
                        .fillMaxWidth()
                        .focusRequester(requesters[index])
                        // Focus stays in the panel while it is open: the ends of the list and the sides go nowhere.
                        .focusProperties {
                            left = FocusRequester.Cancel
                            right = FocusRequester.Cancel
                            if (index == 0) up = FocusRequester.Cancel
                            if (index == rows.lastIndex) down = FocusRequester.Cancel
                        }.marqueeFocus(Marquee.radius.m, scale = 1.02f)
                        .clip(shape)
                        .background(if (row.selected) colors.raised else Color.Transparent)
                        .semantics {
                            selected = row.selected
                            contentDescription = listOfNotNull(row.name, row.detail).joinToString(", ")
                        }.focusClickable({ onPick(row.id) }, Role.RadioButton)
                        .padding(horizontal = 16.dp, vertical = if (tv) 12.dp else 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(if (tv) 30.dp else 18.dp)) {
                        if (row.selected) MqIcon(Icons.Check, tint = colors.text)
                    }
                    Column {
                        MqText(row.name, role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 1)
                        row.detail?.let { MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 2) }
                    }
                }
            }
            if (note != null) {
                item(key = "note") {
                    MqText(note, role = TextRole.Caption, color = colors.muted, modifier = Modifier.padding(16.dp))
                }
            }
        }
    }
}

/**
 * What is happening before anything plays, or why nothing does: a turning mark and one line, or the
 * problem with one plain reason per source that failed, and the way on.
 */
@Composable
fun PlayerStatus(
    stage: PlaybackController.Stage,
    status: String?,
    problem: String?,
    attempts: List<Attempt>,
    onVersions: () -> Unit,
    onBack: () -> Unit,
    focus: FocusRequester,
    modifier: Modifier = Modifier,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        if (stage == PlaybackController.Stage.Failed) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                MqIcon(Icons.CircleAlert, tint = colors.bad, size = if (tv) 36.dp else 22.dp)
                MqText(problem ?: "Playback stopped", role = TextRole.Headline, weight = FontWeight.Medium)
            }
            for (attempt in attempts) {
                Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                    MqText(attempt.via, role = TextRole.Callout, weight = FontWeight.SemiBold)
                    MqText(attemptReason(attempt.error), role = TextRole.Callout, color = colors.muted)
                }
            }
            Row(
                horizontalArrangement = Arrangement.spacedBy(16.dp),
                modifier = Modifier.padding(top = Marquee.space.xs),
            ) {
                PrimaryButton("Choose another version", onVersions, icon = Icons.Layers, focusRequester = focus)
                SecondaryButton("Back", onBack)
            }
        } else if (status != null) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Spinner()
                MqText(status, role = TextRole.Headline, weight = FontWeight.Medium)
            }
        }
    }
}

/** The card that offers the next episode near the end: its picture, name, and a countdown when it starts by itself. */
@Composable
fun UpNextCard(
    next: NextEpisode,
    countdown: Int?,
    onPlay: () -> Unit,
    modifier: Modifier = Modifier,
    focus: FocusRequester? = null,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.l)
    Row(
        modifier
            .clip(shape)
            .background(colors.surface)
            .border(2.dp, colors.line, shape)
            .padding(if (tv) 24.dp else 12.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 28.dp else 12.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        RemoteImage(
            url = next.thumbnail,
            width = if (tv) 320.dp else 120.dp,
            height = if (tv) 180.dp else 68.dp,
            fallbackText = next.title,
            modifier = Modifier.clip(RoundedCornerShape(Marquee.radius.m)),
        )
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            MqText("Next episode", role = TextRole.Caption, color = colors.muted)
            MqText(
                "${Format.episodeLabel(next.season, next.episode)} ${next.title}".trim(),
                role = TextRole.Headline,
                maxLines = 2,
                modifier = Modifier.widthIn(max = if (tv) 420.dp else 200.dp),
            )
            if (countdown != null) MqText("Starts in $countdown", role = TextRole.Callout, color = colors.muted)
            PrimaryButton("Play now", onPlay, icon = Icons.SkipForward, focusRequester = focus)
        }
    }
}

/**
 * A round transport button. [label] is what a screen reader says (the icon alone carries no words).
 *
 * @param main the larger, filled one (play and pause).
 */
@Composable
fun RoundButton(
    icon: ImageVector,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    main: Boolean = false,
    focusRequester: FocusRequester? = null,
    /** A short text in the middle of the icon: the seconds a skip button jumps ("10"). */
    badge: String? = null,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val size = if (tv) (if (main) 104.dp else 84.dp) else (if (main) 60.dp else 48.dp)
    val glyph = if (tv) (if (main) 44.dp else 36.dp) else (if (main) 26.dp else 22.dp)
    Box(
        modifier
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .size(size)
            .marqueeFocus(size / 2)
            .clip(CircleShape)
            .background(if (main) colors.text else colors.text.copy(alpha = 0.14f))
            .semantics { contentDescription = label }
            .focusClickable(onClick, Role.Button),
        contentAlignment = Alignment.Center,
    ) {
        MqIcon(icon, tint = if (main) colors.bg else colors.text, size = glyph)
        if (badge != null) {
            MqText(badge, role = TextRole.Caption, weight = FontWeight.Bold, color = colors.text)
        }
    }
}

/**
 * What the player says when it reached the end: over the last frame, the way on. A series offers
 * the next episode first, then "Play again" and "Back to <title>"; a movie puts "Back to <title>"
 * first. The scrim is full-bleed, so the picture is still there behind the words.
 */
@Composable
fun EndCard(
    title: String,
    next: NextEpisode?,
    onNext: () -> Unit,
    onAgain: () -> Unit,
    onBack: () -> Unit,
    focus: FocusRequester,
    modifier: Modifier = Modifier,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Box(modifier.background(Color.Black.copy(alpha = 0.72f)), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(16.dp)) {
            if (next != null) {
                MqText(
                    "Next: ${Format.episodeLabel(next.season, next.episode)} ${next.title}".trim(),
                    role = TextRole.Callout,
                    color = colors.muted,
                    maxLines = 2,
                )
            }
            val buttons: List<Triple<String, () -> Unit, ImageVector?>> =
                buildList {
                    if (next != null) add(Triple("Next episode", onNext, Icons.SkipForward))
                    if (next != null) add(Triple("Play again", onAgain, Icons.RotateCcw))
                    add(Triple("Back to $title", onBack, Icons.ArrowLeft))
                    if (next == null) add(Triple("Play again", onAgain, Icons.RotateCcw))
                }
            val buttonRow: @Composable () -> Unit = {
                buttons.forEachIndexed { i, (label, action, icon) ->
                    if (i == 0) {
                        PrimaryButton(label, action, icon = icon, focusRequester = focus)
                    } else {
                        SecondaryButton(label, action, icon = icon)
                    }
                }
            }
            if (tv) {
                Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) { buttonRow() }
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) { buttonRow() }
            }
        }
    }
}

/** A scrim from one edge: transparent to [alpha] black over [height], full width, so there is no box edge to see. */
@Composable
fun Scrim(
    top: Boolean,
    height: Dp,
    alpha: Float,
    modifier: Modifier = Modifier,
) {
    val stops =
        if (top) {
            listOf(
                Color.Black.copy(alpha = alpha),
                Color.Transparent,
            )
        } else {
            listOf(Color.Transparent, Color.Black.copy(alpha = alpha))
        }
    Box(
        modifier
            .fillMaxWidth()
            .height(height)
            .background(Brush.verticalGradient(stops)),
    )
}

/**
 * The reason a source failed, in words a viewer can use. The core says it in its own terms ("source
 * did not answer: http status 404"); the two shapes it uses most are put plainly, the rest is shown as is.
 */
fun attemptReason(error: String): String {
    val status = Regex("http status (\\d{3})").find(error)?.groupValues?.get(1)
    val seconds = Regex("timed out after (\\d+)\\s*s").find(error)?.groupValues?.get(1)
    return when {
        status == "404" || status == "410" -> "The link no longer exists ($status)"
        status != null -> "The source refused the request ($status)"
        seconds != null -> "No answer in $seconds seconds"
        error.contains("did not answer in time") -> "No answer in time"
        else -> error
    }
}

/** An action of the OSD: icon, bold name, and a small line under it (what is chosen now). */
@Composable
fun ActionButton(
    icon: ImageVector,
    label: String,
    detail: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    Column(
        modifier
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .marqueeFocus(Marquee.radius.m, scale = 1.04f)
            .clip(RoundedCornerShape(Marquee.radius.m))
            .semantics { contentDescription = listOfNotNull(label, detail).joinToString(", ") }
            .focusClickable(onClick, Role.Button)
            .widthIn(min = if (tv) 128.dp else 0.dp, max = if (tv) 240.dp else 150.dp)
            .padding(horizontal = if (tv) 8.dp else 2.dp, vertical = if (tv) 10.dp else 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        MqIcon(icon, tint = colors.text, size = if (tv) 32.dp else 20.dp)
        MqText(label, role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 1)
        detail?.let { MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 1) }
    }
}

/** 1.0 as "1x", 1.25 as "1.25x". */
fun speedText(speed: Float): String = if (speed % 1f == 0f) "${speed.toInt()}x" else "${speed}x"

/** The speeds the speed button steps through, back to 1x after the last. */
val SPEEDS = listOf(1f, 1.25f, 1.5f, 2f, 0.75f)

fun nextSpeed(current: Float): Float = SPEEDS[(SPEEDS.indexOf(current) + 1) % SPEEDS.size]
