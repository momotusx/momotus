package app.marquee.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import app.marquee.core.AddonsCopy
import app.marquee.core.ErrorCopy
import app.marquee.core.PluginCopy
import app.marquee.core.model.ListedRepo
import app.marquee.core.model.PluginDirectory
import app.marquee.core.model.PluginRepoInfo
import app.marquee.core.model.PluginRepoPreview
import app.marquee.core.model.PluginsAnswer
import app.marquee.core.session.CoreSession
import app.marquee.design.ConfirmSheet
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.RoundButton
import app.marquee.design.SecondaryButton
import app.marquee.design.SwitchRow
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Find addons > Nuvio plugins (R8): the repositories nuvioplugins.com lists, in its order, each with its provider
 * count, kinds and languages, searched on the device; Add reads one and asks on the sheet first, and one the
 * profile has says Installed (removed in Settings > Sources > Plugins).
 */
@Composable
fun PluginDirectoryScreen(session: CoreSession) {
    val scope = rememberCoroutineScope()
    var listed by remember { mutableStateOf<PluginDirectory?>(null) }
    var query by remember { mutableStateOf("") }
    var asking by remember { mutableStateOf<PluginRepoPreview?>(null) }
    var said by remember { mutableStateOf<Pair<String, Said>?>(null) }
    val reload: suspend () -> Unit = { listed = runCatching { session.ops.pluginDirectory() }.getOrNull() }
    LaunchedEffect(Unit) { reload() }
    asking?.let { p ->
        ConfirmSheet(
            PluginCopy.fill(PluginCopy.SHEET_TITLE, "name" to p.name),
            listOf(PluginCopy.adds(p.providers), PluginCopy.SHEET_NOTE),
            PluginCopy.ADD,
            "Cancel",
            details = PluginCopy.fill(PluginCopy.SHEET_FROM, "host" to p.host),
            detailsLabel = AddonsCopy.DETAILS,
            onConfirm = {
                asking = null
                scope.launch {
                    said =
                        runCatching { session.ops.addPluginRepo(p.url) }.fold(
                            {
                                val added = PluginCopy.fill(PluginCopy.ADDED, "name" to p.name)
                                p.url to Said(Place.Addons, added, failed = false)
                            },
                            {
                                if (it is CancellationException) throw it
                                p.url to couldNotAdd(it)
                            },
                        )
                    reload()
                }
            },
            onCancel = { asking = null },
        )
    }
    val source = listed?.source.orEmpty()
    val tv = Marquee.form == Form.Tv
    val shown = listed?.repos.orEmpty().filter { PluginCopy.matches(it, query) }
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader(PluginCopy.DIRECTORY_TITLE) }
        // The directory by its own name; its host only behind Details (a TV shows none), as a repository's row does.
        if (!tv) item(key = "from") { SourceDetails(PluginCopy.DIRECTORY_TITLE, source) }
        when {
            listed == null -> {
                item(key = "loading") { Note(PluginCopy.LOADING) }
            }

            listed?.repos.isNullOrEmpty() -> {
                item(key = "na") { Note(PluginCopy.NOT_AVAILABLE) }
            }

            else -> {
                if (!tv) {
                    item(key = "search") {
                        Section {
                            MqTextField(
                                value = query,
                                onValueChange = { query = it },
                                label = PluginCopy.DIRECTORY_SEARCH,
                                placeholder = PluginCopy.DIRECTORY_SEARCH,
                            )
                        }
                    }
                }
                if (shown.isEmpty()) item(key = "none") { Note(PluginCopy.DIRECTORY_NO_MATCH, spoken = true) }
                items(shown, key = { "listed:${it.url}" }) { r ->
                    ListedCard(r, source, said?.takeIf { it.first == r.url }?.second) {
                        scope.launch {
                            runCatching { session.ops.previewPluginRepo(r.url) }
                                .onSuccess { asking = it }
                                .onFailure {
                                    if (it is CancellationException) throw it
                                    said = r.url to couldNotAdd(it)
                                }
                        }
                    }
                }
            }
        }
    }
}

/** A name's host behind Details (R5-9): a raw address is never printed under the name. */
@Composable
private fun SourceDetails(
    name: String,
    source: String,
) {
    var shown by remember(name) { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        SecondaryButton(
            AddonsCopy.DETAILS,
            { shown = !shown },
            selected = shown,
            spoken = "$name ${AddonsCopy.DETAILS.lowercase()}",
        )
        if (shown) {
            MqText(
                PluginCopy.fill(PluginCopy.DIRECTORY_FROM, "source" to source),
                role = TextRole.Caption,
                color = Marquee.colors.dim,
                maxLines = 1,
            )
        }
    }
}

