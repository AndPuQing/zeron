package sh.zeron.android.transcript

import android.os.SystemClock
import androidx.compose.ui.unit.Constraints
import kotlin.math.roundToInt
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
    @Test
    fun largeMarkdownRowsFitComposeAndModeSwitchesReleaseFollow() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        lateinit var reader: TranscriptState
        val document = DocumentPreview("log.md",
            "# Log\n\n```text\n" + "line\n".repeat(20_000) + "```",
            true, false, "host", "/repo")
        instrumentation.runOnMainSync {
            reader = TranscriptState(startsAtTop = true)
            reader.setViewport(390f, 500f, 1f)
            reader.showDocument("chat", document, true)
        }
        fun waitForFrame(after: ULong) {
            val deadline = SystemClock.uptimeMillis() + 10_000
            var ready = false
            while (!ready && SystemClock.uptimeMillis() < deadline) {
                instrumentation.runOnMainSync {
                    ready = reader.frame?.let { it.revision() > after && it.rowCount() >= 200u } == true
                }
                if (!ready) SystemClock.sleep(20)
            }
            assertTrue("Document layout did not arrive", ready)
        }
        try {
            waitForFrame(0uL)
            instrumentation.runOnMainSync {
                val frame = reader.frame!!
                for (i in 0u until frame.rowCount()) {
                    val row = frame.placement(i)!!
                    // Use the real Compose constraint encoder at phone density.
                    Constraints.fixed(1170, (row.height * 3).roundToInt())
                }
            }
            for (markdown in listOf(false, true)) {
                var revision = 0uL
                instrumentation.runOnMainSync {
                    reader.scrollBy(reader.maxOffset)
                    assertTrue("Scrolling to the tail should reproduce follow mode", reader.following)
                    revision = reader.frame!!.revision()
                    reader.showDocument("chat", document, markdown)
                    assertFalse(reader.following)
                    assertEquals(0f, reader.offset, 1f)
                }
                waitForFrame(revision)
                instrumentation.runOnMainSync {
                    assertFalse(reader.following)
                    assertEquals(0f, reader.offset, 1f)
                }
            }
        } finally {
            instrumentation.runOnMainSync { reader.close() }
        }
    }

}
