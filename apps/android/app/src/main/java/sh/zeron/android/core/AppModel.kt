package sh.zeron.android.core

import android.app.Application
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.SystemClock
import uniffi.zeron_core.SendState
import android.os.Looper
import android.provider.Settings
import android.util.Log
import kotlinx.coroutines.CompletableDeferred
import sh.zeron.android.design.Appearance
import sh.zeron.android.design.ThemeMode
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.zeron_core.AuthCallback
import uniffi.zeron_core.AuthOrg
import uniffi.zeron_core.ChatConfig
import uniffi.zeron_core.ClientEvent
import uniffi.zeron_core.ClientListener
import uniffi.zeron_core.Connectivity
import uniffi.zeron_core.CoreClient
import uniffi.zeron_core.CoreConfig
import uniffi.zeron_core.Credentials
import uniffi.zeron_core.DemoFixture
import uniffi.zeron_core.DemoOptions
import uniffi.zeron_core.NewSession
import uniffi.zeron_core.SandboxLevel
import uniffi.zeron_core.SendRequest
import uniffi.zeron_core.BusyPolicy
import uniffi.zeron_core.SessionRow
import uniffi.zeron_core.SessionTarget
import uniffi.zeron_core.StreamSpeed
import uniffi.zeron_core.TranscriptScale
import uniffi.zeron_core.WorkspaceSnapshot
import uniffi.zeron_core.WorktreeSpec
import uniffi.zeron_core.authExchangeCode
import uniffi.zeron_core.authListOrgs
import uniffi.zeron_core.authProductionEdgeUrl
import uniffi.zeron_core.authRefresh
import uniffi.zeron_core.parseAuthCallback
import uniffi.zeron_core.workosAuthorizeUrl
import java.io.File
import java.util.UUID

/** How the app was launched (adb extras mirror the iOS launch arguments). */
data class LaunchOptions(
    val demo: Boolean = false,
    val fast: Boolean = false,
    val longReply: Boolean = false,
    val big: Boolean = false,
    val huge: Boolean = false,
    val noProjects: Boolean = false,
    val signedOut: Boolean = false,
    val route: String? = null,
    /** `--es companion companions|terminal|side-chat:<id>|subagent:<tool id>`: open it over the routed chat (screenshots). */
    val companion: String? = null,
    /** `--ez terminal-demo true`: the opened terminal types a short script. */
    val terminalDemo: Boolean = false,
    /** `--ez cloud true`: offer Cloud in the demo (its pages read the demo's Cloud). */
    val cloud: Boolean = false,
    val wallpaper: String? = null,
    val wallpaperEffect: String? = null,
    val voicePreview: Boolean = false,
    val onboarding: String? = null,
)

/**
 * App-wide state owner: holds the Rust [CoreClient], republishes its
 * snapshots as flows, and fans session events out to screens. Core events
 * arrive on Rust threads and hop to the main thread here.
 */
class AppModel(private val app: Application) {
    val updates = sh.zeron.android.update.AndroidUpdater(app)
    private val scope = MainScope()
    private val main = Handler(Looper.getMainLooper())
    val credentials = CredentialStore(app)
    val wallpaper = WallpaperStore(app)

    private val _client = MutableStateFlow<CoreClient?>(null)
    val client: StateFlow<CoreClient?> = _client.asStateFlow()
    val agentSignIn = DeviceSignIn(scope, client = { _client.value?.let(::CoreAgentLoginClient) })
    val voice = sh.zeron.android.voice.VoiceController(app) { _client.value }

    private val _workspace = MutableStateFlow<WorkspaceSnapshot?>(null)
    val workspace: StateFlow<WorkspaceSnapshot?> = _workspace.asStateFlow()

    private val _connectivity = MutableStateFlow<Connectivity?>(null)
    val connectivity: StateFlow<Connectivity?> = _connectivity.asStateFlow()

    /** Chat ids whose session or composer changed. */
    private val _sessionEvents = MutableSharedFlow<String>(extraBufferCapacity = 256)
    val sessionEvents: SharedFlow<String> = _sessionEvents

    private val _sideChatBusy = MutableStateFlow(false)
    val sideChatBusy: StateFlow<Boolean> = _sideChatBusy.asStateFlow()

    /** Organizations to pick from mid sign-in (more than one). */
    val orgChoice = MutableStateFlow<Pair<List<AuthOrg>, CompletableDeferred<AuthOrg?>>?>(null)
    val signInError = MutableStateFlow<String?>(null)