/** One listed repository: name, the directory it comes from, its line and description, Add or Installed. */
@Composable
private fun ListedCard(
    r: ListedRepo,
    source: String,
    said: Said?,
    onAdd: () -> Unit,
) {
    val colors = Marquee.colors
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            MqText(r.name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 2)
            if (Marquee.form != Form.Tv) SourceDetails(r.name, source)
            MqText(PluginCopy.directoryLine(r), role = TextRole.Caption, weight = FontWeight.SemiBold)
            if (r.description.isNotEmpty()) {
                MqText(
                    r.description,
                    role = TextRole.Caption,
                    color = colors.muted,
                    maxLines = 3,
                )
            }
            if (said != null) LiveSpoken(said.text) { SaidLine(said, side = 0.dp) }
            if (r.installed) {
                MqText(PluginCopy.INSTALLED, role = TextRole.Callout, weight = FontWeight.SemiBold)
            } else {
                RoundButton(Icons.Plus, PluginCopy.ADD, onAdd, showLabel = true, spoken = "${PluginCopy.ADD} ${r.name}")
            }
        }
    }
}

/** Why an op failed, in the core's words. */
private fun reason(e: Throwable): String = ErrorCopy.words(e)

/** "Could not add: {reason}" under the field or the card. */
private fun couldNotAdd(e: Throwable): Said {
    val words = PluginCopy.fill(PluginCopy.COULD_NOT_ADD, "reason" to reason(e))
    return Said(Place.Addons, words, failed = true)
}

/**
 * Settings > Sources > Plugins (plans/plugins-and-marketplace.md 7.7, R7): the profile's Nuvio
 * repositories, a switch for each and for each of its providers, Remove, and a link field whose
 * repository is read and shown on a sheet; nothing is added before the sheet's Add.
 */
@Composable
fun PluginsScreen(session: CoreSession) {
    val scope = rememberCoroutineScope()
    var answer by remember { mutableStateOf<PluginsAnswer?>(null) }
    var failed by remember { mutableStateOf<String?>(null) }
    var said by remember { mutableStateOf<Said?>(null) }
    var asking by remember { mutableStateOf<PluginRepoPreview?>(null) }
    var busy by remember { mutableStateOf(false) }
    val addFocus = remember { FocusRequester() }
    val reload: suspend () -> Unit = {
        runCatching { session.ops.plugins() }
            .onSuccess {
                answer = it
                failed = null
            }.onFailure { if (it is CancellationException) throw it else failed = reason(it) }
    }
    LaunchedEffect(Unit) { reload() }
    // A switch or a removal, then the list as the core has it now.
    val run: (suspend () -> Unit) -> Unit = { op ->
        scope.launch {
            runCatching { op() }.onFailure {
                if (it is CancellationException) {
                    throw it
                } else {
                    said =
                        Said(Place.Addons, reason(it), failed = true)
                }
            }
            reload()
        }
    }
    asking?.let { p ->
        ConfirmSheet(
            PluginCopy.fill(PluginCopy.SHEET_TITLE, "name" to p.name),
            listOf(PluginCopy.adds(p.providers), PluginCopy.SHEET_NOTE),
            PluginCopy.ADD,
            "Cancel",
            details = PluginCopy.fill(PluginCopy.SHEET_FROM, "host" to p.host),
            detailsLabel = AddonsCopy.DETAILS,
            onConfirm = {
                asking = null
                said = Said(Place.Addons, PluginCopy.ADDING, failed = false, pending = true)
                scope.launch {
                    runCatching { session.ops.addPluginRepo(p.url) }
                        .onSuccess {
                            answer = it
                            said =
                                Said(Place.Addons, PluginCopy.fill(PluginCopy.ADDED, "name" to p.name), failed = false)
                        }.onFailure {
                            if (it is CancellationException) throw it
                            said =
                                Said(
                                    Place.Addons,
                                    PluginCopy.fill(PluginCopy.COULD_NOT_ADD, "reason" to reason(it)),
                                    failed = true,
                                )
                        }
                }
            },
            onCancel = { asking = null },
            opener = addFocus,
        )
    }
    val repos = answer?.repos.orEmpty()
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader(PluginCopy.TITLE) }
        item(key = "lede") { Note(PluginCopy.LEDE) }
        when {
            failed != null && answer == null -> {
                item(key = "failed") { Note(failed!!, failed = true, spoken = true) }
            }

            answer == null -> {
                item(key = "loading") { Note(PluginCopy.LOADING) }
            }

            answer?.available == false -> {
                item(key = "na") { Note(PluginCopy.NOT_AVAILABLE) }
            }

            else -> {
                item(key = "field") {
                    AddField(busy, addFocus, said) { url ->
                        busy = true
                        said = Said(Place.Addons, PluginCopy.CHECKING, failed = false, pending = true)
                        scope.launch {
                            runCatching { session.ops.previewPluginRepo(url) }
                                .onSuccess {
                                    said = null
                                    asking = it
                                }.onFailure {
                                    if (it is CancellationException) throw it
                                    said =
                                        Said(
                                            Place.Addons,
                                            PluginCopy.fill(
                                                PluginCopy.COULD_NOT_ADD,
                                                "reason" to reason(it),
                                            ),
                                            failed = true,
                                        )
                                }
                            busy = false
                        }
                    }
                }
                if (repos.isEmpty()) item(key = "empty") { Note(PluginCopy.EMPTY) }
                itemsIndexed(repos, key = { _, r -> "repo:${r.url}" }) { _, repo ->
                    RepoRow(
                        repo,
                        onToggle = { run { session.ops.setPluginRepoEnabled(repo.url, !repo.enabled) } },
                        onProvider = { id, on -> run { session.ops.setPluginEnabled(repo.url, id, on) } },
                        onRemove = { run { session.ops.removePluginRepo(repo.url) } },
                    )
                }
            }
        }
    }
}

