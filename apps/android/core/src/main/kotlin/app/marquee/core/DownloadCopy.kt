package app.marquee.core

import app.marquee.core.model.DownloadBadge
import app.marquee.core.model.DownloadReason
import app.marquee.core.model.DownloadRow
import app.marquee.core.model.DownloadState
import app.marquee.core.model.DownloadStorage
import app.marquee.core.model.DownloadTitle
import app.marquee.core.model.GroupDownload
import app.marquee.core.session.DownloadAction
import kotlin.math.ceil
import kotlin.math.roundToInt

/** The download control's glyph (plans/offline-downloads.md 3.1); each tree draws its Lucide icon. */
enum class DownloadGlyph {
    /** `arrow-down-to-line`: nothing downloaded yet. */
    Download,

    /** `square` in the ring: running. */
    Stop,
    Pause,
    WifiOff,
    Clock,
    Check,
    Alert,
}

/** One item of a download's menu. */
enum class DownloadItem(
    val label: String,
) {
    Start("Download"),
    Choose("Choose a version to download"),
    Pause("Pause download"),
    Resume("Resume download"),
    Cancel("Cancel download"),
    Delete("Delete download"),
    Play("Play"),
    Retry("Try again"),
    Another("Choose another version"),
    MobileData("Download now on mobile data"),
    Again("Download again"),
    Remove("Remove"),
}

/**
 * What the control shows for one video. [label] is the word beside or under it where labels show ("45%");
 * [spoken] the state's word for TalkBack ("Downloading"), [value] the ring's "45 percent". [ring] is the ring's
 * fill, null with no ring. [items] is the menu a tap opens; empty, a tap starts the download.
 */
data class DownloadControl(
    val glyph: DownloadGlyph,
    val label: String,
    val spoken: String,
    val value: String?,
    val ring: Float?,
    /** The reason in full, the menu's line under its title. */
    val detail: String?,
    val items: List<DownloadItem>,
    /** The hint TalkBack reads where a tap opens a menu. */
    val hint: String? = if (items.isEmpty()) null else MENU_HINT,
)

/** One item of a series' Download menu. */
data class GroupItem(
    val label: String,
    val act: GroupAct,
)

enum class GroupAct {
    DownloadSeason,
    DownloadSeries,
    DeleteSeason,

    /** A line that says where the season stands; pressing it does nothing. */
    None,
}

private const val MENU_HINT = "Shows download options"

/** The words of downloads, shared by both Android trees and equal to the Apple and web families'. */
object DownloadCopy {
    /** Where nothing is downloaded yet: a tap starts it, a press and hold offers the version. */
    val NONE =
        DownloadControl(DownloadGlyph.Download, "Download", "Download", null, null, null, emptyList())

    /** A video's control: [badge] null is not downloaded. */
    fun control(badge: DownloadBadge?): DownloadControl {
        badge ?: return NONE
        val progress = badge.progress?.coerceIn(0f, 1f)
        val percent = progress?.let { "${(it * 100).roundToInt()}" }
        val value = percent?.let { "$it percent" }
        return when (badge.state) {
            DownloadState.Queued -> {
                DownloadControl(DownloadGlyph.Clock, "Queued", "Queued", null, 0f, null, listOf(DownloadItem.Cancel))
            }

            DownloadState.Running -> {
                DownloadControl(
                    DownloadGlyph.Stop,
                    percent?.let { "$it%" } ?: "Downloading",
                    "Downloading",
                    value,
                    progress ?: 0f,
                    null,
                    listOf(DownloadItem.Pause, DownloadItem.Cancel),
                )
            }

            DownloadState.Paused -> {
                DownloadControl(
                    DownloadGlyph.Pause,
                    "Paused",
                    "Paused",
                    value,
                    progress ?: 0f,
                    null,
                    listOf(DownloadItem.Resume, DownloadItem.Cancel),
                )
            }

            DownloadState.Waiting -> {
                val word = waiting(badge.reason)
                val release = badge.reason == DownloadReason.Wifi || badge.reason == DownloadReason.LowData
                DownloadControl(
                    waitingGlyph(badge.reason),
                    word,
                    word,
                    value,
                    progress ?: 0f,
                    waitingDetail(badge.reason),
                    listOfNotNull(DownloadItem.MobileData.takeIf { release }, DownloadItem.Cancel),
                )
            }

            DownloadState.Done -> {
                DownloadControl(
                    DownloadGlyph.Check,
                    "Downloaded",
                    "Downloaded",
                    null,
                    null,
                    null,
                    listOf(DownloadItem.Play, DownloadItem.Delete),
                )
            }

            DownloadState.Failed -> {
                DownloadControl(
                    DownloadGlyph.Alert,
                    "Couldn't download",
                    "Couldn't download",
                    null,
                    null,
                    failure(badge.reason),
                    listOf(DownloadItem.Retry, DownloadItem.Another, DownloadItem.Cancel),
                )
            }

            DownloadState.Missing -> {
                DownloadControl(
                    DownloadGlyph.Alert,
                    "Download again",
                    "Download again",
                    null,
                    null,
                    "The downloaded file is no longer on this device.",
                    listOf(DownloadItem.Again, DownloadItem.Remove),
                )
            }

            // A state from a newer core: it can still be cancelled.
            DownloadState.Unknown -> {
                DownloadControl(DownloadGlyph.Clock, "Waiting", "Waiting", null, 0f, null, listOf(DownloadItem.Cancel))
            }
        }
    }

