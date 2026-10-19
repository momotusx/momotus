package app.marquee.design

import com.google.zxing.BarcodeFormat
import com.google.zxing.qrcode.QRCodeWriter
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class QrReadTest {
    /** A frame's Y plane with [text] drawn as a QR code at [scale] pixels a module, rows padded to [stride]. */
    private fun frame(
        text: String,
        scale: Int,
        stride: Int,
    ): Triple<ByteArray, Int, Int> {
        val matrix = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 0, 0)
        val side = matrix.width * scale
        val luma = ByteArray(stride * side) { 0xF0.toByte() }
        for (y in 0 until side) {
            for (x in 0 until side) if (matrix[x / scale, y / scale]) luma[y * stride + x] = 0x10
        }
        return Triple(luma, side, side)
    }

    @Test
    fun `a pairing code in a frame reads back, row padding and all`() {
        val code = "marquee:pair?v=2&s=http%3A%2F%2F127.0.0.1%3A17803&p=abc&k=key&g=sig"
        val (luma, w, h) = frame(code, scale = 6, stride = 64 * 6 + 32)
        assertEquals(code, readQr(luma, w, h, rowStride = 64 * 6 + 32))
    }

    @Test
    fun `a frame with no code reads nothing`() {
        assertNull(readQr(ByteArray(320 * 240) { 0x80.toByte() }, 320, 240))
    }
}
