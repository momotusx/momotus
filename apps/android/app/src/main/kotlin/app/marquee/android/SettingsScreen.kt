package app.marquee.android

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.AccountCopy
import app.marquee.core.AccountRules
import app.marquee.core.AddonsCopy
import app.marquee.core.DebridCopy
import app.marquee.core.DefaultsCopy
import app.marquee.core.ErrorCopy
import app.marquee.core.FindAddonsCopy
import app.marquee.core.LanguagePicks
import app.marquee.core.LinkInstall
import app.marquee.core.PluginCopy
import app.marquee.core.ProfileCopy
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.SyncCopy
import app.marquee.core.TraktCopy
import app.marquee.core.model.AddonInfo
import app.marquee.core.model.LanguageChoice
import app.marquee.core.model.PluginsAnswer
import app.marquee.core.model.SubtitleLayout
import app.marquee.core.model.SubtitleSize
import app.marquee.core.model.Tile
import app.marquee.core.session.CoreSession
import app.marquee.core.views.AddonsView
import app.marquee.core.views.SettingsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.BrandLockup
import app.marquee.design.Chip
import app.marquee.design.FocusInk
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.LocalIncognito
import app.marquee.design.Marks
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.RoundButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.SwitchRow
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.TrailerPreviews
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.focusSoon
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

// Settings (A1, SettingsA1.kt): the profile on a film's backdrop, then Sources, Account, Home, Playback, This device and
// About as grouped lists. Addons and "Add a source" are a page of their own (AddonsScreen).

class SettingsHolder(
    val session: CoreSession,
) : ViewModel() {
    val prefs: SettingsView = session.settings()
    val addons: AddonsView = session.addons()

    override fun onCleared() {
        prefs.close()
        addons.close()
    }
}

/** The parts of the page an operation can come from. */
internal enum class Place { Addons, Install, Playback, Subtitles }

/** What an operation ended with, shown under the part of the page it came from. */
internal class Said(
    val place: Place,
    val text: String,
    val failed: Boolean,
    /** Still running: the line says what is under way, as the web's "Installing". */
    val pending: Boolean = false,
)

@Composable
internal fun SaidLine(
    said: Said,
    side: Dp = Marquee.safe.side,
) {
    when {
        said.failed -> Note(said.text, failed = true, side = side)
        else -> StatusWord(said.text, if (said.pending) Status.Warn else Status.Ok, Modifier.padding(horizontal = side))
    }
}

/**
 * One operation at a time; what it ends with stays on screen in plain words, under the part of the page it came
 * from (the top of the page is out of sight from the install field).
 */
internal class Runner(
    private val scope: CoroutineScope,
) {
    var busy by mutableStateOf<String?>(null)
        private set
    var said by mutableStateOf<Said?>(null)
        private set

    fun run(
        what: String,
        place: Place,
        then: () -> Unit = {},
        op: suspend () -> Unit,
    ) {
        if (busy != null) return
        busy = what
        said = Said(place, what, failed = false, pending = true)
        scope.launch {
            try {
                op()
                // An operation that says nothing on success (all but an install) leaves no line.
                if (said?.pending == true) said = null
                then()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                said =
                    Said(
                        place,
                        "$what did not work: ${ErrorCopy.words(e)}",
                        failed = true,
                    )
            } finally {
                busy = null
            }
        }
    }
}

@Composable
private fun rememberRunner(): Runner {
    val scope = rememberCoroutineScope()
    return remember(scope) { Runner(scope) }
}

@Composable
private fun settingsHolder(session: CoreSession) =
    viewModel<SettingsHolder>(
        key = "settings",
        factory = viewModelFactory { initializer { SettingsHolder(session) } },
    )

