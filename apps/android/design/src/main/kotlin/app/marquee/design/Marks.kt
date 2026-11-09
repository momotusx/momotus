package app.marquee.design

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.graphics.vector.group
import androidx.compose.ui.unit.dp

/**
 * The services' own marks for the title page's ratings row, drawn from design/marks (their sources are listed
 * there; MarksTest keeps the path data equal to those files). Never recoloured or altered.
 */
object Marks {
    private fun nodes(d: String) = PathParser().parsePathString(d).toNodes()

    /** IMDb's yellow badge, 64x32. */
    val Imdb: ImageVector by lazy {
        ImageVector
            .Builder("IMDb", 64.dp, 32.dp, 64f, 32f)
            .addPath(nodes(IMDB_BADGE), fill = SolidColor(Color(0xFFF5C518)))
            .group(translationX = 8f, translationY = 7f) {
                addPath(nodes(IMDB[0]), fill = SolidColor(Color.Black))
                addPath(nodes(IMDB[1]), fill = SolidColor(Color.Black))
                addPath(nodes(IMDB[2]), fill = SolidColor(Color.Black))
                addPath(nodes(IMDB[3]), fill = SolidColor(Color.Black))
            }.build()
    }

    /** TMDB's "Primary short (blue)" square, 190.24x81.52, with its gradient. */
    val Tmdb: ImageVector by lazy {
        ImageVector
            .Builder("TMDB", 190.24.dp, 81.52.dp, 190.24f, 81.52f)
            .addPath(
                nodes(TMDB),
                fill =
                    Brush.linearGradient(
                        0f to Color(0xFF90CEA1),
                        0.56f to Color(0xFF3CBEC9),
                        1f to Color(0xFF00B3E5),
                        start = Offset(0f, 40.76f),
                        end = Offset(190.24f, 40.76f),
                    ),
            ).build()
    }

    /** Trakt's "Logo Mark", 48x48. */
    val Trakt: ImageVector by lazy {
        ImageVector
            .Builder("Trakt", 48.dp, 48.dp, 48f, 48f)
            .addPath(nodes(TRAKT_SQUARE), fill = SolidColor(Color(0xFF6438D0)))
            .group(translationX = 9.6f, translationY = 9.6f, scaleX = 0.47213f, scaleY = 0.47213f) {
                addPath(
                    nodes(TRAKT[0]),
                    stroke = SolidColor(Color.White),
                    strokeLineWidth = 6f,
                    strokeLineCap = StrokeCap.Round,
                    strokeLineJoin = StrokeJoin.Round,
                )
                addPath(
                    nodes(TRAKT[1]),
                    stroke = SolidColor(Color.White),
                    strokeLineWidth = 6f,
                    strokeLineCap = StrokeCap.Round,
                    strokeLineJoin = StrokeJoin.Round,
                )
            }.build()
    }

    /** Google's "G", 48x48, in its four colours: Continue with Google (plans/first-screens.md 3.3). */
    val Google: ImageVector by lazy {
        ImageVector
            .Builder("Google", 48.dp, 48.dp, 48f, 48f)
            .apply { GOOGLE.forEach { (fill, d) -> addPath(nodes(d), fill = SolidColor(Color(fill))) } }
            .build()
    }

    /** Facebook's round "f" mark, 40x40, white on its blue: Continue with Facebook. */
    val Facebook: ImageVector by lazy {
        ImageVector
            .Builder("Facebook", 40.dp, 40.dp, 40f, 40f)
            .addPath(nodes(FACEBOOK[0]), fill = SolidColor(Color.White))
            .addPath(nodes(FACEBOOK[1]), fill = SolidColor(Color(FACEBOOK_BLUE)))
            .build()
    }

    /** Facebook's blue, the button's fill. */
    const val FACEBOOK_BLUE = 0xFF1877F2

