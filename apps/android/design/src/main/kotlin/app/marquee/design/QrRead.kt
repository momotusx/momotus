package app.marquee.design

import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader

/**
 * The text of a QR code in a camera frame's brightness (the Y plane of a YUV frame, [rowStride] bytes
 * a row), or null when there is none. ZXing reads it, as it draws the codes this app shows.
 */
fun readQr(
    luma: ByteArray,
    width: Int,
    height: Int,
    rowStride: Int = width,
): String? {
    val source = PlanarYUVLuminanceSource(luma, rowStride, height, 0, 0, width, height, false)
    return try {
        QRCodeReader().decode(BinaryBitmap(HybridBinarizer(source)), mapOf(DecodeHintType.TRY_HARDER to true)).text
    } catch (e: ReaderException) {
        null
    }
}