@Composable
fun SettingsScreen(
    session: CoreSession,
    gate: ProfileGate,
    /** The header's film ([settingsArt]); null leaves the header on the page color. */
    art: Tile?,
    onProfiles: () -> Unit,
    onEditProfile: (String) -> Unit,
    onManageProfiles: () -> Unit,
    sync: SyncController,
    accounts: AccountFlows,
    /** Sign in: the phone's sign-in screen, a TV's QR. */
    onSignIn: () -> Unit,
    onAddons: () -> Unit,
    onImport: () -> Unit,
    onSync: () -> Unit,
    onDebrid: () -> Unit,
    onTrakt: () -> Unit,
    onHomeRows: () -> Unit,
    onFindAddons: () -> Unit,
    onStreamChoice: () -> Unit,
    /** The recommended addons step with every switch. */
    onDefaults: () -> Unit,
    /** Settings > Sources > Plugins. */
    onPlugins: () -> Unit = {},
    /** The phone's section page this entry shows ([SettingsSection] name); null for the list of sections. */
    section: String? = null,
    /** Opens a section's page on the phone. */
    onSection: (String) -> Unit = {},
) {
    val holder = settingsHolder(session)
    val runner = rememberRunner()
    val tv = Marquee.form == Form.Tv
    val downloads = !tv && downloadsView(session).current() != null
    val context = LocalContext.current
    val snapshot = gate.snapshot
    val me = AccountRules.activeProfile(snapshot)
    // Incognito shows its header, Sources (Addons and Plugins), Home, Playback, Subtitles, This device (its trailer row
    // says it is off) and About; nothing of the profiles, the account or Downloads (plans/batch-10-incognito.md 6).
    val incognito = ProfileCopy.inIncognito(snapshot)
    // Edit and Manage need the owner's PIN on a kids profile (DECISION kids rules): a kids profile shows neither.
    val kids = me?.kids == true || snapshot?.kids == true
    val editable = editableProfile(me, kids).takeUnless { incognito }
    val canManage = editable?.isOwner == true
    val line = if (incognito) ProfileCopy.INCOGNITO_KEPT_NOTHING else profileLine((snapshot?.profiles?.size ?: 1) > 1)
    val onLeave = if (incognito) gate::startLeave else null
    // Sign in and Connect Google sit here, where people look for them; the Account page keeps the rest.
    val actions = accounts.actions
    val server = remember(sync.joined) { accounts.server(context) }
    val offered by produceState(emptyList<String>(), server, sync.joined) {
        value = if (sync.joined) emptyList() else accounts.providers(server)
    }
    val mayLink = accounts.rows(tv).links
    LaunchedEffect(sync.joined, mayLink) { if (sync.joined && mayLink && !tv) actions.load() }
    val connect =
        sync.joined && mayLink && !tv &&
            AccountRules.connectGoogle(actions.providers, actions.links?.map { it.provider })
    val parts =
        buildList {
            // The phone's header holds the profile alone; its edits are a group of their own. TV has them on the art.
            if (!tv && editable != null) {
                add(
                    SettingsPart(SettingsSection.Profiles) {
                        SettingsGroup(
                            {
                                SettingsRow(
                                    Icons.UserPen,
                                    Tone.Blue,
                                    "Edit profile",
                                    "Name, picture and PIN.",
                                    { onEditProfile(editable.id) },
                                )
                            },
                            if (canManage) {
                                {
                                    SettingsRow(
                                        Icons.Users,
                                        Tone.Sky,
                                        "Manage profiles",
                                        "Add, edit and remove profiles on this account.",
                                        onManageProfiles,
                                    )
                                }
                            } else {
                                null
                            },
                        )
                    },
                )
            }
            add(
                SettingsPart(SettingsSection.Sources) {
                    Sources(
                        holder,
                        session,
                        kids,
                        incognito,
                        onDefaults,
                        onFindAddons,
                        onAddons,
                        onDebrid,
                        onTrakt,
                        onImport,
                        onPlugins,
                    )
                },
            )
            add(
                SettingsPart(SettingsSection.Account) {
                    SettingsGroup(
                        // Signed out, where the sync service offers sign-in: the phone's screen, a TV's QR.
                        if (!sync.joined && offered.isNotEmpty()) {
                            {
                                SettingsRow(
                                    Icons.DoorOpen,
                                    Tone.Blue,
                                    AccountCopy.SIGN_IN_TITLE,
                                    AccountCopy.LIBRARY_ADDED,
                                    onSignIn,
                                )
                            }
                        } else {
                            null
                        },
                        { SettingsRow(Icons.MonitorSmartphone, Tone.Sky, SyncCopy.TITLE, syncWords(sync), onSync) },
                        // Google's page opens over the Account page, which shows how the link went.
                        if (connect) {
                            {
                                SettingsRow(
                                    Icons.Link,
                                    Tone.Blue,
                                    AccountRules.connectTitle("google"),
                                    AccountRules.connectText("google"),
                                    {
                                        actions.link("google")
                                        onSync()
                                    },
                                )
                            }
                        } else {
                            null
                        },
                    )
                },
            )
            add(
                SettingsPart(SettingsSection.Home) {
                    SettingsGroup({
                        SettingsRow(
                            Icons.Rows3,
                            Tone.Mint,
                            "Home rows",
                            "Which catalogs Home shows after Continue Watching, and in what order.",
                            onHomeRows,
                        )
                    })
                },
            )
            add(
                SettingsPart(SettingsSection.Playback) {
                    Column(verticalArrangement = Arrangement.spacedBy(if (tv) 20.dp else 16.dp)) {
                        val streams = holder.prefs.current()?.streams
                        SettingsGroup({
                            SettingsRow(
                                Icons.SlidersHorizontal,
                                Tone.Teal,
                                StreamChoiceCopy.TITLE,
                                streams?.let(StreamChoiceCopy::summary) ?: StreamChoiceCopy.LEDE,
                                onStreamChoice,
                            )
                        })
                        Playback(holder, runner.busy != null) { what, op -> runner.run(what, Place.Playback, op = op) }
                        runner.said?.takeIf { it.place == Place.Playback }?.let { SaidLine(it, side = 0.dp) }
                    }
                },
            )
            add(
                SettingsPart(SettingsSection.Subtitles) {
                    Column(verticalArrangement = Arrangement.spacedBy(if (tv) 20.dp else 16.dp)) {
                        SubtitlesAudio(
                            holder,
                            runner.busy != null,
                        ) { what, op -> runner.run(what, Place.Subtitles, op = op) }
                        runner.said?.takeIf { it.place == Place.Subtitles }?.let { SaidLine(it, side = 0.dp) }
                    }
                },
            )
            // Downloads are the phone's (plans/offline-downloads.md 3.7); the section comes with its content, so
            // nothing shows before the view answers.
            if (downloads) add(SettingsPart(SettingsSection.Downloads) { DownloadsSettings(session) })
            // This device only, never synced (plans/appletv-polish.md decision 13): on a TV where its title pages may
            // play the header trailer (plans/trailer-inline.md 4a).
            if (!tv || TrailerPreviews.headerOnTv(context)) add(SettingsPart(SettingsSection.Device) { DevicePrefs() })
            add(SettingsPart(SettingsSection.About) { About() })
        }.filter { incognitoShows(incognito, it.section) && kidsShows(kids, it.section) }
    if (tv) {
        TvSettings(
            me,
            line,
            art,
            editable != null,
            canManage,
            onProfiles,
            { me?.let { onEditProfile(it.id) } },
            onManageProfiles,
            parts,
            onLeave,
        )
    } else {
        PhoneSettings(
            me,
            line,
            art,
            onProfiles,
            parts,
            settingsSection(section),
            onLeave = onLeave,
        ) { onSection(it.name) }
    }
}

