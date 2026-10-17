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
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel

/**
 * A QR code (dark modules on a white quiet zone, as on the web): ECC M with a two-module border.
 * [label] is what a screen reader says.
 */
@Composable
fun Qr(
    text: String,
    label: String,
    size: Dp,
    modifier: Modifier = Modifier,
) {
    val matrix =
        remember(text) {
            val hints = mapOf(EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.M, EncodeHintType.MARGIN to 2)
            // Size 0 asks for one pixel per module; the canvas scales it.
            QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 0, 0, hints)
        }
    Canvas(modifier.size(size).semantics { contentDescription = label }) {
        val cells = matrix.width
        val cell = this.size.minDimension / cells
        drawRect(Color.White)
        for (y in 0 until cells) {
            for (x in 0 until cells) {
                if (matrix[x, y]) drawRect(QR_DARK, Offset(x * cell, y * cell), Size(cell + 0.5f, cell + 0.5f))
            }
        }
    }
}

private val QR_DARK = Color(0xFF0F0D0B)

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
