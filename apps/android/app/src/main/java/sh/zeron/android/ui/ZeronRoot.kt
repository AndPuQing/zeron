package sh.zeron.android.ui

import uniffi.zeron_core.MobileComposerAction

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.ui.input.key.*
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import androidx.navigation.NavType
import androidx.navigation.navArgument
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import sh.zeron.android.core.AppModel
import sh.zeron.android.core.OnboardingStep
import sh.zeron.android.design.GlassScene
import sh.zeron.android.design.ZIcons
import sh.zeron.android.design.ZeronTheme

@Composable
fun ZeronRoot(model: AppModel) {
    val appearance by model.appearance.collectAsState()
    ZeronTheme(appearance) {
        Surface(Modifier.fillMaxSize().semantics { testTagsAsResourceId = true }, color = MaterialTheme.colorScheme.background) {
            val client by model.client.collectAsState()
            val onboarding by model.onboarding.step.collectAsState()
            var startNewSession by rememberSaveable { mutableStateOf(false) }
            LaunchedEffect(client) { if (client == null) startNewSession = false }
            val destination = when {
                onboarding == OnboardingStep.WELCOME -> "welcome"
                onboarding != null -> "onboarding"
                client != null -> "main"
                else -> "welcome"
            }
            AnimatedContent(destination, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "root") { page ->
                when (page) {
                    "main" -> MainNav(model, startNewSession)
                    "onboarding" -> onboarding?.let { step ->
                        OnboardingScreen(model, step) { start ->
                            if (model.onboarding.step.value == OnboardingStep.DONE) {
                                startNewSession = start
                                model.onboarding.finish()
                            }
                        }
                    }
                    else -> SignInScreen(model)
                }
            }
        }
        UpdateDialog(model.updates)
        AgentSignInBrowser(model)
    }
}

object Routes {
    const val HOME = "home"
    const val CHAT = "chat/{id}"
    const val TERMINAL = "terminal/{chat}?id={id}"
    const val SUBAGENT = "subagent/{parent}/{doc}?title={title}&running={running}"
    const val VOICE = "voice-stage"
    const val VOICE_SETTINGS = "voice-settings"
    const val NEW = "new"
    const val SEARCH = "search"
    const val IMPORT = "import"
    const val PROVIDERS = "providers?device={device}&harness={harness}&login={login}"
    fun terminal(chat: String, id: String?) = "terminal/${android.net.Uri.encode(chat)}" + (id?.let { "?id=${android.net.Uri.encode(it)}" } ?: "")
    fun subagent(parent: String, doc: String, title: String, running: Boolean) =
        "subagent/${android.net.Uri.encode(parent)}/${android.net.Uri.encode(doc)}?title=${android.net.Uri.encode(title)}&running=$running"
    fun chat(id: String) = "chat/$id"
    fun providers(device: String? = null, harness: String? = null, signIn: Boolean = false) =
        "providers?device=${android.net.Uri.encode(device.orEmpty())}&harness=${android.net.Uri.encode(harness.orEmpty())}&login=$signIn"
}

