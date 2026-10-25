package sh.zeron.android.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.net.Uri
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.zeron_core.FileMatch
import uniffi.zeron_core.OutgoingAttachment
import uniffi.zeron_core.fileMentionLink
import java.io.ByteArrayOutputStream
import java.util.UUID
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/** An image staged in the composer before sending (JPEG, long side ≤ 2560px). */
class StagedImage(val id: String, val name: String, val bytes: ByteArray, val thumb: ImageBitmap) {
    val outgoing get() = OutgoingAttachment(name, "image/jpeg", bytes)
}

/**
 * What's being composed: the prompt (with its cursor, for `@` mentions),
 * staged images, and the mention tokens it shows.
 */
@Stable
class ComposerModel(initial: String = "") {
    var value by mutableStateOf(TextFieldValue(initial, TextRange(initial.length)))
    val images = mutableStateListOf<StagedImage>()
    /** `@name` shown in the composer → the file it stands for. */
    private val tokens = LinkedHashMap<String, FileMatch>()
    var commandContext: String = ""
        set(value) {
            if (field.isNotEmpty() && field != value) {
                commands.keys.toList().forEach { token ->
                    commands[token] = commands.getValue(token).copy(unavailableReason = "Context changed")
                }
            }
            field = value
        }
    private val commands = mutableStateMapOf<String, uniffi.zeron_core.ComposerCommand>()
    var localActions: MobileCommandActions? = null
    private var localPending by mutableStateOf(false)
    private var localFailure by mutableStateOf<Pair<String, String>?>(null)
    val localError: String? get() = localFailure?.takeIf { it.first == text }?.second

    fun displayCommand(command: uniffi.zeron_core.ComposerCommand): uniffi.zeron_core.ComposerCommand {
        val actions = localActions ?: return command
        val local = uniffi.zeron_core.mobileComposerAction(actions.harness, command.insertion) ?: return command
        return command.copy(description = local.description, unavailableReason =
            if (localPending) "An action is already in progress." else
                uniffi.zeron_core.mobileComposerActionError(local.action, actions.context()))
    }

    /** True means this is a local action (including rejected attempts), never a prompt. */
    fun performLocalCommand(input: String = text, range: IntRange? = null): Boolean {
        val actions = localActions ?: return false
        val local = uniffi.zeron_core.mobileComposerAction(actions.harness, input) ?: return false
        val reason = when {
            localPending -> "An action is already in progress."
            range == null && local.hasArguments -> "This action takes no arguments. Select it from the / menu to keep the rest of your draft."
            else -> uniffi.zeron_core.mobileComposerActionError(local.action, actions.context())
        }
        if (reason != null) { localFailure = text to reason; return true }
        val identity = commandContext
        val previous = value
        val remaining = if (range == null) "" else text.removeRange(range)
        value = TextFieldValue(remaining, TextRange((range?.first ?: 0).coerceAtMost(remaining.length)))
        localPending = true
        localFailure = null
        actions.scope.launch(start = CoroutineStart.UNDISPATCHED) {
            try {
                if (identity != commandContext) return@launch
                actions.execute(local.action)
            } catch (e: Exception) {
                if (identity == commandContext) {
                    if (text == remaining) value = previous
                    if (e !is CancellationException) localFailure = text to (e.message ?: "Could not complete this action")
                }
                if (e is CancellationException) throw e
            } finally { localPending = false }
        }
        return true
    }

    /** Shared desktop Markdown boundaries, with UTF-16 offsets for TextFieldValue. */
    fun completionQuery(): uniffi.zeron_core.ComposerCompletion? {
        if (!value.selection.collapsed || value.composition != null) return null
        val query = uniffi.zeron_core.composerCompletionQuery(
            text, value.selection.start.toUInt(), value.selection.end.toUInt(),
        ) ?: return null
        // A reference that was already selected must not reopen the menu.
        val selected = commands[text.substring(query.start.toInt(), query.end.toInt())]
        return query.takeUnless { selected != null && selected.unavailableReason == null }
    }

    fun completionRows(
        catalog: uniffi.zeron_core.ComposerCatalog,
        query: uniffi.zeron_core.ComposerCompletion,
    ): List<uniffi.zeron_core.ComposerCommand> =
        catalog.commands.filter { it.token.startsWith(if (query.skills) "$" else "/") }
            .map(::displayCommand)
            .filter {
                query.query.isEmpty() || it.token.drop(1).contains(query.query, ignoreCase = true) ||
                    it.description.contains(query.query, ignoreCase = true)
            }
            .sortedWith(compareBy({ it.unavailableReason != null }, { it.token }))

