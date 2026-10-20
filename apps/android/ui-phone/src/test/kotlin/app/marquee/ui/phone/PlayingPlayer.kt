package app.marquee.ui.phone

import android.os.Looper
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.text.Cue
import androidx.media3.common.text.CueGroup
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture

/** A player that is ready on a ten-minute file at one minute in, and takes seeks and play or pause. */
internal class PlayingPlayer : SimpleBasePlayer(Looper.getMainLooper()) {
    private var at = 60_000L
    private var wants = true
    private var ready = false
    private var firstFrame = false
    private var cues = emptyList<Cue>()

    override fun getState(): State {
        val state =
            State
                .Builder()
                .setAvailableCommands(
                    Player.Commands
                        .Builder()
                        .addAllCommands()
                        .build(),
                ).setPlayWhenReady(wants, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
                // Constant: the position moves only by a seek, so a test reads exact numbers.
                .setContentPositionMs(PositionSupplier.getConstant(at))
                .setNewlyRenderedFirstFrame(firstFrame)
                .setCurrentCues(CueGroup(cues, at * 1000))
        if (!ready) return state.setPlaybackState(Player.STATE_IDLE).build()
        return state
            .setPlaylist(listOf(MediaItemData.Builder("film").setDurationUs(600_000_000L).build()))
            .setPlaybackState(Player.STATE_READY)
            .build()
    }

    /** The file is open: Media3 says it is ready, and the first frame is drawn. */
    fun open() {
        ready = true
        firstFrame = true
        invalidateState()
        firstFrame = false
    }

    /** A subtitle line on screen now, as the file has it (no line or position of its own). */
    fun say(text: String) {
        cues = listOf(Cue.Builder().setText(text).build())
        invalidateState()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        wants = playWhenReady
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(
        mediaItemIndex: Int,
        positionMs: Long,
        seekCommand: Int,
    ): ListenableFuture<*> {
        at = positionMs
        return Futures.immediateVoidFuture()
    }

    override fun handleSetVideoOutput(videoOutput: Any): ListenableFuture<*> = Futures.immediateVoidFuture()

    override fun handleClearVideoOutput(videoOutput: Any?): ListenableFuture<*> = Futures.immediateVoidFuture()
}
