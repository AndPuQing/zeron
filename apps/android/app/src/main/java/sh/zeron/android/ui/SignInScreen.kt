package sh.zeron.android.ui

import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.WallpaperHero
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons

@Composable
fun SignInScreen(model: AppModel) {
    val client by model.client.collectAsState()
    Box(Modifier.fillMaxSize()) {
        WallpaperHero(model.wallpaper, Modifier.matchParentSize())
        BoxWithConstraints(Modifier.fillMaxSize().safeDrawingPadding()) {
            Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).heightIn(min = maxHeight).padding(horizontal = 24.dp),
                horizontalAlignment = Alignment.CenterHorizontally) {
                Box(Modifier.weight(1f).heightIn(min = 300.dp), contentAlignment = Alignment.Center) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        ZIcon(ZIcons.Logo, null, Modifier.size(46.dp, 53.dp))
                        Spacer(Modifier.height(8.dp))
                        Text("Welcome to Zerun", style = MaterialTheme.typography.displaySmall, textAlign = TextAlign.Center)
                        Text("Your coding agents, from anywhere.", style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
                    }
                }
                AppButton(onClick = { model.onboarding.begin() }, modifier = Modifier.fillMaxWidth().testTag("welcome-get-started"),
                    container = MaterialTheme.colorScheme.onSurface, foreground = MaterialTheme.colorScheme.background) { Text("Get started") }
                if (client == null) {
                    AppTextButton(onClick = { model.startDemo() }, modifier = Modifier.fillMaxWidth()) { Text("Explore the demo") }
                }
                AppTextButton(onClick = { model.updates.check() }, modifier = Modifier.fillMaxWidth(), color = MaterialTheme.colorScheme.onSurfaceVariant) {
                    Text("Check for updates", style = MaterialTheme.typography.bodySmall)
                }
                Spacer(Modifier.height(16.dp))
            }
        }
    }
}

/** The existing WorkOS flow, now reached from onboarding's sync step. */
@Composable
internal fun AccountSignIn(model: AppModel) {
    val context = LocalContext.current
    val error by model.signInError.collectAsState()
    val orgs by model.orgChoice.collectAsState()
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium) }
        AppButton(onClick = {
            runCatching {
                val url = model.beginSignIn()
                CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(context, Uri.parse(url))
            }.onFailure { model.signInError.value = "Couldn't open sign-in. Check your browser and try again." }
        }, modifier = Modifier.fillMaxWidth().testTag("onboarding-log-in")) { Text("Log in") }
    }
    orgs?.let { (list, choice) ->
        AppSheet("Organization", onDismiss = { choice.complete(null) }) {
            Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 20.dp).padding(bottom = 24.dp)) {
                list.forEachIndexed { i, org ->
                    GroupedRow(i, list.size, onClick = { choice.complete(org) }) { Text(org.name) }
                }
            }
        }
    }
}