    internal val GOOGLE =
        listOf(
            0xFFEA4335 to
                "M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z",
            0xFF4285F4 to
                "M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z",
            0xFFFBBC05 to
                "M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z",
            0xFF34A853 to
                "M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z",
        )
    internal val FACEBOOK =
        listOf(
            "M16.7 39.8C7.2 38.1 0 29.9 0 20 0 9 9 0 20 0s20 9 20 20c0 9.9-7.2 18.1-16.7 19.8l-1.1-.9h-4.4l-1.1.9z",
            "m27.8 25.6.9-5.6h-5.3v-3.9c0-1.6.6-2.8 3-2.8H29V8.2c-1.4-.2-3-.4-4.4-.4-4.6 0-7.8 2.8-7.8 7.8V20h-5v5.6h5v14.1c1.1.2 2.2.3 3.3.3 1.1 0 2.2-.1 3.3-.3V25.6h4.4z",
        )

    /** The rounded rectangles the SVGs draw with `rect`, as paths. */
    private const val IMDB_BADGE =
        "M4,0 H60 A4,4 0 0 1 64,4 V28 A4,4 0 0 1 60,32 H4 A4,4 0 0 1 0,28 V4 A4,4 0 0 1 4,0 Z"
    private const val TRAKT_SQUARE =
        "M10.8,0 H37.2 A10.8,10.8 0 0 1 48,10.8 V37.2 A10.8,10.8 0 0 1 37.2,48 " +
            "H10.8 A10.8,10.8 0 0 1 0,37.2 V10.8 A10.8,10.8 0 0 1 10.8,0 Z"

