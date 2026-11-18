package sh.zeron.android.ui

import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.zeron_core.ComposerCommand

class CommandSuggestionDisplayTest {
    @Test fun detailMatchesDesktopComposition() {
        val command = ComposerCommand("\$review", "Review changes", "files", "insertion", "Desktop only")
        assertEquals("Review changes · Desktop only · <files>", commandDetail(command))
        assertEquals("", commandDetail(command.copy(description = "", inputHint = null, unavailableReason = null)))
    }
}