@Composable
private fun MainNav(model: AppModel, startNewSession: Boolean) {
    val nav = rememberNavController()
    val client by model.client.collectAsState()
    val draftStore: ComposerDrafts = viewModel()
    val drafts = draftStore.forClient(client)
    BoxWithConstraints(Modifier.fillMaxSize()) {
    val wide = maxWidth >= 700.dp && maxHeight >= 480.dp
    val roomy = maxWidth >= 1100.dp
    val sidebarWidth = if (maxWidth < 1000.dp) 320.dp else 380.dp
    val shell = remember { TabletShellState() }
    val open: (String) -> Unit = { id -> if (wide) shell.select(id) else nav.navigate(Routes.chat(id)) }
    val newSession: () -> Unit = { if (wide) shell.newSession() else nav.navigate(Routes.NEW) { launchSingleTop = true } }
    val search: () -> Unit = { if (wide) shell.searching = true else nav.navigate(Routes.SEARCH) { launchSingleTop = true } }
    LaunchedEffect(wide) {
        if (!wide) {
            shell.selected?.let { id -> shell.select(null); nav.navigate(Routes.chat(id)) }
        } else if (nav.currentDestination?.route == Routes.CHAT) {
            val id = nav.currentBackStackEntry?.arguments?.getString("id")
            nav.popBackStack(Routes.HOME, false)
            shell.select(id)
        }
    }
    var companionPending by rememberSaveable { mutableStateOf(model.launch.companion) }
    @Composable
    fun Session(id: String, back: () -> Unit, beside: Boolean = false) {
        SessionScreen(model, id, onBack = back, draft = drafts.getOrPut(id) { ComposerModel(model.drafts.load(id)) },
            onNavigate = { action -> when (action) {
                MobileComposerAction.NEW -> newSession()
                MobileComposerAction.SEARCH -> search()
                MobileComposerAction.APPEARANCE -> nav.navigate("appearance")
                else -> Unit
            } },
            onAgentAccounts = { device, harness -> nav.navigate(Routes.providers(device, harness, true)) { launchSingleTop = true } },
            onOpen = { target ->
                if (beside) shell.select(target) else if (target != id && !nav.popBackStack(Routes.chat(target), false)) {
                    val parent = model.row(id)?.parentChatId
                    nav.navigate(Routes.chat(target)) {
                        if (parent != null && model.row(target)?.parentChatId == parent) popUpTo(Routes.chat(id)) { inclusive = true }
                    }
                }
            },
            onOpenSideChat = { side -> if (beside) shell.push(id, InspectorPage.SideChat(side)) else nav.navigate(Routes.chat(side)) },
            onOpenTerminal = { chat, terminal -> if (beside) shell.push(id, InspectorPage.Terminal(terminal, chat)) else nav.navigate(Routes.terminal(chat, terminal)) },
            onOpenSubagent = { parent, doc, title, running ->
                if (beside) shell.push(id, InspectorPage.Subagent(parent, doc, title, running)) else nav.navigate(Routes.subagent(parent, doc, title, running))
            },
            onOpenVoice = { model.voice.openStage() }, onVoiceSettings = { model.voice.openSettings() },
            openCompanion = companionPending?.takeIf { model.launch.route == "chat:$id" },
            onShowCompanions = if (beside) ({ section -> shell.toggleCompanions(id, section) }) else null,
        )
        LaunchedEffect(id) { if (model.launch.route == "chat:$id") companionPending = null }
    }
    var launchHandled by rememberSaveable { mutableStateOf(false) }
    val navigateAction: (MobileComposerAction) -> Unit = { action ->
        when (action) {
            MobileComposerAction.NEW -> newSession()
            MobileComposerAction.SEARCH -> search()
            MobileComposerAction.APPEARANCE -> nav.navigate("appearance") { launchSingleTop = true }
            else -> Unit
        }
    }
    LaunchedEffect(Unit) {
        if (launchHandled) return@LaunchedEffect
        launchHandled = true
        if (model.launch.voicePreview && !model.voice.state.value.live) model.voice.preview()
        when (val route = if (startNewSession) "new" else model.launch.route) {
            null -> Unit
            "new" -> newSession()
            "search" -> search()
            "import" -> nav.navigate(Routes.IMPORT)
            else -> if (route.startsWith("chat:")) open(route.removePrefix("chat:"))
        }
    }
    Box(Modifier.fillMaxSize().onPreviewKeyEvent { e ->
        if (e.type != KeyEventType.KeyDown || !(e.isCtrlPressed || e.isMetaPressed)) return@onPreviewKeyEvent false
        val chat = if (wide) shell.selected else nav.currentBackStackEntry?.takeIf { it.destination.route == Routes.CHAT }?.arguments?.getString("id")
        when {
            e.key == Key.N -> true.also { newSession() }
            e.key == Key.F && e.isShiftPressed -> true.also { search() }
            e.key == Key.B && wide -> true.also { shell.sidebar = !shell.sidebar }
            e.key == Key.J && chat != null -> true.also {
                val live = model.terminals.tabs(chat).firstOrNull { it.isLive }?.ref?.id
                if (wide) shell.push(chat, InspectorPage.Terminal(live)) else nav.navigate(Routes.terminal(chat, live))
            }
            e.key == Key.Zero && e.isAltPressed && chat != null && wide -> true.also { shell.toggleCompanions(chat, null) }
            else -> false
        }
    }) {
    NavHost(nav, startDestination = Routes.HOME) {
        composable(Routes.HOME) {
            if (!wide) Home(model, nav, onOpen = open, onNew = newSession, onSearch = search)
            else TabletShell(model, shell, roomy, sidebarWidth,
                sidebar = {
                    if (shell.searching) SearchScreen(model, onBack = { shell.searching = false }, onOpen = { shell.searching = false; open(it) })
                    else Home(model, nav, onOpen = open, onNew = newSession, onSearch = search)
                },
                session = { id -> Session(id, { shell.select(null) }, beside = true) },
                draft = { NewSessionScreen(model, onClose = null, onCreated = { shell.select(it) }, onNavigate = navigateAction,
                    composer = drafts.getOrPut("new") { ComposerModel() }, focusOnAppear = shell.draftToken > 0) },
                page = { chat, page -> InspectorPageContent(model, shell, chat, page,
                    drafts = drafts, onOpen = open,
                    onAgentAccounts = { device, harness -> nav.navigate(Routes.providers(device, harness, true)) }) },
            )
        }
        composable(Routes.CHAT) { entry ->
            val id = entry.arguments?.getString("id") ?: return@composable
            Session(id, { nav.popBackStack() })
        }
        composable(Routes.TERMINAL, arguments = listOf(
            navArgument("chat") { type = NavType.StringType },
            navArgument("id") { type = NavType.StringType; nullable = true; defaultValue = null },
        )) { entry ->
            TerminalScreen(model, entry.arguments?.getString("chat") ?: return@composable,
                terminalId = entry.arguments?.getString("id"), onBack = { nav.popBackStack() },
                script = if (model.launch.terminalDemo) listOf("git log --oneline -5", "cargo test -p zeron-veil") else emptyList())
        }
        composable(Routes.SUBAGENT, arguments = listOf(
            navArgument("parent") { type = NavType.StringType }, navArgument("doc") { type = NavType.StringType },
            navArgument("title") { defaultValue = "Subagent" }, navArgument("running") { type = NavType.BoolType; defaultValue = false },
        )) { entry ->
            val args = entry.arguments ?: return@composable
            SubagentScreen(model, args.getString("parent") ?: return@composable, args.getString("doc") ?: return@composable,
                title = args.getString("title") ?: "Subagent", running = args.getBoolean("running"), onBack = { nav.popBackStack() },
                onOpenSubagent = { parent, doc, title, running -> nav.navigate(Routes.subagent(parent, doc, title, running)) })
        }
        composable(Routes.NEW) {
            NewSessionScreen(model, onNavigate = navigateAction, composer = drafts.getOrPut("new") { ComposerModel() }, onClose = { nav.popBackStack() }, onCreated = { id ->
                nav.popBackStack()
                open(id)
            })
        }
        composable("appearance") { SettingsScreen(model, onProviders = { nav.navigate(Routes.providers()) }) }
        composable(Routes.PROVIDERS, arguments = listOf(
            navArgument("device") { defaultValue = "" },
            navArgument("harness") { defaultValue = "" },
            navArgument("login") { type = NavType.BoolType; defaultValue = false },
        )) { entry ->
            ProvidersScreen(model,
                initialDevice = entry.arguments?.getString("device")?.takeIf { it.isNotEmpty() },
                initialHarness = entry.arguments?.getString("harness")?.takeIf { it.isNotEmpty() },
                signIn = entry.arguments?.getBoolean("login") == true,
                onBack = { nav.popBackStack() },
            )
        }
        composable(Routes.IMPORT) { SessionImportScreen(model, onClose = { nav.popBackStack() }) }
        composable(Routes.SEARCH) {
            SearchScreen(model, onBack = { nav.popBackStack() }, onOpen = open)
        }
    }
    val toastHost = remember { androidx.compose.material3.SnackbarHostState() }
    LaunchedEffect(model) {
        model.toasts.collect { toast ->
            toastHost.currentSnackbarData?.dismiss()
            if (toastHost.showSnackbar(toast.message, actionLabel = toast.action) == androidx.compose.material3.SnackbarResult.ActionPerformed) toast.run?.invoke()
        }
    }
    androidx.compose.material3.SnackbarHost(toastHost, Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 100.dp))
    VoiceOverlays(model, onTranscript = open)
    }
    }
}

