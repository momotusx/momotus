package app.marquee.android

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import app.marquee.core.DownloadCopy
import app.marquee.core.DownloadItem
import app.marquee.core.GroupAct
import app.marquee.core.client.CoreLog
import app.marquee.core.model.DownloadBadge
import app.marquee.core.session.CoreSession
import app.marquee.core.session.DownloadAction
import app.marquee.core.session.DownloadScope
import app.marquee.core.session.DownloadTarget
import app.marquee.design.ActionMenu
import app.marquee.design.MenuAction
import app.marquee.ui.phone.TitleDownloads
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Downloads started and changed from the screens (plans/offline-downloads.md 3.3), on the activity's scope so a page
 * that closes does not cancel them. Every start counts in [starts], which the notification ask follows.
 */
class DownloadActions(
    private val session: CoreSession,
    private val caps: CapsSource,
    private val scope: CoroutineScope,
) {
    var starts by mutableIntStateOf(0)
        private set

    /**
     * Why the last add or op did not go through, one line the pages show; never silence (code review r1). It clears
     * after [PROBLEM_MS], at the next op that goes through, or when the user moves to another page (r2 n3).
     */
    var problem by mutableStateOf<String?>(null)
        private set
    private var clearing: Job? = null

    fun clearProblem() {
        clearing?.cancel()
        problem = null
    }

    private fun fail(line: String) {
        clearing?.cancel()
        problem = line
        clearing =
            scope.launch {
                delay(PROBLEM_MS)
                problem = null
            }
    }

    fun add(
        kind: String,
        itemId: String,
        what: DownloadScope,
        videoId: String? = null,
        season: Int? = null,
        pick: Pair<Long, String>? = null,
    ) {
        starts++
        scope.launch {
            try {
                session.ops.downloadAdd(kind, itemId, what, caps(), videoId, season, pick)
                clearProblem()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                CoreLog.w("download could not start: ${CoreLog.describe(e)}")
                fail(DownloadCopy.ADD_FAILED)
            }
        }
    }

    fun act(
        action: DownloadAction,
        which: DownloadTarget,
        mobileData: Boolean = false,
    ) {
        if (action == DownloadAction.Resume || action == DownloadAction.Retry) starts++
        scope.launch {
            try {
                session.ops.downloadAct(action, which, mobileData)
                clearProblem()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                CoreLog.w("download ${action.op} failed: ${CoreLog.describe(e)}")
                fail(DownloadCopy.ACT_FAILED)
            }
        }
    }

    /** An item of an existing download's menu that is an op; the rest go somewhere and are the caller's. */
    fun item(
        item: DownloadItem,
        badge: DownloadBadge?,
    ): Boolean {
        val (action, mobileData) = DownloadCopy.op(item) ?: return false
        badge?.let { act(action, DownloadTarget.Ids(listOf(it.id)), mobileData) }
        return true
    }

    /** What a title page's controls do: [onVersion] opens Versions to pick a download's version, [onPlay] plays. */
    fun forTitle(
        kind: String,
        itemId: String,
        onVersion: (videoId: String) -> Unit,
        onPlay: (videoId: String) -> Unit,
    ): TitleDownloads =
        object : TitleDownloads {
            override val problem: String?
                get() = this@DownloadActions.problem

            override fun item(
                item: DownloadItem,
                videoId: String,
                badge: DownloadBadge?,
            ) {
                when (item) {
                    DownloadItem.Start -> add(kind, itemId, DownloadScope.Video, videoId)
                    DownloadItem.Choose, DownloadItem.Another -> onVersion(videoId)
                    DownloadItem.Play -> onPlay(videoId)
                    else -> this@DownloadActions.item(item, badge)
                }
            }

            override fun group(
                act: GroupAct,
                season: Int?,
                ids: List<String>,
            ) {
                when (act) {
                    GroupAct.DownloadSeason -> {
                        add(kind, itemId, DownloadScope.Season, season = season)
                    }

                    GroupAct.DownloadSeries -> {
                        add(kind, itemId, DownloadScope.Series)
                    }

                    GroupAct.DeleteSeason -> {
                        if (ids.isNotEmpty()) {
                            this@DownloadActions.act(
                                DownloadAction.Remove,
                                DownloadTarget.Ids(ids),
                            )
                        }
                    }

                    GroupAct.None -> {
                        Unit
                    }
                }
            }
        }
}

/**
 * The first download on Android 13 and later asks for the notification permission once, with a line before it
 * (3.3). A refusal stops nothing: the job's notification then shows only in the system's Task Manager.
 */
@Composable
fun NotificationAsk(actions: DownloadActions) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return
    val context = LocalContext.current
    val prefs = remember { context.getSharedPreferences(PREFS, Context.MODE_PRIVATE) }
    var asked by remember { mutableStateOf(prefs.getBoolean(ASKED, false)) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    val granted =
        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
    if (!asksForNotifications(Build.VERSION.SDK_INT, actions.starts, asked, granted)) return
    val done = {
        asked = true
        prefs.edit { putBoolean(ASKED, true) }
    }
    ActionMenu(
        title = DownloadCopy.NOTIFICATION_ASK,
        actions =
            listOf(
                MenuAction("Continue", null) {
                    done()
                    launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
                },
            ),
        onDismiss = done,
    )
}

/** Android 13 and later, after a download started, once, and never when notifications are allowed already. */
internal fun asksForNotifications(
    sdk: Int,
    starts: Int,
    asked: Boolean,
    granted: Boolean,
): Boolean = sdk >= Build.VERSION_CODES.TIRAMISU && starts > 0 && !asked && !granted

/** As long as the web's status line stays. */
internal const val PROBLEM_MS = 6_000L

private const val PREFS = "downloads"
private const val ASKED = "notificationsAsked"