/** The account's state in a line: what is wrong or still waits first, never "Synced" over a round that failed or waits. */
private fun syncWords(sync: SyncController): String {
    val last = sync.lastSynced
    return if (sync.syncing && sync.joined && sync.problem == null) {
        SyncCopy.SYNCING
    } else {
        SyncCopy.summary(
            joined = sync.joined,
            problem = sync.problem?.words,
            failed = SyncCopy.failedSince(sync.failed.refused, sync.failed.unreadable, sync.failed.newer),
            report = sync.lastReport,
            time =
                last?.let {
                    java.text.DateFormat
                        .getTimeInstance(
                            java.text.DateFormat.SHORT,
                        ).format(java.util.Date(it))
                },
        )
    }
}

/** Sources: the recommended addons, Find addons, the addons page, debrid, Trakt and the Stremio import. */
@Composable
private fun Sources(
    holder: SettingsHolder,
    session: CoreSession,
    kids: Boolean,
    incognito: Boolean,
    onDefaults: () -> Unit,
    onFindAddons: () -> Unit,
    onAddons: () -> Unit,
    onDebrid: () -> Unit,
    onTrakt: () -> Unit,
    onImport: () -> Unit,
    onPlugins: () -> Unit,
) {
    // Nuvio plugin repositories (R7): the row only where plugins run (full builds, not kids).
    val plugins by produceState<PluginsAnswer?>(null, kids) {
        value =
            if (kids) null else runCatching { session.ops.plugins() }.getOrNull()
    }
    val shownPlugins = plugins?.takeIf { it.available }
    val addons = holder.addons
    val addonsLine =
        when {
            addons.status is ViewStatus.Failed -> "Addons could not load"
            else -> addons.current()?.addons?.let(AddonsCopy::summary) ?: ""
        }
    val debrid =
        session.debrid
            .current()
            ?.accounts
            ?.size ?: 0
    val trakt = (LocalContext.current.applicationContext as MarqueeApp).trakt
    val addonsRow: @Composable () -> Unit = { SettingsRow(Icons.Puzzle, Tone.Mint, "Addons", addonsLine, onAddons) }
    val pluginsRow: (@Composable () -> Unit)? =
        shownPlugins?.let { p ->
            { SettingsRow(Icons.Power, Tone.Teal, PluginCopy.TITLE, PluginCopy.summary(p.repos), onPlugins) }
        }
    if (incognito) {
        // Incognito: its own copy of the addons and plugins; the device's debrid account is used, never changed.
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            SettingsGroup(addonsRow, pluginsRow)
            if (debrid > 0) Note(ProfileCopy.INCOGNITO_DEBRID, side = 0.dp)
        }
        return
    }
    SettingsGroup(
        // A kids profile reads the owner's addons: the defaults are not its to change.
        if (kids) {
            null
        } else {
            {
                SettingsRow(
                    Icons.Sparkles,
                    Tone.Green,
                    DefaultsCopy.SETTINGS_ROW,
                    DefaultsCopy.SETTINGS_ROW_LINE,
                    onDefaults,
                )
            }
        },
        {
            SettingsRow(
                Icons.PackageSearch,
                Tone.Teal,
                FindAddonsCopy.TITLE,
                FindAddonsCopy.LEDE,
                onFindAddons,
                lines = Int.MAX_VALUE,
            )
        },
        addonsRow,
        pluginsRow,
        {
            SettingsRow(
                Icons.KeyRound,
                Tone.Sky,
                "Debrid services",
                if (debrid ==
                    0
                ) {
                    "Play torrents as fast direct links with a debrid account."
                } else {
                    DebridCopy.inUse(debrid)
                },
                onDebrid,
            )
        },
        // The state is said whole, never cut short.
        {
            SettingsRow(
                Icons.Radio,
                Tone.Blue,
                TraktCopy.TITLE,
                TraktCopy.row(trakt.status, ::clockTime),
                onTrakt,
                lines = 4,
            )
        },
        {
            SettingsRow(
                Icons.Download,
                Tone.Sea,
                "Import from Stremio",
                "Addons, library and progress, with a code. No password typed here.",
                onImport,
            )
        },
    )
}

/**
 * Settings > Addons, and "Add a source" from a title ([sourcesOnly]: the link field first and only the source addons,
 * as on Apple): every installed addon with its switch, order and removal, the link field and Find addons.
 */
