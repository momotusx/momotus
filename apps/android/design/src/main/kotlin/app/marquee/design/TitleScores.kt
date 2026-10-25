package app.marquee.design

import androidx.compose.foundation.Image
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import coil3.size.Size

/** One item of the title page's ratings row: the service's [mark], its number ([text] null while pending), and the
 * title's page there ([url], [name] the service's name for the link's words). */
data class Score(
    val mark: ImageVector,
    val text: String?,
    val spoken: String,
    val url: String? = null,
    val name: String = "",
)

/**
 * The title page's meta line (plans/title-page-ratings-genres-similar.md, design/DECISION.md): the facts first, then
 * the ratings row, each number after its service's own mark, in its colours. The row's height is held from the
 * first draw (24 dp, TV 36) and a pending slot keeps its width, so nothing moves when TMDB and Trakt land; a slot
 * with no number takes no space. Each item is one element with one spoken name; the marks say nothing themselves.
 * [onOpen] makes each score with a URL a link to the title's page there (the phone; never TV). Its target grows to
 * 48 dp in hit testing only (Compose's minimum touch target), so the row keeps its height and neighbours split the
 * gap between them.
 */
@Composable
fun TitleRatingLine(
    facts: String,
    scores: List<Score>,
    modifier: Modifier = Modifier,
    onOpen: ((String) -> Unit)? = null,
    role: TextRole = TextRole.Callout,
    factsColor: Color = Marquee.colors.muted,
) {
    val tv = Marquee.form == Form.Tv
    Row(
        modifier,
        horizontalArrangement = Arrangement.spacedBy(if (tv) 30.dp else 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (facts.isNotEmpty()) {
            MqText(
                facts,
                Modifier.weight(1f, fill = false),
                role = role,
                color = factsColor,
                weight = FontWeight.SemiBold,
                maxLines = 1,
            )
        }
        if (scores.isNotEmpty()) {
            Row(
                Modifier.height(if (tv) 36.dp else 24.dp),
                horizontalArrangement = Arrangement.spacedBy(if (tv) 30.dp else 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                scores.forEach { score ->
                    val url = score.url?.takeIf { score.text != null && onOpen != null }
                    ScoreItem(
                        score,
                        role,
                        if (url != null &&
                            onOpen != null
                        ) {
                            Modifier.focusClickable({ onOpen(url) }, Role.Button)
                        } else {
                            Modifier
                        },
                        link = url != null,
                    )
                }
            }
        }
    }
}

@Composable
private fun ScoreItem(
    score: Score,
    role: TextRole,
    modifier: Modifier,
    link: Boolean,
) {
    val tv = Marquee.form == Form.Tv
    val pending = score.text == null
    Row(
        modifier
            // A pending slot keeps its width and says nothing; an answered one says its one name.
            .alpha(if (pending) 0f else 1f)
            .clearAndSetSemantics {
                if (!pending) {
                    contentDescription =
                        if (link) "${score.spoken}, on ${score.name}" else score.spoken
                }
            },
        horizontalArrangement = Arrangement.spacedBy(if (tv) 12.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Image(score.mark, contentDescription = null, modifier = Modifier.height(if (tv) 28.dp else 18.dp))
        MqText(
            score.text ?: "00%",
            role = role,
            color = Marquee.colors.text,
            weight = FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Visible,
            softWrap = false,
        )
    }
}

/**
 * The title page's rated cover (plans/title-page-ratings-genres-similar.md): RPDB's poster with the IMDb, Metacritic
 * and Rotten Tomatoes band drawn in, the only place Rotten Tomatoes shows on the page. A fixed 2:3 poster ([width]),
 * decoration only: no focus stop, one spoken name ("Cover with ratings"). When the image cannot load it hides itself
 * rather than leave an empty box.
 */
@Composable
fun RatedCover(
    url: String,
    width: Dp,
    modifier: Modifier = Modifier,
) {
    var failed by remember(url) { mutableStateOf(false) }
    if (failed) return
    val height = width * 1.5f
    val context = LocalContext.current
    val density = LocalDensity.current
    val widthPx = with(density) { width.roundToPx() }
    val heightPx = with(density) { height.roundToPx() }
    val request =
        remember(url, widthPx, heightPx) {
            ImageRequest
                .Builder(context)
                .data(url)
                .size(Size(widthPx, heightPx))
                .memoryCacheKey(imageKey(url, widthPx, heightPx))
                .build()
        }
    AsyncImage(
        model = request,
        contentDescription = "Cover with ratings",
        contentScale = ContentScale.Crop,
        onError = { failed = true },
        modifier = modifier.size(width, height).clip(RoundedCornerShape(Marquee.radius.m)),
    )
}

/**
 * The people on the title page (plans/title-page-ratings-genres-similar.md, 4): a label, then each name a chip that
 * opens Search for that person. On TV each row is a focus group, Left and Right walking its names.
 */
@OptIn(ExperimentalLayoutApi::class, ExperimentalComposeUiApi::class)
@Composable
fun People(
    rows: List<Pair<String, List<String>>>,
    onPerson: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (rows.isEmpty()) return
    val tv = Marquee.form == Form.Tv
    // Entering the names from the side lands on the first one, wherever the rated cover put the column.
    val first = remember { FocusRequester() }
    Column(
        modifier.focusProperties { enter = { first } }.focusGroup(),
        verticalArrangement = Arrangement.spacedBy(if (tv) 14.dp else 10.dp),
    ) {
        rows.forEachIndexed { row, (label, names) ->
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                MqText(
                    label,
                    role = TextRole.Caption,
                    color = Marquee.colors.dim,
                    weight = FontWeight.SemiBold,
                    maxLines = 1,
                )
                FlowRow(
                    Modifier.focusGroup(),
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                ) {
                    names.forEachIndexed { i, name ->
                        Chip(
                            label = name,
                            selected = false,
                            onClick = { onPerson(name) },
                            focusRequester = if (row == 0 && i == 0) first else null,
                        )
                    }
                }
            }
        }
    }
}