private enum class Tab { Sessions, Settings }

@Composable
private fun Home(model: AppModel, nav: NavHostController,
    onOpen: (String) -> Unit, onNew: () -> Unit, onSearch: () -> Unit) {
    var tab by rememberSaveable { mutableStateOf(if (model.launch.route == "settings") Tab.Settings else Tab.Sessions) }
    val workspace by model.workspace.collectAsState()
    val voice by model.voice.state.collectAsState()
    val summary = remember(workspace) { workspace?.let { liveSummary(it) } }
    GlassScene(
        modifier = Modifier.fillMaxSize(),
        background = { source ->
            Box(source.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
                when (tab) {
                    Tab.Sessions -> SessionsScreen(model, onOpen = onOpen, onImport = { nav.navigate(Routes.IMPORT) })
                    Tab.Settings -> SettingsScreen(model, onProviders = { nav.navigate(Routes.providers()) })
                }
            }
        },
    ) {
        // Sibling glass samples the page, including rows scrolling underneath.
        Column(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .navigationBarsPadding()
                .padding(top = 12.dp, bottom = 8.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            if (voice.live) VoiceCallStrip(model, Modifier.padding(horizontal = 16.dp))
            else NewSessionBar(summary, onClick = onNew, trailing = { VoiceEntry(model) })
            FloatingNavBar(
                listOf(
                    NavItem("Sessions", ZIcons.TabSessions, tab == Tab.Sessions) { tab = Tab.Sessions },
                    NavItem("Settings", ZIcons.TabSettings, tab == Tab.Settings) { tab = Tab.Settings },
                ),
                trailing = {
                    TonalCircleButton(ZIcons.Search, "Search", onClick = onSearch, size = 64.dp, iconSize = 28.dp)
                },
            )
        }
    }
}

@Composable
private fun AgentSignInBrowser(model: AppModel) {
    val state by model.agentSignIn.state.collectAsState()
    val context = androidx.compose.ui.platform.LocalContext.current
    LaunchedEffect(state?.browserRequest) {
        val current = state ?: return@LaunchedEffect
        val url = current.url ?: return@LaunchedEffect
        if (!current.busy || !current.browserPending) return@LaunchedEffect
        try {
            openAgentBrowser(context, url)
            model.agentSignIn.browserOpened(current.browserRequest)
        } catch (_: Exception) {
            model.agentSignIn.browserFailed()
        }
    }
}