    val drafts = Drafts(app)
    data class Toast(val message: String, val action: String?, val run: (() -> Unit)?)
    val toasts = MutableSharedFlow<Toast>(extraBufferCapacity = 8)
    fun toast(message: String, action: String? = null, run: (() -> Unit)? = null) {
        toasts.tryEmit(Toast(message, action, run))
    }

    private val settings = app.getSharedPreferences("settings", 0)
    val onboarding = OnboardingState(
        read = { settings.getString(it, null) },
        write = { key, value -> settings.edit().putString(key, value).apply() },
    )
    private val _appearance = MutableStateFlow(
        Appearance(
            runCatching { ThemeMode.valueOf(settings.getString("theme", "System")!!) }.getOrDefault(ThemeMode.System),
        ),
    )
    val appearance: StateFlow<Appearance> = _appearance.asStateFlow()

    var providersDevice: String?
        get() = settings.getString("providersDevice", null)
        set(value) { settings.edit().putString("providersDevice", value).apply() }

    fun setAppearance(value: Appearance) {
        _appearance.value = value
        settings.edit().putString("theme", value.mode.name).remove("dynamicColor").apply()
    }

    var lastDraft: NewSessionDraft = NewSessionDraft(
        projectId = settings.getString("draft.project", null),
        hostId = settings.getString("draft.host", null),
        harness = settings.getString("draft.harness", null) ?: "claude-code",
        model = settings.getString("draft.model", null),
        effort = settings.getString("draft.effort", null),
    )
        set(value) {
            field = value
            if (!launch.demo) {
                settings.edit()
                    .putString("draft.project", value.projectId)
                    .putString("draft.host", value.hostId)
                    .putString("draft.harness", value.harness)
                    .putString("draft.model", value.model)
                    .putString("draft.effort", value.effort)
                    .putBoolean("draft.cloud", value.cloud)
                    .apply()
            }
        }

    private fun storedDraft() = NewSessionDraft(
        projectId = settings.getString("draft.project", null),
        hostId = settings.getString("draft.host", null),
        harness = settings.getString("draft.harness", null) ?: "claude-code",
        model = settings.getString("draft.model", null),
        effort = settings.getString("draft.effort", null),
        cloud = settings.getBoolean("draft.cloud", false),
    )

    var launch = LaunchOptions()
        private set

    val isDemo: Boolean get() = _client.value?.isDemo() == true
    private var refreshScheduled = false
    private val signInState = SignInState(
        read = { settings.getString("auth.pendingState", null) },
        write = { settings.edit().putString("auth.pendingState", it).apply() },
    )

    private val listener = object : ClientListener {
        override fun onEvent(event: ClientEvent) {
            main.post { handle(event) }
        }
    }

    fun boot(options: LaunchOptions) {
        launch = options
        _client.value?.let { existing ->
            forcedOnboarding()?.let { forced ->
                onboarding.attach("${existing.orgId()}/${existing.userId()}", existing.isDemo(), forced)
            }
            return
        }
        if (options.signedOut) credentials.clear()
        watchNetwork()
        // `wallpaper <path>` / `wallpaper none` and `wallpaper-effect <name>`:
        // set the wallpaper at launch (screenshots, tests).
        options.wallpaper?.let { path ->
            if (path == "none") wallpaper.remove() else scope.launch { wallpaper.set(File(path)) }
        }
        options.wallpaperEffect?.let { name ->
            WallpaperStore.effects.firstOrNull { it.name.equals(name, ignoreCase = true) }?.let(wallpaper::setEffect)
        }
        val stored = credentials.stored()
        when {
            options.demo -> start(Credentials.Demo(demoOptions()))
            stored != null -> start(stored)
            else -> onboarding.attach(null, false, forcedOnboarding())
        }
        // Relative times ("4m") and staleness age without events.
        scope.launch {
            while (true) {
                delay(30_000)
                refreshWorkspace()
            }
        }
    }

    fun demoOptions() = DemoOptions(
        fixture = if (launch.noProjects) DemoFixture.NO_PROJECTS else DemoFixture.STANDARD,
        transcriptScale = when {
            launch.huge -> TranscriptScale.Huge
            launch.big -> TranscriptScale.Big
            else -> TranscriptScale.Normal
        },
        streamSpeed = if (launch.fast) StreamSpeed.FAST else StreamSpeed.REALISTIC,
        longReply = launch.longReply,
    )

