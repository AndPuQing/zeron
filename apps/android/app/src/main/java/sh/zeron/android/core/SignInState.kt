package sh.zeron.android.core

import java.util.UUID

/** One pending browser sign-in, backed by app storage to survive process death. */
internal class SignInState(
    private val read: () -> String?,
    private val write: (String?) -> Unit,
) {
    fun begin(): String = UUID.randomUUID().toString().also(write)

    fun accept(returnedState: String?): Boolean {
        val expected = read() ?: return false
        if (expected != returnedState) return false
        write(null)
        return true
    }

    fun clear() = write(null)
}