    /** IMDb's letters: the I (the SVG's polygon), M, D and b. */
    internal val IMDB =
        listOf(
            "M0,18 L5,18 L5,0 L0,0 Z",
            "M15.6725178,0 L14.5534833,8.40846934 L13.8582008,3.83502426 C13.65661,2.37009263 13.4632474,1.09175121 13.278113,0 L7,0 L7,18 L11.2416347,18 L11.2580911,6.11380679 L13.0436094,18 L16.0633571,18 L17.7583653,5.8517865 L17.7707076,18 L22,18 L22,0 L15.6725178,0 Z",
            "M24,18 L24,0 L31.8045586,0 C33.5693522,0 35,1.41994415 35,3.17660424 L35,14.8233958 C35,16.5777858 33.5716617,18 31.8045586,18 L24,18 Z M29.8322479,3.2395236 C29.6339219,3.13233348 29.2545158,3.08072342 28.7026524,3.08072342 L28.7026524,14.8914865 C29.4312846,14.8914865 29.8796736,14.7604764 30.0478195,14.4865461 C30.2159654,14.2165858 30.3021941,13.486105 30.3021941,12.2871637 L30.3021941,5.3078959 C30.3021941,4.49404499 30.272014,3.97397442 30.2159654,3.74371416 C30.1599168,3.5134539 30.0348852,3.34671372 29.8322479,3.2395236 Z",
            "M44.4299079,4.50685823 L44.749518,4.50685823 C46.5447098,4.50685823 48,5.91267586 48,7.64486762 L48,14.8619906 C48,16.5950653 46.5451816,18 44.749518,18 L44.4299079,18 C43.3314617,18 42.3602746,17.4736618 41.7718697,16.6682739 L41.4838962,17.7687785 L37,17.7687785 L37,0 L41.7843263,0 L41.7843263,5.78053556 C42.4024982,5.01015739 43.3551514,4.50685823 44.4299079,4.50685823 Z M43.4055679,13.2842155 L43.4055679,9.01907814 C43.4055679,8.31433946 43.3603268,7.85185468 43.2660746,7.63896485 C43.1718224,7.42607505 42.7955881,7.2893916 42.5316822,7.2893916 C42.267776,7.2893916 41.8607934,7.40047379 41.7816216,7.58767002 L41.7816216,9.01907814 L41.7816216,13.4207851 L41.7816216,14.8074788 C41.8721037,15.0130276 42.2602358,15.1274059 42.5316822,15.1274059 C42.8031285,15.1274059 43.1982131,15.0166981 43.281155,14.8074788 C43.3640968,14.5982595 43.4055679,14.0880581 43.4055679,13.2842155 Z",
        )
    internal const val TMDB =
        "M105.67,36.06h66.9A17.67,17.67,0,0,0,190.24,18.4h0A17.67,17.67,0,0,0,172.57.73h-66.9A17.67,17.67,0,0,0,88,18.4h0A17.67,17.67,0,0,0,105.67,36.06Zm-88,45h76.9A17.67,17.67,0,0,0,112.24,63.4h0A17.67,17.67,0,0,0,94.57,45.73H17.67A17.67,17.67,0,0,0,0,63.4H0A17.67,17.67,0,0,0,17.67,81.06ZM10.41,35.42h7.8V6.92h10.1V0H.31v6.9h10.1Zm28.1,0h7.8V8.25h.1l9,27.15h6l9.3-27.15h.1V35.4h7.8V0H66.76l-8.2,23.1h-.1L50.31,0H38.51ZM152.43,55.67a15.07,15.07,0,0,0-4.52-5.52,18.57,18.57,0,0,0-6.68-3.08,33.54,33.54,0,0,0-8.07-1h-11.7v35.4h12.75a24.58,24.58,0,0,0,7.55-1.15A19.34,19.34,0,0,0,148.11,77a16.27,16.27,0,0,0,4.37-5.5,16.91,16.91,0,0,0,1.63-7.58A18.5,18.5,0,0,0,152.43,55.67ZM145,68.6A8.8,8.8,0,0,1,142.36,72a10.7,10.7,0,0,1-4,1.82,21.57,21.57,0,0,1-5,.55h-4.05v-21h4.6a17,17,0,0,1,4.67.63,11.66,11.66,0,0,1,3.88,1.87A9.14,9.14,0,0,1,145,59a9.87,9.87,0,0,1,1,4.52A11.89,11.89,0,0,1,145,68.6Zm44.63-.13a8,8,0,0,0-1.58-2.62A8.38,8.38,0,0,0,185.63,64a10.31,10.31,0,0,0-3.17-1v-.1a9.22,9.22,0,0,0,4.42-2.82,7.43,7.43,0,0,0,1.68-5,8.42,8.42,0,0,0-1.15-4.65,8.09,8.09,0,0,0-3-2.72,12.56,12.56,0,0,0-4.18-1.3,32.84,32.84,0,0,0-4.62-.33h-13.2v35.4h14.5a22.41,22.41,0,0,0,4.72-.5,13.53,13.53,0,0,0,4.28-1.65,9.42,9.42,0,0,0,3.1-3,8.52,8.52,0,0,0,1.2-4.68A9.39,9.39,0,0,0,189.66,68.47ZM170.21,52.72h5.3a10,10,0,0,1,1.85.18,6.18,6.18,0,0,1,1.7.57,3.39,3.39,0,0,1,1.22,1.13,3.22,3.22,0,0,1,.48,1.82,3.63,3.63,0,0,1-.43,1.8,3.4,3.4,0,0,1-1.12,1.2,4.92,4.92,0,0,1-1.58.65,7.51,7.51,0,0,1-1.77.2h-5.65Zm11.72,20a3.9,3.9,0,0,1-1.22,1.3,4.64,4.64,0,0,1-1.68.7,8.18,8.18,0,0,1-1.82.2h-7v-8h5.9a15.35,15.35,0,0,1,2,.15,8.47,8.47,0,0,1,2.05.55,4,4,0,0,1,1.57,1.18,3.11,3.11,0,0,1,.63,2A3.71,3.71,0,0,1,181.93,72.72Z"
    internal val TRAKT =
        listOf(
            "M56.00 38.95A27.5 27.5 0 1 1 36.19 3.20",
            "M16.93 27.11L35.82 42.96M29.92 34.75L53.89 6.17M56.69 18.31L42.49 35.24M26.88 22.41L23.36 19.45",
        )
}
