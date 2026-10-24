package sh.zeron.android.core

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow

enum class OnboardingStep { WELCOME, APPEARANCE, SYNC, AGENTS, DONE }

/** Device appearance is shared; completion and resumable signed-in steps belong to one account/org. */
class OnboardingState(private val read: (String) -> String?, private val write: (String, String?) -> Unit) {
    private val current = MutableStateFlow<OnboardingStep?>(OnboardingStep.WELCOME)
    val step = current.asStateFlow()
    private var identity: String? = null
    private var preview = false

    fun attach(identity: String?, demo: Boolean, forced: OnboardingStep? = null) {
        this.identity = identity
        preview = demo || forced != null
        if (forced != null) {
            move(if (identity == null && forced > OnboardingStep.SYNC) OnboardingStep.SYNC else forced)
        } else if (demo) {
            move(null)
        } else if (identity != null) {
            val saved = savedStep().takeIf { read(OWNER) == identity && it >= OnboardingStep.AGENTS }
            move(if (read(completed(identity)) == "true") null else saved ?: OnboardingStep.AGENTS)
        } else {
            move(savedStep().takeIf { read(OWNER).isNullOrEmpty() && it <= OnboardingStep.SYNC } ?: OnboardingStep.WELCOME)
        }
    }

    fun begin() = move(if (!preview && read(APPEARANCE_SEEN) == "true") OnboardingStep.SYNC else OnboardingStep.APPEARANCE)

    fun next() {
        when (current.value) {
            OnboardingStep.APPEARANCE -> {
                if (!preview) write(APPEARANCE_SEEN, "true")
                move(OnboardingStep.SYNC)
            }
            OnboardingStep.SYNC -> if (identity != null) move(OnboardingStep.AGENTS)
            OnboardingStep.AGENTS -> move(OnboardingStep.DONE)
            else -> Unit
        }
    }

    fun back() = move(when (current.value) {
        OnboardingStep.APPEARANCE -> OnboardingStep.WELCOME
        OnboardingStep.SYNC -> OnboardingStep.APPEARANCE
        OnboardingStep.AGENTS -> OnboardingStep.SYNC
        OnboardingStep.DONE -> OnboardingStep.AGENTS
        else -> current.value
    })

    fun finish() {
        if (identity == null || current.value != OnboardingStep.DONE) return
        if (!preview) write(completed(identity!!), "true")
        move(null)
    }

    fun signedOut() {
        identity = null
        preview = false
        move(OnboardingStep.WELCOME)
    }

    private fun savedStep() = read(PENDING)?.let { runCatching { OnboardingStep.valueOf(it) }.getOrNull() } ?: OnboardingStep.WELCOME

    private fun move(step: OnboardingStep?) {
        current.value = step
        if (!preview) {
            write(PENDING, step?.name)
            write(OWNER, if (step == null) null else identity.orEmpty())
        }
    }

    private fun completed(identity: String) = "onboarding.complete:$identity"

    private companion object {
        const val APPEARANCE_SEEN = "onboarding.appearanceSeen"
        const val PENDING = "onboarding.step"
        const val OWNER = "onboarding.owner"
    }
}