/** The repository link field and its Add; how the last read went under it. */
@Composable
private fun AddField(
    busy: Boolean,
    focus: FocusRequester,
    said: Said?,
    onAdd: (String) -> Unit,
) {
    var text by remember { mutableStateOf("") }
    val add = { if (!busy && text.isNotBlank()) onAdd(text.trim()) }
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            MqTextField(
                value = text,
                onValueChange = { text = it },
                label = PluginCopy.FIELD_LABEL,
                placeholder = "https://... manifest.json",
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = { add() }),
            )
            PrimaryButton(
                label = PluginCopy.ADD,
                icon = Icons.Plus,
                enabled = !busy && text.isNotBlank(),
                onClick = add,
                focusRequester = focus,
            )
            if (said != null) LiveSpoken(said.text) { SaidLine(said, side = 0.dp) }
        }
    }
}

/** One repository: its switch, its providers' switches, Remove (a second press within 4 s). */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun RepoRow(
    repo: PluginRepoInfo,
    onToggle: () -> Unit,
    onProvider: (String, Boolean) -> Unit,
    onRemove: () -> Unit,
) {
    val colors = Marquee.colors
    val name = repo.name ?: PluginCopy.UNNAMED
    // The repository by its own name; its host only behind Details.
    var details by remember(repo.url) { mutableStateOf(false) }
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            SwitchRow(
                repo.enabled,
                onToggle,
                Modifier
                    .marqueeFocus(Marquee.radius.m, Tokens.FOCUS_SCALE_TILE),
                shape = RoundedCornerShape(Marquee.radius.m),
            ) {
                Column(Modifier.weight(1f)) {
                    MqText(name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 2)
                }
            }
            SecondaryButton(
                AddonsCopy.DETAILS,
                { details = !details },
                selected = details,
                spoken = "$name ${AddonsCopy.DETAILS.lowercase()}",
            )
            if (details) MqText(repo.host, role = TextRole.Caption, color = colors.dim, maxLines = 2)
            if (repo.description.isNotEmpty()) {
                MqText(
                    repo.description,
                    role = TextRole.Caption,
                    color = colors.muted,
                    maxLines = 3,
                )
            }
            repo.problem?.let {
                MqText(PluginCopy.fill(PluginCopy.UNREAD, "problem" to it), role = TextRole.Caption, color = colors.bad)
            }
            repo.providers.forEach { p ->
                SwitchRow(
                    p.enabled,
                    { onProvider(p.id, !p.enabled) },
                    Modifier
                        .marqueeFocus(Marquee.radius.m, Tokens.FOCUS_SCALE_TILE),
                    shape = RoundedCornerShape(Marquee.radius.m),
                ) {
                    Column(Modifier.weight(1f)) {
                        MqText(p.name, role = TextRole.Body, maxLines = 2)
                        if (p.languages.isNotEmpty()) {
                            MqText(
                                p.languages.joinToString(", "),
                                role = TextRole.Caption,
                                color = colors.dim,
                                maxLines = 1,
                            )
                        }
                    }
                }
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.m)) {
                var armed by remember(repo.url) { mutableStateOf(false) }
                LaunchedEffect(armed) {
                    if (armed) {
                        delay(4_000)
                        armed = false
                    }
                }
                RoundButton(
                    Icons.Trash,
                    if (armed) "Press again" else PluginCopy.REMOVE,
                    {
                        if (armed) {
                            armed = false
                            onRemove()
                        } else {
                            armed = true
                        }
                    },
                    showLabel = true,
                    selected = armed,
                    spoken = if (armed) "${PluginCopy.REMOVE_CONFIRM} $name" else "${PluginCopy.REMOVE} $name",
                )
            }
        }
    }
}
