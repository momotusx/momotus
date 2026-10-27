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

    /**
     * The TV's pairing QR as drawn (300 dp on a 1080p TV at density 2: 600 px), then blurred (a 3 x 3 box) and
     * halved (2 x 2 means), as a phone camera across a room sees it: still read whole (plans/tv-qr-login.md 7).
     */
    @Test
    fun `the TV's pairing code reads back blurred and at half size`() {
        val code =
            "https://momotus.normalisdead.mom/pair#v=3&s=https%3A%2F%2Fmomotus.normalisdead.mom" +
                "&p=0123456789abcdef0123456789abcdef&k=AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE" +
                "&g=AbCdEfGhIjKlMnOpQrStUvWxYz0123456789-_AbCdE&n=Living%20room%20TV&f=tv"
        val matrix = qrMatrix(code, pairing = true)
        val side = 600
        val whole = qrCell(side.toFloat(), matrix.width)
        assertEquals(whole.toInt().toFloat(), whole, "a whole number of pixels a module")
        val cell = whole.toInt()
        val start = (side - cell * matrix.width) / 2
        val drawn = IntArray(side * side) { 0xF0 }
        for (y in 0 until cell * matrix.width) {
            for (x in 0 until cell * matrix.width) {
                if (matrix[x / cell, y / cell]) drawn[(start + y) * side + start + x] = 0x10
            }
        }
        val blurred = IntArray(side * side)
        for (y in 0 until side) {
            for (x in 0 until side) {
                var sum = 0
                var n = 0
                for (dy in -1..1) {
                    for (dx in -1..1) {
                        val yy = y + dy
                        val xx = x + dx
                        if (yy in 0 until side && xx in 0 until side) {
                            sum += drawn[yy * side + xx]
                            n++
                        }
                    }
                }
                blurred[y * side + x] = sum / n
            }
        }
        val half = side / 2
        val luma = ByteArray(half * half)
        for (y in 0 until half) {
            for (x in 0 until half) {
                val sum =
                    blurred[2 * y * side + 2 * x] + blurred[2 * y * side + 2 * x + 1] +
                        blurred[(2 * y + 1) * side + 2 * x] + blurred[(2 * y + 1) * side + 2 * x + 1]
                luma[y * half + x] = (sum / 4).toByte()
            }
        }
        assertEquals(code, readQr(luma, half, half))
        // The numbers for bench/accounts/tv-qr.jsonl.
        println("tv-qr: modules=${matrix.width} cell=$cell px at $side px, ${cell / 2.0} px a module after halving")
        assertEquals(true, cell >= 6, "at least 6 px a module at TV size")
    }

    @Test
    fun `a frame with no code reads nothing`() {
        assertNull(readQr(ByteArray(320 * 240) { 0x80.toByte() }, 320, 240))
    }
}