    private val coreDir get() = File(app.filesDir, "core")

    /** Local docs belong to one identity: another account starts empty. */
    private fun claimCoreDir(credentials: Credentials) {
        val owner = when (credentials) {
            is Credentials.WorkOs -> "${credentials.userId}/${credentials.orgId}"
            is Credentials.Dev -> "${credentials.userId}/${credentials.orgId}"
            is Credentials.Demo -> return
        }
        val marker = File(coreDir, ".owner")
        if (runCatching { marker.readText() }.getOrNull() != owner) coreDir.deleteRecursively()
        coreDir.mkdirs()
        marker.writeText(owner)
    }

    private fun start(credentials: Credentials): Boolean {
        voice.reset()
        val demo = credentials is Credentials.Demo
        val dir = if (demo) File(app.filesDir, "demo") else coreDir
        if (!demo) claimCoreDir(credentials)
        dir.mkdirs()
        val config = CoreConfig(
            edgeUrl = edgeUrl,
            dataDir = dir.path,
            deviceId = deviceId,
            deviceName = deviceName(),
            platform = "android",
            appVersion = app.packageManager.getPackageInfo(app.packageName, 0).versionName ?: "0",
        )
        return try {
            val client = CoreClient(config, credentials, listener)
            if (credentials is Credentials.WorkOs) this.credentials.store(credentials)
            onboarding.attach("${client.orgId()}/${client.userId()}", demo, forcedOnboarding())
            _client.value = client
            refreshWorkspace()
            client.preloadSessions()
            cloud.refresh()
            backfillProfile()
            true
        } catch (e: Exception) {
            Log.e("Zeron", "core start failed", e)
            false
        }
    }

    private fun backfillProfile() {
        val stored = credentials.stored() as? Credentials.WorkOs ?: return
        if (credentials.profile.orgName != null) return
        scope.launch {
            val orgs = runCatching { authListOrgs(edgeUrl, stored.tokens.accessToken) }.getOrNull() ?: return@launch
            val org = orgs.firstOrNull { it.organizationId == stored.orgId } ?: return@launch
            credentials.profile = credentials.profile.copy(orgName = org.name)
            refreshWorkspace()
        }
    }

    fun startDemo() {
        start(Credentials.Demo(demoOptions()))
    }

    private fun forcedOnboarding(): OnboardingStep? = launch.onboarding?.let { name ->
        OnboardingStep.entries.firstOrNull { it.name.equals(name, ignoreCase = true) }
    }

    val edgeUrl: String get() = authProductionEdgeUrl()

    private val deviceId: String
        get() {
            val prefs = app.getSharedPreferences("device", 0)
            prefs.getString("deviceId", null)?.let { return it }
            val id = "android-" + UUID.randomUUID().toString().take(8)
            prefs.edit().putString("deviceId", id).apply()
            return id
        }

    private fun deviceName(): String =
        Settings.Global.getString(app.contentResolver, Settings.Global.DEVICE_NAME) ?: Build.MODEL

    // ── sign-in ────────────────────────────────────────────────────────────

    /** The WorkOS authorize URL to open in a Custom Tab. */
    fun beginSignIn(): String {
        signInError.value = null
        return workosAuthorizeUrl(signInState.begin())
    }

    /** `zerun-dev://callback?code=…`: verify state → tokens → org → client. */
    fun handleCallback(url: String) {
        val cb = parseAuthCallback(url) ?: return
        val returnedState = when (cb) {
            is AuthCallback.Code -> cb.state
            is AuthCallback.Error -> Uri.parse(url).getQueryParameter("state")
        }
        if (!signInState.accept(returnedState)) {
            signInError.value = "Sign-in didn't complete. Try signing in again."
            return
        }
        when (cb) {
            is AuthCallback.Code -> scope.launch { signIn(cb.code) }
            is AuthCallback.Error -> signInError.value = cb.description ?: cb.error
        }
    }

