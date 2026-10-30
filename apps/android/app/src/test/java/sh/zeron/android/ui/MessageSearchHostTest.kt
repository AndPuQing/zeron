package sh.zeron.android.ui

import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.DeviceView

class MessageSearchHostTest {
    private fun host(id: String, online: Boolean) = DeviceView(
        id, "Computer $id", "macos", online, null, null, emptyList(), true, false, 0u,
    )

    @Test fun initialChoiceWaitsForDevicesAndPrefersOnline() {
        assertNull(retainedSearchHost(null, emptyList()))
        assertEquals("b", retainedSearchHost(null, listOf(host("a", false), host("b", true)))?.id)
        assertEquals("a", retainedSearchHost(null, listOf(host("a", false)))?.id)
    }

    @Test fun presenceChangesDoNotReplaceTheInitialChoice() {
        val selected = retainedSearchHost(null, listOf(host("a", false), host("b", true)))
        val next = retainedSearchHost(selected, listOf(host("a", true), host("b", false)))
        assertEquals("b", next?.id)
        assertFalse(next!!.online)
    }

    @Test fun explicitSelectionSurvivesRosterGapsAndRecovers() {
        val selected = host("b", true)
        val remaining = listOf(host("a", true))
        val missing = retainedSearchHost(selected, remaining)
        assertEquals(selected, missing)
        assertNull(remaining.firstOrNull { it.id == missing?.id }) // unavailable, not cached online
        assertEquals(selected, retainedSearchHost(missing, emptyList()))
        val renamed = selected.copy(name = "Renamed", online = false)
        assertEquals(renamed, retainedSearchHost(missing, listOf(host("a", true), renamed)))
        assertEquals(selected, retainedSearchHost(renamed, listOf(selected)))
        assertEquals("a", retainedSearchHost(host("a", true), remaining)?.id) // explicit replacement
    }
}
