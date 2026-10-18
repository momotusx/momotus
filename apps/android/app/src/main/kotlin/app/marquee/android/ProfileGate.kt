package app.marquee.android

import androidx.compose.runtime.getValue
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
 * and the switch. While the gate is shut the app shows only the picker; every screen of the old
 * profile has left composition, so their views are closed when the core is asked to switch.
 */
class ProfileGate(
    private val session: CoreSession,
) {
    private var view: ProfilesView? by mutableStateOf(null)

    /** The picker's snapshot, or null until the core sent it (nothing profile-bound opens before). */
    val snapshot: ProfilesSnapshot? get() = view?.snapshot

    /** The start picker was answered this launch (a cold start with more than one profile asks once). */
    var chosen by mutableStateOf(false)
        private set

    /** A switch under way: the profile asked for. The gate is shut until it ends. */
    var switching: String? by mutableStateOf(null)
        private set

    /** Why the last switch failed, shown by the picker that takes over (a wrong PIN, a wait). */
    var switchProblem: CoreError? by mutableStateOf(null)
        private set

    /** Bumped by every switch and lock: work from before it must not touch the screens after. */
    var epoch by mutableStateOf(0)
        private set

    /** Whether the app may show profile content, or only the picker. */
    val open: Boolean
        get() {
            val s = snapshot ?: return false
            return !s.locked && chosen && switching == null
        }

    /** Opens the view once the core runs; the first snapshot decides whether the start picker asks. */
    fun start() {
        session.scope.launch(Dispatchers.Main) {
            snapshotFlow { session.health }.first { it == CoreHealth.Running }
            val v = session.profiles()
            view = v
            val first = snapshotFlow { v.snapshot }.filterNotNull().first()
            if (!ProfileCopy.startsOnPicker(first)) chosen = true
        }
    }

    /** The current profile, unlocked: the start picker's answer. */
    fun markChosen() {
        chosen = true
    }

    /** Unlocks the locked session as its own profile; throws the core's refusal. */
    suspend fun unlock(pin: String?) {
        session.ops.unlockProfile(pin)
        chosen = true
        switchProblem = null
    }

    /**
     * Shuts the gate (every screen of the old profile leaves, closing its views), then switches,
     * waiting while views finish closing (`viewsOpen`). A refusal leaves the gate shut, with the
     * picker showing [switchProblem] for [profile]; [cancelSwitch] goes back to the current profile.
     */
    suspend fun switchTo(
        profile: String,
        pin: String?,
    ): Boolean {
        switching = profile
        switchProblem = null
        epoch++
        // The gate's own work, not the caller's: the picker that asked leaves with the app the moment
        // the gate shuts, and its cancellation must not stop the switch half way.
        return session.scope.async(Dispatchers.Main) { runSwitch(profile, pin) }.await()
    }

    private suspend fun runSwitch(
        profile: String,
        pin: String?,
    ): Boolean {
        try {
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
        }
    }

    /** A failed switch given up: the current profile opens again, on Home. */
    fun cancelSwitch() {
        switching = null
        switchProblem = null
        chosen = true
    }
}
