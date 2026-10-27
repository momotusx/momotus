package app.marquee.design

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.em
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.common.BitMatrix
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import kotlin.math.floor

/**
 * A QR code (dark modules on a white quiet zone, as on the web): ECC M with a two-module border by default; a
 * pairing code asks for ECC L and four modules, to be read from across a room (plans/tv-qr-login.md 5.5). Each
 * module is a whole number of pixels, centred, so no edge blurs. [label] is what a screen reader says.
 */
@Composable
fun Qr(
    text: String,
    label: String,
    size: Dp,
    modifier: Modifier = Modifier,
    /** A pairing code: ECC L and a four-module quiet zone. */
    pairing: Boolean = false,
) {
    val matrix = remember(text, pairing) { qrMatrix(text, pairing) }
    Canvas(modifier.size(size).semantics { contentDescription = label }) {
        val cells = matrix.width
        val cell = qrCell(this.size.minDimension, cells)
        val start = (this.size.minDimension - cell * cells) / 2
        drawRect(Color.White)
        for (y in 0 until cells) {
            for (x in 0 until cells) {
                if (matrix[x, y]) drawRect(QR_DARK, Offset(start + x * cell, start + y * cell), Size(cell, cell))
            }
        }
    }
}

private val QR_DARK = Color(0xFF0F0D0B)

/** The code's modules, quiet zone included, one cell each. */
fun qrMatrix(
    text: String,
    pairing: Boolean,
): BitMatrix {
    val ecc = if (pairing) ErrorCorrectionLevel.L else ErrorCorrectionLevel.M
    val hints = mapOf(EncodeHintType.ERROR_CORRECTION to ecc, EncodeHintType.MARGIN to if (pairing) 4 else 2)
    // Size 0 asks for one pixel per module; the canvas scales it.
    return QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 0, 0, hints)
}

/** Pixels a module takes in a [side] px square: a whole number, at least one. */
fun qrCell(
    side: Float,
    cells: Int,
): Float = floor(side / cells).coerceAtLeast(1f)

/**
 * A code to type on another device (design audit 5): monospace with 0.1 em tracking, read out one
 * character at a time.
 */
@Composable
fun CodeText(
    code: String,
    modifier: Modifier = Modifier,
) {
    MqText(
        code,
        modifier = modifier.semantics { contentDescription = "Code ${code.toList().joinToString(" ")}" },
        role = TextRole.Title1,
        weight = FontWeight.Bold,
        maxLines = 1,
        letterSpacing = 0.1.em,
        fontFamily = FontFamily.Monospace,
    )
}