@Composable
fun AddonsScreen(
    session: CoreSession,
    link: LinkInstall,
    onFindAddons: () -> Unit,
    sourcesOnly: Boolean,
) {
    val holder = settingsHolder(session)
    val runner = rememberRunner()
    val busy = runner.busy != null
    val requesters = remember { mutableStateMapOf<String, FocusRequester>() }
    val installFocus = remember { FocusRequester() }
    LinkSheet(session, link, installFocus)
    val addonsSnapshot = holder.addons.current()
    val addons =
        addonsSnapshot
            ?.addons
            .orEmpty()
            .filter { !sourcesOnly || "stream" in it.provides }
    // A remote types; only a phone can paste.
    val linkVerb = if (Marquee.form == Form.Tv) "Type" else "Paste"
    val incognito = LocalIncognito.current
    if (sourcesOnly) {
        LaunchedEffect(Unit) {
            // The field is laid out in the first frame; focus it after that.
            focusSoon(installFocus)
        }
    }
    val find: @Composable () -> Unit = {
        LinkRow(Icons.PackageSearch, FindAddonsCopy.TITLE, FindAddonsCopy.LEDE, onFindAddons, lines = Int.MAX_VALUE)
    }
    LazyColumn(
        // Down from the TV pill comes back to what had focus (plans 2.1).
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader(if (sourcesOnly) "Add a source" else "Addons") }
        // Incognito works on its own copy of the addons (plans/batch-10-incognito.md 3).
        if (incognito) item(key = "incognito") { Note(ProfileCopy.INCOGNITO_ADDONS_NOTE) }
        if (sourcesOnly) {
            item(key = "sources-why") {
                Note(
                    "A source is an addon that finds a playable stream for a title. " +
                        "$linkVerb a source addon's link to add one.",
                )
            }
            item(key = "install") { LinkField(session, link, installFocus) }
            item(key = "find") { find() }
            item(key = "addons-head") { Kicker("Your sources") }
        }
        when {
            holder.addons.status is ViewStatus.Failed -> {
                item(key = "addons-failed") { Note("Addons could not load", failed = true) }
            }

            addons.isEmpty() && addonsSnapshot != null -> {
                item(key = "addons-none") {
                    Note(
                        if (sourcesOnly) {
                            "No source yet. $linkVerb a source addon's link above."
                        } else {
                            "No addon yet. $linkVerb an addon link below to add one."
                        },
                    )
                }
            }
        }
        itemsIndexed(addons, key = { _, addon -> "addon:${addon.url}" }) { index, addon ->
            val requester = remember(addon.url) { requesters.getOrPut(addon.url) { FocusRequester() } }
            AddonRow(
                addon = addon,
                first = index == 0,
                last = index == addons.lastIndex,
                busy = busy,
                focusRequester = requester,
                onToggle = {
                    runner.run(
                        if (addon.enabled) "Turning ${addon.name} off" else "Turning ${addon.name} on",
                        Place.Addons,
                    ) {
                        session.ops.setAddonEnabled(addon.url, !addon.enabled)
                    }
                },
                onMove = { by ->
                    runner.run("Moving ${addon.name}", Place.Addons) { session.ops.moveAddon(addon.url, index + by) }
                },
                onRemove = {
                    // Focus falls to the next addon, or to the link field when this was the last.
                    val next = addons.getOrNull(index + 1) ?: addons.getOrNull(index - 1)
                    runner.run(
                        "Removing ${addon.name}",
                        Place.Addons,
                        then = {
                            val target = next?.let { requesters[it.url] } ?: installFocus
                            runCatching { target.requestFocus() }
                        },
                    ) { session.ops.removeAddon(addon.url) }
                },
            )
        }
        runner.said?.takeIf { it.place == Place.Addons }?.let { item(key = "addons-said") { SaidLine(it) } }
        if (!sourcesOnly) {
            item(key = "install") { LinkField(session, link, installFocus) }
            item(key = "find") { find() }
        }
    }
}

/** A section card (Home's defaults card among them): solid `surface`, no edge (design/DECISION.md, Surfaces). */
@Composable
internal fun Section(
    side: Dp = Marquee.safe.side,
    radius: Dp = Marquee.radius.l,
    content: @Composable () -> Unit,
) {
    val shape = RoundedCornerShape(radius)
    Box(
        Modifier
            .padding(horizontal = side)
            .fillMaxWidth()
            .clip(shape)
            .background(Marquee.colors.surface)
            .padding(if (Marquee.form == Form.Tv) 24.dp else 16.dp),
    ) { content() }
}

/**
 * A setting that is on or off: its name and line at the left, the shared switch at the right, one switch for TalkBack
 * with its state (as the web and Apple). On TV the row takes the focus and turns white.
 */
@Composable
internal fun SettingSwitch(
    title: String,
    line: String?,
    on: Boolean,
    onToggle: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    SwitchRow(
        on,
        onToggle,
        Modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .marqueeFocus(Marquee.radius.m, Tokens.FOCUS_SCALE_TILE),
        if (tv) PaddingValues(horizontal = 16.dp, vertical = 12.dp) else PaddingValues(),
        RoundedCornerShape(Marquee.radius.m),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(title, role = TextRole.Headline, weight = FontWeight.SemiBold)
            // Read here: a TV focus turns the row white, and its words dark.
            line?.let { MqText(it, role = TextRole.Caption, color = Marquee.colors.dim) }
        }
    }
}

