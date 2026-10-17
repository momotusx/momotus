package app.marquee.core

import app.marquee.core.model.Episode
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.TitleDetails

/** The words of the title page, shared by both UI trees. */
object TitleCopy {
    /** "Play", "Play S1:E1", "Resume S1:E2 from 41:12". */
    fun playLabel(target: PlayTarget): String {
        val at = target.resumeMs?.takeIf { it > 0 }?.let { Format.clock(it / 1000) }
        return if (at == null) target.label else "${target.label} from $at"
    }

    /** "2010 · 52 min · Rated 7.8 · Animation, Short": what the metadata gives, nothing invented. */
    fun meta(details: TitleDetails): String =
        listOfNotNull(
            details.releaseInfo,
            details.runtime,
            details.rating?.let { "Rated $it" },
            details.genres
                .take(3)
                .takeIf { it.isNotEmpty() }
                ?.joinToString(", "),
        ).joinToString(" · ")

    /** "Watched, S1:E2, The Kingsroad": what a screen reader says after the tile, and the line under it. */
    fun episode(episode: Episode): String =
        listOfNotNull(
            "Watched".takeIf { episode.watched },
            Format.episodeLabel(episode.season, episode.episode),
            episode.title.takeIf { it.isNotBlank() },
        ).joinToString(", ")
}
