package sh.zeron.android.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.GlassScene
import sh.zeron.android.design.ZIcons
import sh.zeron.android.design.ZeronTheme

@Composable
fun ZeronRoot(model: AppModel) {
    val appearance by model.appearance.collectAsState()
    ZeronTheme(appearance) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            val client by model.client.collectAsState()
            AnimatedContent(client != null, transitionSpec = { fadeIn() togetherWith fadeOut() }, label = "root") { signedIn ->
                if (signedIn) MainNav(model) else SignInScreen(model)
            }
        }
        UpdateDialog(model.updates)
    }
}

object Routes {
    const val HOME = "home"
    const val CHAT = "chat/{id}"
    const val NEW = "new"
    const val SEARCH = "search"
    fun chat(id: String) = "chat/$id"
}

@Composable
private fun MainNav(model: AppModel) {
    val nav = rememberNavController()
    LaunchedEffect(Unit) {
        when (val route = model.launch.route) {
            null -> Unit
            "new" -> nav.navigate(Routes.NEW)
            "search" -> nav.navigate(Routes.SEARCH)
            else -> if (route.startsWith("chat:")) nav.navigate(Routes.chat(route.removePrefix("chat:")))
        }
    }
    NavHost(nav, startDestination = Routes.HOME) {
        composable(Routes.HOME) { Home(model, nav) }
        composable(Routes.CHAT) { entry ->
            val id = entry.arguments?.getString("id") ?: return@composable
            SessionScreen(model, id, onBack = { nav.popBackStack() }, onOpen = { target ->
                val currentId = nav.currentBackStackEntry?.arguments?.getString("id")
                val route = Routes.chat(target)
                // Reuse the source page when returning from a fork or its seam.
                if (target != currentId && !nav.popBackStack(route, inclusive = false)) {
                    val parent = currentId?.let { model.row(it)?.parentChatId }
                    nav.navigate(route) {
                        if (currentId != null && parent != null && model.row(target)?.parentChatId == parent) {
                            popUpTo(Routes.chat(currentId)) { inclusive = true }
                        }
                    }
                }
            })
        }
        composable(Routes.NEW) {
            NewSessionScreen(model, onClose = { nav.popBackStack() }, onCreated = { id ->
                nav.popBackStack()
                nav.navigate(Routes.chat(id))
            })
        }
        composable(Routes.SEARCH) {
            SearchScreen(model, onBack = { nav.popBackStack() }, onOpen = { nav.navigate(Routes.chat(it)) })
        }
    }
}

private enum class Tab { Sessions, Settings }

@Composable
private fun Home(model: AppModel, nav: NavHostController) {
    var tab by rememberSaveable { mutableStateOf(if (model.launch.route == "settings") Tab.Settings else Tab.Sessions) }
    val workspace by model.workspace.collectAsState()
    val summary = remember(workspace) { workspace?.let { liveSummary(it) } }
    GlassScene(
        modifier = Modifier.fillMaxSize(),
        background = { source ->
            Box(source.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
                when (tab) {
                    Tab.Sessions -> SessionsScreen(model, onOpen = { nav.navigate(Routes.chat(it)) })
                    Tab.Settings -> SettingsScreen(model)
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
            NewSessionBar(summary, onClick = { nav.navigate(Routes.NEW) })
            FloatingNavBar(
                listOf(
                    NavItem("Sessions", ZIcons.TabSessions, tab == Tab.Sessions) { tab = Tab.Sessions },
                    NavItem("Settings", ZIcons.TabSettings, tab == Tab.Settings) { tab = Tab.Settings },
                ),
                trailing = {
                    TonalCircleButton(ZIcons.Search, "Search", onClick = { nav.navigate(Routes.SEARCH) }, size = 64.dp)
                },
            )
        }
    }
}