/** A row that opens another page: its icon, name and what it does, and a chevron (the web's LinkRow); no edge. */
@Composable
internal fun LinkRow(
    icon: ImageVector,
    title: String,
    text: String,
    onClick: () -> Unit,
    /** Lines the text may take; Find addons' lede is never cut. */
    lines: Int = 2,
    /** The page's own side margin; a grid that pads its items already gives none. */
    side: Dp = Marquee.safe.side,
) {
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.l)
    // On TV the focused row turns white with dark words (focusPalette).
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        Row(
            Modifier
                .padding(horizontal = side)
                .fillMaxWidth()
                .marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(colors.surface)
                .semantics(mergeDescendants = true) {}
                .focusClickable(onClick, Role.Button)
                .padding(if (Marquee.form == Form.Tv) 24.dp else 16.dp),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MqIcon(icon, tint = colors.muted)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                MqText(title, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
                MqText(text, role = TextRole.Caption, color = colors.muted, maxLines = lines)
            }
            // Points the way the page goes: left in a right-to-left layout.
            val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
            MqIcon(
                Icons.ChevronRight,
                modifier = Modifier.graphicsLayer { scaleX = if (rtl) -1f else 1f },
                tint = colors.muted,
            )
        }
    }
}

@Composable
private fun AddonRow(
    addon: AddonInfo,
    first: Boolean,
    last: Boolean,
    busy: Boolean,
    focusRequester: FocusRequester,
    onToggle: () -> Unit,
    onMove: (Int) -> Unit,
    onRemove: () -> Unit,
) {
    val colors = Marquee.colors
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            // The name line is one switch (as the recommended addons): TalkBack reads the name with its state.
            SwitchRow(
                addon.enabled,
                onToggle,
                Modifier
                    .focusRequester(focusRequester)
                    .marqueeFocus(Marquee.radius.m, Tokens.FOCUS_SCALE_TILE),
                shape = RoundedCornerShape(Marquee.radius.m),
            ) {
                Column(Modifier.weight(1f)) {
                    MqText(addon.name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
                    // The URL may hold keys: the host stands in for it, or where a built-in row runs.
                    MqText(
                        AddonsCopy.caption(addon),
                        role = TextRole.Caption,
                        // Read here: a TV focus turns the switch row white, and its words dark.
                        color = Marquee.colors.dim,
                        maxLines = 1,
                    )
                }
            }
            addon.description?.let { MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 2) }
            // Wraps on a small phone or a large font instead of cutting a label.
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.m),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
            ) {
                // Spoken with the addon's name: TalkBack moves between several of these rows.
                val name = addon.name
                RoundButton(
                    Icons.ChevronUp,
                    "Move up",
                    { onMove(-1) },
                    showLabel = true,
                    enabled = !first && !busy,
                    spoken = "Move $name up",
                )
                RoundButton(
                    Icons.ChevronDown,
                    "Move down",
                    { onMove(1) },
                    showLabel = true,
                    enabled = !last && !busy,
                    spoken = "Move $name down",
                )
                // Removing takes a second press within 4 s, as on the web (Apple asks in a dialog).
                var armed by remember(addon.url) { mutableStateOf(false) }
                LaunchedEffect(armed) {
                    if (armed) {
                        delay(4_000)
                        armed = false
                    }
                }
                RoundButton(
                    Icons.Trash,
                    // Short, so the circle barely moves under a finger that presses again.
                    if (armed) "Press again" else "Remove",
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
                    enabled = !busy,
                    spoken = if (armed) "Press again to remove $name" else "Remove $name",
                )
            }
        }
    }
}

/**
 * The link field with the one add-by-link flow ([LinkInstall]): Settings and Find addons' Paste a
 * link show the same request, which goes on when the page closes.
 */
@Composable
internal fun LinkField(
    session: CoreSession,
    link: LinkInstall,
    focus: FocusRequester,
    side: Dp = Marquee.safe.side,
    compact: Boolean = false,
) {
    val said = link.said?.let { Said(Place.Install, it.words, it.failed, it.pending) }
    // A link whose settings carry a sign-in is refused first; the user's confirm sends the field's
    // link (an edit drops the warning, so it is the link warned about), acknowledged.
    InstallField(
        link.busy,
        focus,
        said,
        side,
        compact,
        warned = link.said?.token == true,
        onEdit = link::edited,
        offered = link.offered,
        onTaken = { link.offered = null },
    ) { url, acknowledged, clearField ->
        link.clearField = clearField
        link.check(url, acknowledged, session.ops::previewAddon)
    }
}

/**
 * The link field's install sheet, held by the page rather than by the field: the field sits in a scrolling
 * list, and scrolled out of sight it leaves composition while its check runs on. Nothing installs from a link
 * before this sheet's Install (plans/addon-install-sheet.md); Cancel keeps the link in the field and the focus
 * goes back to Add. The page leaving drops a running check and an open sheet.
 */
@Composable
internal fun LinkSheet(
    session: CoreSession,
    link: LinkInstall,
    focus: FocusRequester,
) {
    DisposableEffect(link) { onDispose { link.left() } }
    link.checked?.let { checked ->
        val addon = checked.addon
        InstallSheet(
            addon.name,
            addon.host,
            addon.provides,
            addon.types,
            onInstall = { link.confirm({ url, ack -> session.ops.installAddon(url, ack).name }) },
            onCancel = link::cancel,
            opener = focus,
        )
    }
}

