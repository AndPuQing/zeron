package sh.zeron.android.ui

import org.junit.Assert.*
import org.junit.Test

class TextSheetTest {
    @Test fun shortTextIsShownWhole() {
        assertEquals("guide", clipText("guide", TEXT_SHEET_LIMIT))
    }

    @Test fun longTextIsCutWithoutSplittingASurrogatePair() {
        val emoji = "😀"
        assertEquals("ab", clipText("ab$emoji", 3))
        assertEquals("ab$emoji", clipText("ab${emoji}c", 4))
        assertEquals(TEXT_SHEET_LIMIT, clipText("x".repeat(TEXT_SHEET_LIMIT * 2), TEXT_SHEET_LIMIT).length)
    }
}
