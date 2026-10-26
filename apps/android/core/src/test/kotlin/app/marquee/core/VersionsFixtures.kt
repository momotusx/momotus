package app.marquee.core

import app.marquee.core.model.Group
import app.marquee.core.model.Network
import app.marquee.core.model.Playability
import app.marquee.core.model.RankKey
import app.marquee.core.model.ReasonLine
import app.marquee.core.model.SourceClass
import app.marquee.core.model.SourceKind
import app.marquee.core.model.Speed
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsConditions

/** The stream choice fields (M13) every test row and snapshot carries; a test that reads them sets its own. */
internal object VersionsFixtures {
    /** The same fields as JSON, for hand-written rows and snapshots. */
    const val ROW_JSON =
        """"line":{"kind":"quick","seeders":null,"sizeBytes":null},"sourceClass":"ready","kind":"direct",""" +
            """"bitrateMbps":null,""" +
            """"score":{"language":true,"legit":true,"viable":true,"yearOk":true,"original":true,"sustains":true,""" +
            """"playability":"native",""" +
            """"sizeOk":true,"start":3,"binge":false,"effectiveHeight":1080,"within":0}"""

    const val SNAPSHOT_JSON =
        """"conditions":{"screenClass":1080,"hdr":false,"network":"other","http":{"mbps":15,"samples":0},""" +
            """"p2p":{"mbps":10,"samples":0},"freeBytes":null,"runtimeMin":null,"sizeLimitBytes":null,""" +
            """"minSeeders":50,"engine":false,"account":false},"blocked":null,"note":null"""

    val line: ReasonLine = ReasonLine.Quick()

    val sourceClass = SourceClass.Ready

    val kind = SourceKind.Direct

    val score =
        RankKey(
            language = true,
            legit = true,
            viable = true,
            yearOk = true,
            original = true,
            sustains = true,
            playability = Playability.Native,
            sizeOk = true,
            start = 3,
            binge = false,
            effectiveHeight = 1080,
            within = 0,
        )

    val conditions =
        VersionsConditions(
            screenClass = 1080,
            hdr = false,
            network = Network.Other,
            http = Speed(15f, 0),
            p2p = Speed(10f, 0),
            minSeeders = 50,
            engine = false,
            account = false,
        )

    /** A row whose line is `line`, for the line's own tests. */
    fun row(line: ReasonLine = this.line) =
        VersionRow(
            key = "k",
            quality = "1080p",
            status = VersionStatus.Ready("Real-Debrid"),
            via = "Fixture",
            releaseName = "A.1080p.mkv",
            group = Group.Best,
            failoverCount = 0,
            line = line,
            sourceClass = sourceClass,
            kind = kind,
            score = score,
        )
}