/** A field for an addon link (https, http or stremio://) and the button that adds it. */
@Composable
internal fun InstallField(
    busy: Boolean,
    focusRequester: FocusRequester,
    /** How the last add went, shown under the button. */
    status: Said?,
    side: Dp = Marquee.safe.side,
    /** Find addons' phone page: no heading and the button beside the field, so the cards start higher. */
    compact: Boolean = false,
    /** The last add was refused for a sign-in in the link: offer to add it anyway. */
    warned: Boolean = false,
    /** The text changed. */
    onEdit: () -> Unit = {},
    /** A link another app handed over: it fills the field (once, [onTaken]), and the user's Add adds it. */
    offered: String? = null,
    onTaken: () -> Unit = {},
    /** The link, whether the user confirmed its sign-in, and what clears the field once it is added. */
    onInstall: (String, Boolean, () -> Unit) -> Unit,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    var text by remember { mutableStateOf("") }
    LaunchedEffect(offered) {
        if (offered != null) {
            text = offered
            onEdit()
            onTaken()
        }
    }
    val keyboard = LocalSoftwareKeyboardController.current
    // The keyboard goes, so how it went (under the button) is in sight.
    val add = {
        if (!busy && text.isNotBlank()) {
            keyboard?.hide()
            val url = text.trim()
            // Cleared only when it still holds the link that went in.
            onInstall(url, false) { if (text.trim() == url) text = "" }
        }
    }
    val field: @Composable (Modifier) -> Unit = { modifier ->
        MqTextField(
            value = text,
            onValueChange = {
                text = it
                onEdit()
            },
            label = "Addon link",
            placeholder = "https://... manifest.json",
            modifier = modifier,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            // The keyboard's Go adds it, as the button does.
            keyboardActions = KeyboardActions(onGo = { add() }),
        )
    }
    val button: @Composable () -> Unit = {
        PrimaryButton(
            label = "Add",
            icon = Icons.Plus,
            enabled = !busy && text.isNotBlank(),
            onClick = add,
            focusRequester = focusRequester,
        )
    }
    Section(side) {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            if (compact) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    field(Modifier.weight(1f))
                    button()
                }
            } else {
                MqText("Add an addon", role = TextRole.Headline, weight = FontWeight.Bold)
                field(Modifier)
                button()
            }
            // Spoken as it changes (TalkBack): the sign-in warning above all, but also how an add went.
            if (status != null) LiveSpoken(status.text) { SaidLine(status, side = 0.dp) }
            if (warned) {
                SecondaryButton(
                    LinkInstall.TOKEN_CONFIRM,
                    {
                        val url = text.trim()
                        if (!busy) onInstall(url, true) { if (text.trim() == url) text = "" }
                    },
                    icon = Icons.ShieldAlert,
                )
            }
        }
    }
}

@Composable
private fun Playback(
    holder: SettingsHolder,
    busy: Boolean,
    run: (String, suspend () -> Unit) -> Unit,
) {
    val prefs = holder.prefs.current() ?: return
    val ops = holder.session.ops
    Section(0.dp, groupRadius()) {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            SettingSwitch(
                "Next episode",
                if (prefs.autoplayNext) "Plays on its own" else "Waits for you",
                prefs.autoplayNext,
            ) {
                if (!busy) {
                    run("Changing autoplay") {
                        ops.setPrefs(buildJsonObject { put("autoplayNext", JsonPrimitive(!prefs.autoplayNext)) })
                    }
                }
            }
        }
    }
}

