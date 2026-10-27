package sh.zeron.android.ui

import androidx.compose.runtime.mutableStateMapOf
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/** One catalog per open picker. Discovery adding tabs must not cancel existing requests. */
internal class ModelPickerCatalog(private val scope: CoroutineScope) {
    val rows = mutableStateMapOf<String, List<ModelPickerRow>?>()

    fun load(tabs: List<ModelPickerTab>) {
        for (tab in tabs) {
            if (rows.containsKey(tab.harness)) continue
            rows[tab.harness] = null
            // The picker owns this job, not the effect watching the changing tab list.
            scope.launch {
                rows[tab.harness] = (within(15_000) { tab.load() } ?: emptyList()).distinctBy { it.id }
            }
        }
    }
}