    /**
     * The op an item of an existing download sends, and whether it releases the download onto mobile data; null for
     * the items that go somewhere instead (Download, a version, Play).
     */
    fun op(item: DownloadItem): Pair<DownloadAction, Boolean>? =
        when (item) {
            DownloadItem.Pause -> DownloadAction.Pause to false
            DownloadItem.Resume -> DownloadAction.Resume to false
            DownloadItem.Cancel, DownloadItem.Delete, DownloadItem.Remove -> DownloadAction.Remove to false
            DownloadItem.Retry, DownloadItem.Again -> DownloadAction.Retry to false
            DownloadItem.MobileData -> DownloadAction.Retry to true
            DownloadItem.Start, DownloadItem.Choose, DownloadItem.Another, DownloadItem.Play -> null
        }

    /** The waiting label by its reason; a wait for room reads "Waiting", never a storage warning (the owner's rule). */
    fun waiting(reason: DownloadReason?): String =
        when (reason) {
            DownloadReason.Wifi -> "Waiting for Wi-Fi"
            DownloadReason.LowData -> "Waiting: Low Data Mode is on"
            DownloadReason.Space -> "Waiting"
            DownloadReason.Network -> "Waiting for a connection"
            DownloadReason.Playing -> "Paused while you watch"
            DownloadReason.App -> "Opens again when you return to Marquee"
            else -> "Waiting"
        }

    private fun waitingGlyph(reason: DownloadReason?): DownloadGlyph =
        when (reason) {
            DownloadReason.Wifi, DownloadReason.LowData, DownloadReason.Network -> DownloadGlyph.WifiOff
            else -> DownloadGlyph.Clock
        }

    private fun waitingDetail(reason: DownloadReason?): String? =
        when (reason) {
            DownloadReason.Wifi -> "Download on Wi-Fi only is on, and this network uses mobile data."
            DownloadReason.Network -> "It goes on by itself when the connection comes back."
            DownloadReason.Playing -> "It goes on by itself when playback ends."
            else -> null
        }

    /** Why a download failed, in full. */
    fun failure(reason: DownloadReason?): String =
        when (reason) {
            DownloadReason.NoVersion -> "No version of this one can be downloaded"
            DownloadReason.Source -> "The source stopped answering"
            DownloadReason.Retries -> "The download was interrupted too many times"
            DownloadReason.NotVideo -> "The file wasn't a video"
            else -> "Something went wrong"
        }

    /**
     * A series' Download menu (3.2): the selected season's item, then every season's. Counts are aired regular
     * episodes, from the core.
     */
    fun seriesMenu(
        season: Int?,
        seasonGroup: GroupDownload?,
        series: GroupDownload?,
    ): List<GroupItem> =
        buildList {
            if (season != null && seasonGroup != null && seasonGroup.episodes > 0) {
                val left = seasonGroup.episodes - seasonGroup.downloaded
                when {
                    seasonGroup.active > 0 -> add(GroupItem("Downloading Season $season", GroupAct.None))
                    left <= 0 -> add(GroupItem("Season $season downloaded", GroupAct.None))
                    else -> add(GroupItem("Download Season $season (${episodes(left)})", GroupAct.DownloadSeason))
                }
                if (seasonGroup.downloaded > 0) add(GroupItem("Delete Season $season downloads", GroupAct.DeleteSeason))
            }
            if (series != null && series.seasons > 1 && series.downloaded < series.episodes) {
                add(
                    GroupItem(
                        "Download every season (${series.seasons} seasons, ${episodes(series.episodes)})",
                        GroupAct.DownloadSeries,
                    ),
                )
            }
        }

