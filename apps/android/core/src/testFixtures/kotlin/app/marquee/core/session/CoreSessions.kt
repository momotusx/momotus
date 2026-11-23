package app.marquee.core.session

import android.content.Context
import org.junit.rules.ExternalResource
import java.util.concurrent.CopyOnWriteArrayList

/**
 * A test's core sessions, each closed after the test with its state thread and its scope. A session opened in a test
 * and never closed kept its looping thread for the rest of the JVM: 107 of them by the end of the app's tests (CI).
 */
class CoreSessions : ExternalResource() {
    private val opened = CopyOnWriteArrayList<CoreSession>()

    /** A new session over [context], closed after the test. */
    fun open(context: Context): CoreSession = CoreSession(context).also { opened += it }

    override fun after() {
        for (session in opened) session.close()
        opened.clear()
    }
}
