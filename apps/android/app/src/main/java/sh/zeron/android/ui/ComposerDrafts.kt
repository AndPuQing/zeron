package sh.zeron.android.ui

import androidx.lifecycle.ViewModel
import uniffi.zeron_core.CoreClient

/** Keep the entire composer, including attachments and skill identities, across folding/rotation. */
class ComposerDrafts : ViewModel() {
    private var owner: CoreClient? = null
    private val drafts = mutableMapOf<String, ComposerModel>()

    fun forClient(client: CoreClient?): MutableMap<String, ComposerModel> {
        if (owner !== client) {
            drafts.clear()
            owner = client
        }
        return drafts
    }
}