    /** The series' action-row control: Download, or where its downloads stand; a tap always opens its menu. */
    fun seriesControl(series: GroupDownload?): DownloadControl =
        when {
            series == null || series.downloaded == 0 -> {
                NONE
            }

            series.active > 0 -> {
                DownloadControl(DownloadGlyph.Clock, "Downloading", "Downloading", null, null, null, emptyList())
            }

            else -> {
                DownloadControl(DownloadGlyph.Check, "Downloaded", "Downloaded", null, null, null, emptyList())
            }
        }.copy(hint = MENU_HINT)

    fun episodes(n: Int): String = if (n == 1) "1 episode" else "$n episodes"

    /** "Downloads use 12.4 GB · 48 GB free on this device". */
    fun storage(storage: DownloadStorage): String =
        listOfNotNull(
            "Downloads use ${Format.bytes(storage.downloadsBytes)}",
            storage.freeBytes?.let { "${Format.bytes(it)} free on this device" },
        ).joinToString(" · ")

    /** "1.2 of 2.1 GB": the first figure drops the unit both share. */
    fun ofTotal(
        done: Long,
        total: Long,
    ): String {
        val a = Format.bytes(done)
        val b = Format.bytes(total)
        val unit = b.substringAfterLast(' ')
        return if (a.substringAfterLast(' ') == unit) "${a.substringBeforeLast(' ')} of $b" else "$a of $b"
    }

    /** "6 min left", "1 h 5 min left"; under a minute "Less than a minute left". */
    fun left(seconds: Long): String {
        if (seconds < 60) return "Less than a minute left"
        val minutes = ceil(seconds / 60.0).toLong()
        return if (minutes < 60) "$minutes min left" else "${minutes / 60} h ${minutes % 60} min left"
    }

    /** A row of the Downloading section: "1.2 of 2.1 GB · 6 min left", or the state's word where it does not run. */
    fun progressLine(row: DownloadRow): String {
        val bytes =
            row.bytesTotal?.takeIf { it > 0 }?.let { ofTotal(row.bytesDone, it) }
                ?: row.bytesDone.takeIf { it > 0 }?.let(Format::bytes)
        val state =
            when (row.state) {
                DownloadState.Running -> row.etaSecs?.let(::left)
                else -> control(badgeOf(row)).label
            }
        return listOfNotNull(bytes, state).joinToString(" · ")
    }

    /** A kept title's line: a film "1080p · 2.1 GB", a series "4 episodes · 7.8 GB". */
    fun titleLine(title: DownloadTitle): String =
        if (title.kind == "movie") {
            listOfNotNull(title.rows.firstOrNull()?.quality, Format.bytes(title.bytes)).joinToString(" · ")
        } else {
            "${episodes(title.count)} · ${Format.bytes(title.bytes)}"
        }

    /** An episode of a series page: "E3 Name". */
    fun episodeName(row: DownloadRow): String =
        listOfNotNull(row.episode?.let { "E$it" }, row.episodeTitle?.takeIf { it.isNotBlank() })
            .joinToString(" ")
            .ifEmpty { row.name }

    /** Under it: "52 min · 1.9 GB". */
    fun episodeLine(row: DownloadRow): String =
        listOfNotNull(row.runtime, (row.bytesTotal ?: row.bytesDone).takeIf { it > 0 }?.let(Format::bytes))
            .joinToString(" · ")

    /** A row's badge, for the control on the Downloads screen. */
    fun badgeOf(row: DownloadRow): DownloadBadge =
        DownloadBadge(
            id = row.id,
            state = row.state,
            reason = row.reason,
            mobileData = row.mobileData,
            progress = row.bytesTotal?.takeIf { it > 0 }?.let { (row.bytesDone.toDouble() / it).toFloat() },
            bytesTotal = row.bytesTotal,
        )

    fun deleteSeries(
        count: Int,
        name: String,
    ): String = "Delete ${if (count == 1) "1 downloaded episode" else "$count downloaded episodes"} of $name?"

    fun deleteAll(bytes: Long): String = "Delete all downloads on this device? ${Format.bytes(bytes)}"

    const val EMPTY_TITLE = "Nothing downloaded yet"
    const val EMPTY_BODY = "Download movies and episodes to watch them without a connection."
    const val OFFLINE = "You're offline. Your downloads still play."
    const val OFFLINE_ACTION = "Go to Downloads"
    const val TITLE_OFFLINE = "Showing what's downloaded"
    const val NOTIFICATION_ASK = "Marquee shows download progress in a notification."
    const val SMART_LINE =
        "When you finish a downloaded episode, the next one downloads and the one you watched is deleted."
}