    private suspend fun signIn(code: String) {
        try {
            val exchange = authExchangeCode(edgeUrl, code)
            val orgs = authListOrgs(edgeUrl, exchange.tokens.accessToken)
            if (orgs.isEmpty()) {
                signInError.value = "This account isn't in an organization yet."
                return
            }
            val org = if (orgs.size == 1) orgs[0] else {
                val choice = CompletableDeferred<AuthOrg?>()
                orgChoice.value = orgs to choice
                choice.await().also { orgChoice.value = null } ?: return
            }
            val tokens = authRefresh(edgeUrl, exchange.tokens.refreshToken, org.organizationId)
            val user = exchange.user
            val name = listOfNotNull(user.firstName, user.lastName).map { it.trim() }.filter { it.isNotEmpty() }.joinToString(" ")
            credentials.profile = CredentialStore.Profile(name.ifEmpty { null }, user.email, org.name)
            if (!start(Credentials.WorkOs(user.id, org.organizationId, tokens))) {
                signInError.value = "Couldn't open your workspace. Try signing in again."
            }
        } catch (e: Exception) {
            signInError.value = e.message ?: "Sign-in failed"
        }
    }

    fun signOut() = signOutLocally(forget = true)

    private fun signOutLocally(forget: Boolean) {
        agentSignIn.cancel()
        voice.reset()
        signInState.clear()
        val wasDemo = isDemo
        endDeliveryHold()
        lastDeliveryActivity = 0L
        terminals.signOut()
        cloud.reset()
        _client.value?.shutdown()
        _client.value = null
        _workspace.value = null
        _connectivity.value = null
        if (!wasDemo) {
            credentials.clear()
            if (forget) coreDir.deleteRecursively()
        }
        lastDraft = NewSessionDraft()
        onboarding.signedOut()
    }

    val accountName: String
        get() = when {
            isDemo -> "Demo"
            _client.value == null -> "Signed out"
            else -> credentials.profile.let { it.name ?: it.email ?: "Signed in" }
        }

    val accountDetail: String
        get() = if (isDemo) "Offline demo workspace" else credentials.profile.let { p ->
            listOfNotNull(if (p.name != null) p.email else null, p.orgName).joinToString(" · ").ifEmpty { "Zerun account" }
        }

    // ── events ─────────────────────────────────────────────────────────────

    private fun handle(event: ClientEvent) {
        when (event) {
            is ClientEvent.WorkspaceChanged -> scheduleRefresh()
            is ClientEvent.SessionChanged -> _sessionEvents.tryEmit(event.chatId)
            is ClientEvent.ComposerChanged -> _sessionEvents.tryEmit(event.chatId)
            is ClientEvent.ConnectivityChanged -> _connectivity.value = event.connectivity
            is ClientEvent.AuthRefreshed -> credentials.updateTokens(event.tokens)
            is ClientEvent.AuthExpired -> signOutLocally(forget = false)
        }
    }

    /** Coalesce bursts of registry frames into one rebuild per loop turn. */
    private fun scheduleRefresh() {
        if (refreshScheduled) return
        refreshScheduled = true
        main.post {
            refreshScheduled = false
            refreshWorkspace()
        }
    }

    fun refreshWorkspace() {
        val client = _client.value ?: return
        _workspace.value = client.workspace()
        voice.refresh()
        sweepFreshSideChats()
    }

    fun onForeground() {
        endDeliveryHold()
        updates.onForeground()
        _client.value?.onForeground()
        refreshWorkspace()
    }

    private var lastDeliveryActivity = 0L
    fun noteDeliveryActivity() { lastDeliveryActivity = SystemClock.elapsedRealtime() }
    private fun endDeliveryHold() = DeliveryService.release(app)
    fun onBackground() {
        endDeliveryHold()
        val recent = lastDeliveryActivity != 0L && SystemClock.elapsedRealtime() - lastDeliveryActivity < 120_000
        val pending = _workspace.value?.let { ws ->
            (ws.front.pinned + ws.front.sections.flatMap { it.sessions } + ws.front.recent)
                .any { it.sendState == SendState.SENDING || it.sendState == SendState.QUEUED }
        } == true
        if (_client.value != null && !isDemo && (recent || pending)) DeliveryService.hold(app, 25_000)
        _client.value?.onBackground()
    }

    /** Pull to refresh: redial and re-probe, then settle for a beat. */
    suspend fun refresh() {
        onForeground()
        delay(700)
    }

    private fun watchNetwork() {
        val cm = app.getSystemService(ConnectivityManager::class.java) ?: return
        cm.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                main.post { _client.value?.setNetworkOnline(true) }
            }

