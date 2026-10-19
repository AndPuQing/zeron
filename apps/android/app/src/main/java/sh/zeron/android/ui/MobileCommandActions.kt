package sh.zeron.android.ui

import kotlinx.coroutines.CoroutineScope
import uniffi.zeron_core.MobileComposerAction
import uniffi.zeron_core.MobileComposerContext

/** Screen-owned actions; neither drafts nor the host command ledger store callbacks. */
class MobileCommandActions(
    val harness: String,
    val scope: CoroutineScope,
    val context: () -> MobileComposerContext,
    val execute: suspend (MobileComposerAction) -> Unit,
)