/**
 * Settings > Subtitles and audio (the owner, 2026-10-27 and 2026-10-31): the subtitle languages, their size and the
 * audio languages; then the second line (plans/dual-subtitles.md 5.5), the same profile values as the player's.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SubtitlesAudio(
    holder: SettingsHolder,
    busy: Boolean,
    run: (String, suspend () -> Unit) -> Unit,
) {
    val prefs = holder.prefs.current() ?: return
    val ops = holder.session.ops
    val set = { what: String, field: String, value: JsonElement ->
        if (!busy) run(what) { ops.setPrefs(buildJsonObject { put(field, value) }) }
    }
    // The core marks the device's language as it lists it ("nb" reads as "no").
    val device = DeviceCaps.languageCode(LocalConfiguration.current.locales[0])
    val load = remember(ops, device) { suspend { ops.languages(device) } }
    Section(0.dp, groupRadius()) {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            SubtitleLanguage(prefs.subtitleLanguages, load) {
                set("Changing the subtitle languages", "subtitleLanguages", json(it))
            }
            MqText("Subtitle size", role = TextRole.Headline, weight = FontWeight.Bold)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                for ((size, words) in SUBTITLE_SIZES) {
                    Chip(words, selected = prefs.subtitleSize == size, onClick = {
                        if (prefs.subtitleSize !=
                            size
                        ) {
                            set("Changing the subtitle size", "subtitleSize", JsonPrimitive(size.wire))
                        }
                    })
                }
            }
            MqText(StreamChoiceCopy.AUDIO, role = TextRole.Headline, weight = FontWeight.Bold)
            Languages(prefs.streams.languages, load, {
                set("Changing the audio languages", "streams", buildJsonObject { put("languages", json(it)) })
            }, audio = true)
            MqText(StreamChoiceCopy.AUDIO_HELP, role = TextRole.Caption, color = Marquee.colors.muted)
        }
    }
    Section(0.dp, groupRadius()) {
        SecondLineRows(
            prefs.secondSubtitle,
            prefs.subtitleLayout,
            prefs.secondOnPause,
            prefs.subtitleLanguages.firstOrNull(),
            load,
            onSecond = { set("Changing the second line", "secondSubtitle", it?.let(::JsonPrimitive) ?: JsonNull) },
            onLayout = { set("Changing the layout", "subtitleLayout", JsonPrimitive(it.wire)) },
            onPaused = { set("Changing the second line", "secondOnPause", JsonPrimitive(it)) },
        )
    }
}

/**
 * The second line's rows (plans/dual-subtitles.md 5.5): Second line (Off, Spoken language, then the languages by
 * search, the first subtitle language's reading "Subtitle language" and not to be picked), then Layout and Only when
 * paused while a second line is set.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun SecondLineRows(
    second: String?,
    layout: SubtitleLayout,
    paused: Boolean,
    /** The profile's first subtitle language, as stored. */
    first: String?,
    load: suspend () -> List<LanguageChoice>,
    onSecond: (String?) -> Unit,
    onLayout: (SubtitleLayout) -> Unit,
    onPaused: (Boolean) -> Unit,
) {
    val all by produceState(emptyList<LanguageChoice>(), load) {
        value =
            try {
                load()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                emptyList()
            }
    }
    var query by remember { mutableStateOf("") }
    val device = DeviceCaps.languageCode(LocalConfiguration.current.locales[0])
    val language = second?.takeIf { it != LanguagePicks.SPOKEN }
    val row = Modifier.fillMaxWidth()
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        // The chosen chip (Off, Spoken language or the language) is the row's value.
        MqText(StreamChoiceCopy.SECOND_LINE, role = TextRole.Headline, weight = FontWeight.Bold)
        MqText(SECOND_LINE_HELP, role = TextRole.Caption, color = Marquee.colors.muted)
        FlowRow(
            row,
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            Chip("Off", selected = second == null, onClick = { if (second != null) onSecond(null) })
            Chip(StreamChoiceCopy.SPOKEN_LANGUAGE, selected = second == LanguagePicks.SPOKEN, onClick = {
                if (second != LanguagePicks.SPOKEN) onSecond(LanguagePicks.SPOKEN)
            })
            language?.let { Chip(LanguagePicks.name(it, all), selected = true, onClick = {}) }
        }
        MqTextField(
            value = query,
            onValueChange = { query = it },
            label = "Find a language",
            placeholder = "Polish, Korean, Arabic",
        )
        FlowRow(
            row,
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            for (choice in LanguagePicks.search(all, query, listOfNotNull(language), device)) {
                key(choice.code) {
                    // The first line's language stays in the list, says why it cannot be picked, and is skipped by
                    // TalkBack's actions; a remote still lands on it, so the focused chip explains itself.
                    val blocked = LanguagePicks.onFirstLine(choice.code, first)
                    Chip(
                        choice.name,
                        selected = false,
                        onClick = {
                            if (!blocked) {
                                onSecond(choice.code)
                                query = ""
                            }
                        },
                        modifier = if (blocked) Modifier.semantics { disabled() } else Modifier,
                        detail = if (blocked) "Subtitle language" else null,
                    )
                }
            }
        }
        if (second != null) {
            MqText(StreamChoiceCopy.LAYOUT, role = TextRole.Headline, weight = FontWeight.Bold)
            FlowRow(
                row,
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                for ((value, words) in StreamChoiceCopy.LAYOUTS) {
                    Chip(words, selected = layout == value, onClick = { if (layout != value) onLayout(value) })
                }
            }
            MqText(LAYOUT_HELP, role = TextRole.Caption, color = Marquee.colors.muted)
            SettingSwitch(StreamChoiceCopy.ONLY_WHEN_PAUSED, PAUSED_HELP, paused) { onPaused(!paused) }
        }
    }
}

internal const val SECOND_LINE_HELP =
    "A second subtitle in another language, with the first. Spoken language follows each film's audio."
internal const val LAYOUT_HELP =
    "Stacked puts the second line above the first, Split at the top of the picture, One box both in one box."
internal const val PAUSED_HELP = "The second line shows only while the video is paused."

/**
 * This device: what this device keeps for itself, never synced (plans/appletv-polish.md decision 13). A TV has it where
 * its title pages may play the header trailer (plans/trailer-inline.md 4a); its Home has none.
 */
@Composable
internal fun DevicePrefs() {
    val context = LocalContext.current
    var previews by remember { mutableStateOf(TrailerPreviews.on(context)) }
    Section(0.dp, groupRadius()) {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            SettingSwitch(
                "Trailer previews",
                if (Marquee.form == Form.Tv) {
                    "Plays trailers muted on title pages, from YouTube, when the connection is not metered."
                } else {
                    "Plays trailers muted on Home and title pages, from YouTube, when the connection is not metered."
                },
                previews,
            ) {
                previews = !previews
                TrailerPreviews.set(context, previews)
            }
            // The switch keeps its value; Incognito plays no trailer (plans/batch-10-incognito.md 5.5).
            if (LocalIncognito.current) {
                MqText(ProfileCopy.INCOGNITO_TRAILERS_OFF, role = TextRole.Caption, color = Marquee.colors.muted)
            }
        }
    }
}