            override fun onLost(network: Network) {
                main.post {
                    val active = cm.getNetworkCapabilities(cm.activeNetwork)
                    val online = active?.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) == true
                    _client.value?.setNetworkOnline(online)
                }
            }
        })
    }

    // ── writes ─────────────────────────────────────────────────────────────

    private inline fun attempt(what: String, body: () -> Unit) {
        try {
            body()
        } catch (e: Exception) {
            Log.w("Zeron", "$what failed", e)
        }
    }

    fun setPinned(id: String, pinned: Boolean) = attempt("pin") {
        val c = _client.value ?: return
        if (pinned) c.pinSession(id) else c.unpinSession(id)
    }

    fun archive(id: String) = attempt("archive") { _client.value?.archiveSession(id) }
    fun unarchive(id: String) = attempt("unarchive") { _client.value?.unarchiveSession(id) }
    fun rename(id: String, title: String) = attempt("rename") { _client.value?.renameSession(id, title) }
    fun markSeen(id: String) = attempt("seen") { _client.value?.markSeen(id) }

    fun deleteSession(id: String) {
        checkNotNull(_client.value) { "Not signed in." }.deleteSession(id)
        refreshWorkspace()
    }
    fun setSectionCollapsed(id: String, collapsed: Boolean) = attempt("collapse") { _client.value?.setSectionCollapsed(id, collapsed) }

    fun row(id: String): SessionRow? = _client.value?.sessionRow(id)

    fun children(parentId: String): List<SessionRow> =
        _client.value?.childSessions(parentId).orEmpty().filterNot { it.archived }

    fun createSideChat(parentId: String): String {
        val client = checkNotNull(_client.value) { "Not signed in." }
        check(_sideChatBusy.compareAndSet(false, true)) { "A side chat is already being created." }
        return try {
            client.createSideChat(parentId, null).also {
                freshSideChats = freshSideChats + it
                refreshWorkspace()
            }
        } finally {
            _sideChatBusy.value = false
        }
    }

    /** A side chat forks as a sibling, under the same top-level parent. */
    suspend fun forkSideChat(sourceId: String): String {
        val client = checkNotNull(_client.value) { "Not signed in." }
        check(_sideChatBusy.compareAndSet(false, true)) { "A side chat is already being created." }
        return try {
            val parentId = client.sessionRow(sourceId)?.parentChatId ?: sourceId
            client.forkSideChat(sourceId, parentId).also { refreshWorkspace() }
        } finally {
            _sideChatBusy.value = false
        }
    }

    /** Delete for good (the desktop's "Delete session"). */
    fun delete(id: String) = attempt("delete") {
        _client.value?.deleteSession(id)
        drafts.save(id, "")
        refreshWorkspace()
    }

    /** A device's name ("MacBook Pro"), or a stand-in. */
    fun deviceName(deviceId: String): String =
        _workspace.value?.devices?.firstOrNull { it.id == deviceId }?.name ?: "its computer"

    // ── companions (side chats, subagents, terminals) ──────────────────────

    /** Cloud for this account (status, GitHub, agent logins, billing). */
    val cloud = CloudStore(scope, { _client.value }, { refreshWorkspace() })

    /** Pages opened in the browser that hand back through a `zeron://` link. */
    val browser = BrowserReturns()

    /** Cloud is offered here (the demo shows it with `--ez cloud true`). */
    val cloudOffered: Boolean get() = if (isDemo) launch.cloud else cloud.available

    /** Every chat's terminals (shells on their hosts). */
    val terminals = TerminalStore(app) { _client.value }

    /** A session's side chats, newest first. */
    fun sideChats(chatId: String): List<SessionRow> =
        _client.value?.childSessions(chatId).orEmpty().filter { !it.archived }

    /** The subagents a session's agent spawned (its open transcript). */
    fun sessionSubagents(chatId: String): List<uniffi.zeron_core.SubagentRow> {
        val client = _client.value ?: return emptyList()
        if ("--sub--" in chatId) return emptyList()
        val handle = client.session(chatId) ?: runCatching { client.openSession(chatId) }.getOrNull()
        return handle?.subagents().orEmpty()
    }

    /**
     * Side chats started here that nothing has been sent in yet: closed
     * unused, they go (the desktop only writes one at its first send). Kept
     * across launches, so one left open when the app was killed goes too.
     */
    private var freshSideChats: Set<String> = settings.getStringSet("freshSideChats", emptySet())!!.toSet()
        set(value) {
            field = value
            settings.edit().putStringSet("freshSideChats", value).apply()
        }

    fun isFreshSideChat(id: String) = id in freshSideChats
    fun forgetFreshSideChat(id: String) { freshSideChats = freshSideChats - id }

    /** Chats with a session screen up (the sweep leaves those alone). */
    val openChats = HashSet<String>()

    /** A chat that got a message is no longer a fresh, unused side chat. */
    private fun sweepFreshSideChats() {
        if (freshSideChats.isEmpty()) return
        for (id in freshSideChats) {
            val row = row(id) ?: continue
            if (row.preview != null) {
                forgetFreshSideChat(id)
            }
        }
    }

    /** Open composers by chat: text handed to a chat (terminal output) lands in its draft. */
    // The shell's ViewModel owns these across navigation and recreation.
    var retainedComposers: Map<String, sh.zeron.android.ui.ComposerModel> = emptyMap()
        internal set
    val composerSinks = HashMap<String, (String) -> Unit>()

    /** Text into a chat's composer, after what's there (or its saved draft when it isn't open). */
    fun appendToComposer(chatId: String, text: String) {
        composerSinks[chatId]?.let { return it(text) }
        val composer = retainedComposers[chatId] ?: sh.zeron.android.ui.ComposerModel(drafts.load(chatId))
        composer.appendText(text)
        drafts.save(chatId, composer.persistedDraft())
    }

    /** A Cloud chat created whose first message didn't go out yet (key → chat). */
    private var unsentCloudChat: Pair<String, String>? = null

    /**
     * A session on its own Cloud machine in `draft.projectId` (a Cloud
     * project, or any project whose repository Cloud reaches): the edge mints
     * the machine, then the first message is sent as usual — it waits in the
     * chat until the machine is up.
     */
    suspend fun createCloudSession(draft: NewSessionDraft, text: String, attachments: List<uniffi.zeron_core.OutgoingAttachment>): String {
        val client = _client.value ?: throw uniffi.zeron_core.CoreException.Closed()
        val spaceId = draft.projectId ?: throw uniffi.zeron_core.CoreException.InvalidArgument("Choose a project")
        val config = ChatConfig(draft.harness, draft.model, draft.effort, emptyMap(), SandboxLevel.WORKSPACE_WRITE)
        // A retry after a failed first send reuses that chat (and machine), while nothing it was made with changed.
        val key = "$spaceId|${draft.branch.orEmpty()}|${draft.harness}|${draft.model.orEmpty()}"
        val chatId = unsentCloudChat?.takeIf { it.first == key && client.sessionRow(it.second) != null }?.second
            ?: client.createCloudSession(spaceId, config, draft.branch, null).also { unsentCloudChat = key to it }
        client.openSession(chatId).send(SendRequest(text, attachments, null, BusyPolicy.QUEUE))
        unsentCloudChat = null
        noteDeliveryActivity()
        refreshWorkspace()
        return chatId
    }

    /** Create the chat and send its first message. */
    fun createSession(draft: NewSessionDraft, text: String, attachments: List<uniffi.zeron_core.OutgoingAttachment> = emptyList()): String? {
        val client = _client.value ?: return null
        val target = when {
            draft.projectId != null -> SessionTarget.Project(draft.projectId)
            draft.hostId != null -> SessionTarget.Projectless(draft.hostId)
            else -> return null
        }
        val config = ChatConfig(draft.harness, draft.model, draft.effort, emptyMap(), SandboxLevel.WORKSPACE_WRITE)
        return try {
            val chatId = client.createSession(NewSession(target, config, if (draft.worktree) null else draft.branch, null, null))
            val handle = client.openSession(chatId)
            val project = _workspace.value?.projects?.firstOrNull { it.id == draft.projectId }
            val worktree = if (draft.worktree && project != null) WorktreeSpec(project.path, draft.branch ?: "HEAD", project.id) else null
            handle.send(SendRequest(text, attachments, worktree, BusyPolicy.QUEUE))
            noteDeliveryActivity()
            refreshWorkspace()
            chatId
        } catch (e: Exception) {
            Log.w("Zeron", "create session failed", e)
            null
        }
    }
}

/** The new-session page's options (kept across launches). */
data class NewSessionDraft(
    val projectId: String? = null,
    val hostId: String? = null,
    val harness: String = "claude-code",
    val model: String? = null,
    val effort: String? = null,
    val branch: String? = null,
    val worktree: Boolean = false,
    /** Run on its own Cloud machine (a project whose repository Cloud reaches). */
    val cloud: Boolean = false,
)
