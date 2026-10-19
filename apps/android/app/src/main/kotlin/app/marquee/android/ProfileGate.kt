package app.marquee.android

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import app.marquee.core.ProfileCopy
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.session.CoreHealth
import app.marquee.core.session.CoreSession
import app.marquee.core.views.ProfilesView
import app.marquee.core.views.ViewStatus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/**
 * The session's profile for the app's life (plans/m11-profiles.md, P1 apps A, B and v2 3 to 6): one
 * `profiles` view the core keeps open across switches and locks, whether profile content may show,
 * and the switch.
 *
 * The gate shuts for a switch, a lock, and a core that stopped; each shut counts in [shutCount]. The
 * app answers a shut by dropping everything of the old profile once its screens have left
 * composition, then calls [resetDone] with that count; a switch waits for that before it asks the
 * core, which switches only with the picker open.
 */
class ProfileGate(
    private val session: CoreSession,
    /** Runs after the gate shut and the app let go of the old profile, right before `switchProfile`. */
    private val beforeSwitch: suspend () -> Unit = {},
) {
    private var view: ProfilesView? by mutableStateOf(null)

    /**
     * The picker's snapshot, from the core that runs now: null until it sent one, and from a core's
     * death until the next core's own snapshot (nothing profile-bound shows meanwhile).
     */
    val snapshot: ProfilesSnapshot? get() = view?.snapshot?.takeIf { current }

    /** Why the profiles could not be read, while the gate tries again. */
    var problem: String? by mutableStateOf(null)
        private set

    /** The start picker was answered this launch (a cold start with more than one profile asks once). */
    var chosen by mutableStateOf(false)
        private set

    /** The core runs and its profiles snapshot came after it started: nothing shows from a core before. */
    private var current by mutableStateOf(false)

    /** A switch under way or refused: the profile asked for. The gate is shut while set. */
    var switching: String? by mutableStateOf(null)
        private set

    /** A switch request is with the core: it cannot be called off any more. */
    var inFlight by mutableStateOf(false)
        private set

    /** Why the last switch failed, shown by the root picker on that profile's keypad. */
    var switchProblem: CoreError? by mutableStateOf(null)
        private set

    /** Shuts so far: a switch, a lock, or a core that stopped. */
    var shutCount by mutableIntStateOf(0)
        private set

    /** The last shut was a core that stopped: the app keeps where it started (onboarding stays onboarding). */
    var restarted = false
        private set

    /** What [chosen] was when the switch began: Cancel goes back to it (the start picker stays a question). */
    private var chosenBefore = false

    private var resetCount by mutableIntStateOf(0)

    /** Whether the app may show profile content, or only the picker. */
    val open: Boolean
        get() {
            val s = snapshot ?: return false
            return current && !s.locked && chosen && switching == null
        }

    /** Opens the view once the core runs, decides the start picker on the first snapshot, and follows restarts and locks. */
    fun start() {
        session.scope.launch(Dispatchers.Main) {
            snapshotFlow { session.health }.first { it == CoreHealth.Running }
            val v = session.profiles()
            view = v
            awaitSnapshot(v, 0)
            val first = v.snapshot ?: return@launch
            if (!ProfileCopy.startsOnPicker(first)) chosen = true
            current = true
            // A core that stops shuts the gate; the next core's own snapshot opens it (a protected
            // profile starts that core locked). The count of snapshots is taken at the shut: nothing
            // from the dead core raises it after that, and the new core's view may come back before
            // this collector hears that it runs.
            var mark = v.opens
            launch {
                snapshotFlow { session.health == CoreHealth.Running }.collect { running ->
                    if (!running && current) {
                        mark = v.opens
                        current = false
                        shut(restart = true)
                    } else if (running && !current) {
                        awaitSnapshot(v, mark)
                        current = true
                    }
                }
            }
            // A lock while open (another device removed this profile, or a lock asked for).
            launch {
                var was = first.locked
                snapshotFlow { v.snapshot?.locked }.filterNotNull().collect { locked ->
                    if (locked && !was) shut()
                    was = locked
                }
            }
        }
    }

    /**
     * Waits for a snapshot after [mark] of them. A view that failed (its resubscribes ran out, or a
     * subscribe timed out) tries again every two seconds, with the reason in [problem] meanwhile.
     */
    private suspend fun awaitSnapshot(
        v: ProfilesView,
        mark: Int,
    ) {
        while (true) {
            snapshotFlow { v.opens > mark || v.status is ViewStatus.Failed }.first { it }
            if (v.opens > mark) break
            problem = (v.status as? ViewStatus.Failed)?.reason
            CoreLog.w("profiles view failed, trying again: $problem")
            delay(2_000)
            // A core that came back revives the view itself: a snapshot meanwhile ends the wait,
            // and only a view still failed, on a core that runs, opens again.
            if (v.opens > mark) break
            if (v.status is ViewStatus.Failed && session.health == CoreHealth.Running) v.retry()
        }
        problem = null
    }

    private fun shut(restart: Boolean = false): Int {
        restarted = restart
        shutCount++
        return shutCount
    }

    /** The app dropped what the old profile left, after shut number [count]. */
    fun resetDone(count: Int) {
        if (count > resetCount) resetCount = count
    }

    /** The current profile, unlocked: the start picker's answer. */
    fun markChosen() {
        chosen = true
    }

    /** Unlocks the locked session as its own profile; throws the core's refusal. */
    suspend fun unlock(pin: String?) {
        session.ops.unlockProfile(pin)
        chosen = true
        switching = null
        switchProblem = null
    }

    /**
     * Shuts the gate, waits for the app to drop the old profile, then switches, waiting while views
     * finish closing (`viewsOpen`). A refusal leaves the gate shut, with the root picker showing
     * [switchProblem] on [profile]'s keypad; [cancelSwitch] goes back to the current profile. One
     * switch at a time; it runs in the gate's own scope, so the picker that asked can leave.
     */
    suspend fun switchTo(
        profile: String,
        pin: String?,
    ): Boolean {
        if (inFlight) return false
        inFlight = true
        if (switching == null) chosenBefore = chosen
        switching = profile
        switchProblem = null
        val count = shut()
        return session.scope.async(Dispatchers.Main) { runSwitch(profile, pin, count) }.await()
    }

    private suspend fun runSwitch(
        profile: String,
        pin: String?,
        count: Int,
    ): Boolean {
        try {
            // The switch waits for the app to say the old profile's screens left (the core switches
            // only with the picker open). A frame clock paused in the background runs no reset:
            // the wait then lasts until the app is in front again and its root answers. Elapsed
            // time is never taken for an answer.
            snapshotFlow { resetCount }.first { it >= count }
            // A Trakt record already on its way reaches the core before the switch (TraktAccount.quiet).
            beforeSwitch()
            for (attempt in 1..10) {
                try {
                    session.ops.switchProfile(profile, pin)
                    chosen = true
                    switching = null
                    return true
                } catch (e: CoreError) {
                    if (e.code == "viewsOpen" && attempt < 10) {
                        delay(100)
                        continue
                    }
                    if (e.code == "viewsOpen") CoreLog.w("switch: views still open: ${e.views}")
                    switchProblem = e
                    return false
                }
            }
            return false
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            switchProblem = CoreError(e.message ?: "The switch did not finish.")
            return false
        } finally {
            inFlight = false
        }
    }

    /**
     * A refused switch given up. From the app, the current profile opens again, on Home; from the
     * start picker, the picker asks again. Not while a request is out.
     */
    fun cancelSwitch() {
        if (inFlight) return
        switching = null
        switchProblem = null
        chosen = chosenBefore
    }
}