private fun json(list: List<String>) = JsonArray(list.map(::JsonPrimitive))

/** Settings > Subtitles and audio's Subtitle language: the first chosen language named, then the ordered picker. */
@Composable
internal fun SubtitleLanguage(
    chosen: List<String>,
    load: suspend () -> List<LanguageChoice>,
    onChange: (List<String>) -> Unit,
) {
    MqText("Subtitle language", role = TextRole.Headline, weight = FontWeight.Bold)
    Languages(chosen, load, onChange, first = { name -> name?.let { "$it first" } ?: "None, subtitles start off" })
    MqText(
        "Languages are listed in the order you turn them on; the first one wins. " +
            "A subtitle you pick while watching moves its language first.",
        role = TextRole.Caption,
        color = Marquee.colors.muted,
    )
}

/**
 * A language picker over every language the core knows ([load]): the chosen ones first, in the order
 * they were turned on (the first wins), each a press away from off; then a search by name, which
 * offers the common ones before anything is typed. A pick clears the search. [first] words a line over
 * the chips from the first chosen language's name (null for none).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun Languages(
    chosen: List<String>,
    load: suspend () -> List<LanguageChoice>,
    onChange: (List<String>) -> Unit,
    /** Audio takes the base codes ("pt" for "pt-BR"); subtitles take the core's codes as they are. */
    audio: Boolean = false,
    first: ((String?) -> String)? = null,
) {
    val all by produceState(emptyList<LanguageChoice>(), load) {
        value =
            try {
                load().let { if (audio) LanguagePicks.audio(it) else it }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                emptyList()
            }
    }
    var query by remember { mutableStateOf("") }
    // The device's own language is offered first, by the code the core reads ("he", never Java's old "iw").
    val device = DeviceCaps.languageCode(LocalConfiguration.current.locales[0])
    // On a remote, focus follows a picked language to its new place with the chosen ones, so a second
    // press of select never lands on the language that took its slot.
    val tv = Marquee.form == Form.Tv
    val focusers = remember { HashMap<String, FocusRequester>() }
    var follow by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(follow, chosen) {
        val code = follow?.takeIf { it in chosen } ?: return@LaunchedEffect
        withFrameNanos { }
        runCatching { focusers[code]?.requestFocus() }
        follow = null
    }
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        first?.let {
            MqText(
                it(
                    chosen.firstOrNull()?.let { code ->
                        LanguagePicks.name(code, all)
                    },
                ),
                role = TextRole.Callout,
            )
        }
        if (chosen.isNotEmpty()) {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                for (code in chosen) {
                    key(code) {
                        val requester = remember { FocusRequester() }
                        DisposableEffect(requester) {
                            focusers[code] = requester
                            onDispose { focusers.remove(code) }
                        }
                        Chip(
                            LanguagePicks.name(code, all),
                            selected = true,
                            onClick = { onChange(chosen - code) },
                            focusRequester = requester,
                            switch = true,
                        )
                    }
                }
            }
        }
        MqTextField(
            value = query,
            onValueChange = { query = it },
            label = "Find a language",
            placeholder = "Polish, Korean, Arabic",
        )
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            for (choice in LanguagePicks.search(all, query, chosen, device)) {
                key(choice.code) {
                    Chip(
                        choice.name,
                        selected = false,
                        onClick = {
                            onChange(chosen + choice.code)
                            query = ""
                            if (tv) follow = choice.code
                        },
                        switch = true,
                    )
                }
            }
        }
    }
}

@Composable
private fun About() {
    val context = LocalContext.current
    val version =
        remember {
            runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull()
        }
    Section(0.dp, groupRadius()) {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            // The headline under it says the name.
            BrandLockup(
                if (Marquee.form == Form.Tv) 48.dp else 36.dp,
                Modifier.padding(bottom = Marquee.space.xs),
                spoken = false,
            )
            MqText("Momotus ${version.orEmpty()}", role = TextRole.Headline, weight = FontWeight.Bold)
            MqText("Free software under the GPL 3.", role = TextRole.Caption, color = Marquee.colors.muted)
            MqText(
                "Type: Inter and Unbounded (SIL Open Font License). " +
                    "Icons: Lucide (ISC). Flags: circle-flags (MIT).",
                role = TextRole.Caption,
                color = Marquee.colors.muted,
            )
            // TMDB's terms: its logo and this line wherever its data shows.
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Image(Marks.Tmdb, contentDescription = null, modifier = Modifier.height(16.dp))
                MqText(TMDB_CREDIT, role = TextRole.Caption, color = Marquee.colors.muted)
            }
            if (CoreBuild.missing) MqText(CoreBuild.ABOUT_LINE, role = TextRole.Caption, color = Marquee.colors.muted)
        }
    }
}

/** TMDB's attribution line (its terms ask for it with the logo). */
internal const val TMDB_CREDIT = "This product uses the TMDB API but is not endorsed or certified by TMDB."

/** The subtitle sizes Settings offers, in order (the profile's, on every device). */
internal val SUBTITLE_SIZES =
    listOf(
        SubtitleSize.Small to "Small",
        SubtitleSize.Medium to "Medium",
        SubtitleSize.Large to "Large",
        SubtitleSize.ExtraLarge to "Extra large",
    )
