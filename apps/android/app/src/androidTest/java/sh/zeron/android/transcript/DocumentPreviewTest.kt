package sh.zeron.android.transcript

import android.os.SystemClock
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.DocumentPreview

class DocumentPreviewTest {
    @Test
    fun firstDocumentFrameStartsAtTopWhileChatStartsAtTail() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        lateinit var reader: TranscriptState
        lateinit var chat: TranscriptState
        instrumentation.runOnMainSync {
            reader = TranscriptState(startsAtTop = true)
            // A conversation the reader scrolled away from still opens at its tail.
            chat = TranscriptState().also { it.following = false }
            val document = DocumentPreview("docs/guide.md",
                (1..80).joinToString("\n\n") { "## Section $it\n\nDocument content." },
                true, false, "host", "/repo")
            for (state in listOf(reader, chat)) {
                state.setViewport(390f, 500f, 1f)
                state.engine.setDocument("chat", document, true)
            }
        }
        try {
            val deadline = SystemClock.uptimeMillis() + 5_000
            var ready = false
            while (!ready && SystemClock.uptimeMillis() < deadline) {
                instrumentation.runOnMainSync {
                    ready = (reader.frame?.rowCount() ?: 0u) > 0u && (chat.frame?.rowCount() ?: 0u) > 0u
                }
                if (!ready) SystemClock.sleep(20)
            }
            assertTrue("Document layout did not arrive", ready)
            instrumentation.runOnMainSync {
                assertTrue(reader.maxOffset > 0f)
                assertEquals(0f, reader.offset, 1f)
                assertEquals(chat.maxOffset, chat.offset, 1f)
            }
        } finally {
            instrumentation.runOnMainSync { reader.close(); chat.close() }
        }
    }
}
