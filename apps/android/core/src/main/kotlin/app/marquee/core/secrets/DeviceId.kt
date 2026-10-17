package app.marquee.core.secrets

import app.marquee.core.client.CoreLog
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.security.SecureRandom

/**
 * The sync device id: random, non-zero (0 means "random, kept in the data dir" inside the core),
 * kept in a plain file under `noBackupFilesDir`. It is not secret, so it stays out of the
 * Keystore blob (a Keystore failure must not rotate it), and `noBackupFilesDir` keeps it out of
 * backups and device transfers, so a restored phone gets a fresh id.
 *
 * A file that exists but cannot be read is not replaced (that would rotate the id on a flaky
 * read); a fresh id that cannot be written is still used for this run.
 */
object DeviceId {
    fun load(
        dir: File,
        random: SecureRandom = SecureRandom(),
    ): ULong {
        val file = File(dir, "device-id")
        if (file.exists()) {
            val text =
                try {
                    file.readText().trim()
                } catch (e: IOException) {
                    // A flaky read: use a throwaway id for this run and leave the stored one alone.
                    CoreLog.w("the device id could not be read, using a temporary one: ${CoreLog.describe(e)}")
                    return newId(random)
                }
            text.toULongOrNull(16)?.takeIf { it != 0UL }?.let { return it }
        }
        val id = newId(random)
        runCatching {
            dir.mkdirs()
            val temp = File(dir, "device-id.tmp")
            FileOutputStream(temp).use {
                it.write(id.toString(16).encodeToByteArray())
                it.fd.sync()
            }
            check(temp.renameTo(file)) { "rename failed" }
        }.onFailure {
            CoreLog.w(
                "the device id could not be stored, using it for this run only: ${CoreLog.describe(it)}",
            )
        }
        return id
    }

    private fun newId(random: SecureRandom): ULong {
        while (true) {
            val id = random.nextLong().toULong()
            if (id != 0UL) return id
        }
    }
}