    fun insertCommand(
        command: uniffi.zeron_core.ComposerCommand,
        expectedContext: String = commandContext,
        expectedQuery: uniffi.zeron_core.ComposerCompletion? = completionQuery(),
    ) {
        if (expectedContext != commandContext) return
        val query = completionQuery() ?: return
        if (query != expectedQuery || !command.token.startsWith(if (query.skills) "$" else "/")) return
        val range = query.start.toInt() until query.end.toInt()
        if (performLocalCommand(command.insertion, range)) return
        if (command.unavailableReason != null) return
        val selected = uniffi.zeron_core.prepareComposerSelection(
            text.removeRange(range), commands.values.toList(), command,
        )
        val end = range.last + 1
        val following = text.getOrNull(end)
        val advance = if (following?.isWhitespace() == true && following != '\n' && following != '\r') 1 else 0
        val closingPunctuation = setOf(')', ']', '}', '*', '_', '~', ',', '.', ';', ':', '!', '?')
        val keepSuffix = following != null && (following.isWhitespace() || following in closingPunctuation)
        val replacement = selected.token + if (keepSuffix) "" else " "
        val next = text.replaceRange(range, replacement)
        commands[selected.token] = selected
        value = TextFieldValue(next, TextRange(range.first + replacement.length + advance))
    }

    val text: String get() = value.text
    val hasContent: Boolean get() = value.text.isNotBlank() || images.isNotEmpty()

    /** A terminal's output preserves the skill identities and any staged attachments. */
    fun appendText(text: String) {
        val current = encodedDraft()
        setText(if (current.isEmpty()) text else current + (if (current.endsWith("\n")) "" else "\n") + text)
    }

    fun setText(s: String) {
        commands.clear()
        val text = if (s.contains("zeron-invoke:") || s.startsWith("zeron-composer-draft-v1:")) {
            val restored = uniffi.zeron_core.restoreComposerDraft(s)
            restored.selections.forEach { commands[it.token] = it }
            restored.text
        } else s
        value = TextFieldValue(text, TextRange(text.length))
    }

    fun encodedDraft(): String = if (commands.isEmpty()) value.text else
        uniffi.zeron_core.encodeComposerSelections(value.text, commands.values.toList())

    val selectionError: String?
        get() = if (commands.values.none { it.unavailableReason != null }) null else
            uniffi.zeron_core.composerSelectionError(value.text, commands.values.toList())

    fun persistedDraft(): String = if (commands.isEmpty()) value.text else
        uniffi.zeron_core.saveComposerDraft(value.text, commands.values.toList())

    fun clear() {
        setText("")
        images.clear()
        tokens.clear()
        commands.clear()
        localFailure = null
    }

    /** The `@query` being typed at the cursor: its start offset and query. */
    fun activeQuery(): Pair<Int, String>? {
        val t = value.text
        if (!value.selection.collapsed) return null
        val cursor = value.selection.start
        var i = cursor - 1
        while (i >= 0) {
            val c = t[i]
            if (c == '@') {
                val before = if (i > 0) t[i - 1] else ' '
                return if (before == ' ' || before == '\n') i to t.substring(i + 1, cursor) else null
            }
            if (c == ' ' || c == '\n') return null
            i--
        }
        return null
    }

    /** Replace the `@query` with a token for `file` (parent folder on basename clashes). */
    fun insertMention(file: FileMatch) {
        val (start, _) = activeQuery() ?: return
        val parts = file.path.trim('/').split('/')
        var token = "@" + (parts.lastOrNull() ?: file.path)
        val existing = tokens[token]
        if (existing != null && existing.path != file.path && parts.size > 1) token = "@" + parts.takeLast(2).joinToString("/")
        tokens[token] = file
        val t = value.text
        val cursor = value.selection.start
        val next = t.substring(0, start) + token + " " + t.substring(cursor)
        value = TextFieldValue(next, TextRange(start + token.length + 1))
    }

    /** The text to send: live tokens become canonical `zeron-file:` links. */
    fun encoded(): String {
        var out = encodedDraft()
        for ((token, file) in tokens.entries.sortedByDescending { it.key.length }) {
            if (out.contains(token)) out = out.replace(token, fileMentionLink(file.path, file.isDir))
        }
        return out.trim()
    }
}

object Staging {
    const val MAX_IMAGES = 8
    private const val MAX_SIDE = 2560
    private const val MAX_BYTES = 24 * 1024 * 1024

    /** Decode, downscale and JPEG-encode a picked image (off the main thread). */
    suspend fun stage(context: Context, uri: Uri): StagedImage? = withContext(Dispatchers.IO) {
        runCatching {
            val source = ImageDecoder.createSource(context.contentResolver, uri)
            val bitmap = ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
                val w = info.size.width
                val h = info.size.height
                val scale = min(1f, MAX_SIDE.toFloat() / max(w, h))
                if (scale < 1f) decoder.setTargetSize((w * scale).roundToInt(), (h * scale).roundToInt())
                decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
            }
            stage(bitmap)
        }.getOrNull()
    }

    fun stage(bitmap: Bitmap): StagedImage? {
        val out = ByteArrayOutputStream()
        bitmap.compress(Bitmap.CompressFormat.JPEG, 86, out)
        val bytes = out.toByteArray()
        if (bytes.size > MAX_BYTES) return null
        val t = 180f / max(bitmap.width, bitmap.height)
        val thumb = if (t < 1f) Bitmap.createScaledBitmap(bitmap, (bitmap.width * t).roundToInt(), (bitmap.height * t).roundToInt(), true) else bitmap
        val id = UUID.randomUUID().toString()
        return StagedImage(id, "image-${id.take(8)}.jpg", bytes, thumb.asImageBitmap())
    }
}
