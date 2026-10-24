package sh.zeron.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import sh.zeron.android.core.AppModel
import sh.zeron.android.core.OnboardingStep
import sh.zeron.android.core.WallpaperStore
import sh.zeron.android.design.*

/** The self-hosted steps: Cloud/GitHub and chat-push setup are not offered on Android. */
@Composable
internal fun OnboardingScreen(model: AppModel, step: OnboardingStep, onFinish: (Boolean) -> Unit) {
    val client by model.client.collectAsState()
    BackHandler { model.onboarding.back() }
    Box(Modifier.fillMaxSize()) {
        WallpaperHero(model.wallpaper, Modifier.matchParentSize().blur(24.dp))
        Column(Modifier.align(Alignment.TopCenter).widthIn(max = 600.dp).fillMaxSize().safeDrawingPadding()) {
            val steps = listOf(OnboardingStep.APPEARANCE, OnboardingStep.SYNC, OnboardingStep.AGENTS)
            val index = steps.indexOf(step)
            if (index >= 0) {
                Row(Modifier.padding(horizontal = 24.dp, vertical = 16.dp), verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    steps.indices.forEach { i ->
                        Box(Modifier.size(20.dp, 3.dp).clip(CircleShape).background(
                            if (i <= index) MaterialTheme.colorScheme.primary else glassControlFill()))
                    }
                    Text("${index + 1} of ${steps.size}", style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(start = 8.dp))
                }
            }
            if (step == OnboardingStep.AGENTS) {
                Column(Modifier.weight(1f)) {
                    OnboardingHeading("Your agents", "Choose a computer, turn on the agents you'd like to use, and manage their accounts.",
                        Modifier.padding(horizontal = 24.dp, vertical = 12.dp))
                    ProvidersScreen(model, null, null, false, onBack = { model.onboarding.back() }, embedded = true)
                }
            } else {
                key(step) {
                    Column(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp, vertical = 12.dp),
                        verticalArrangement = Arrangement.spacedBy(24.dp)) {
                        when (step) {
                            OnboardingStep.APPEARANCE -> AppearancePage(model)
                            OnboardingStep.SYNC -> {
                                OnboardingHeading("Sync your work", "Log in to keep your chats and projects in step everywhere you use Zerun.")
                                Column {
                                    OnboardingInfo(0, 2, ZIcons.Phone, "On every device", "Pick up a chat on your phone or another computer, right where you left it.")
                                    OnboardingInfo(1, 2, ZIcons.Laptop, "Run agents on your computers", "Start work on your connected computers from whichever device you're using.")
                                }
                                if (client != null) Text(if (model.isDemo) "You're exploring the offline demo." else "Signed in as ${model.accountName}",
                                    style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            OnboardingStep.DONE -> DonePage(model)
                            else -> Unit
                        }
                    }
                }
            }
            Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp).padding(top = 12.dp, bottom = 12.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp)) {
                if (step == OnboardingStep.DONE) {
                    AppButton(onClick = { onFinish(true) }, modifier = Modifier.fillMaxWidth().testTag("onboarding-finish")) {
                        ZIcon(ZIcons.Plus, null, Modifier.size(18.dp))
                        Spacer(Modifier.width(6.dp))
                        Text("Start a session")
                    }
                    AppTextButton(onClick = { onFinish(false) }, modifier = Modifier.fillMaxWidth()) { Text("I'll do this later") }
                } else {
                    if (step == OnboardingStep.SYNC && client == null) AccountSignIn(model)
                    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                        AppTextButton(onClick = { model.onboarding.back() }) { Text("Back") }
                        Spacer(Modifier.weight(1f))
                        if (step != OnboardingStep.SYNC || client != null) {
                            AppButton(onClick = { model.onboarding.next() }, modifier = Modifier.testTag("onboarding-continue")) { Text("Continue") }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ColumnScope.AppearancePage(model: AppModel) {
    val appearance by model.appearance.collectAsState()
    val wallpaper by model.wallpaper.state.collectAsState()
    OnboardingHeading("Make it yours", "Pick a look. You can change it any time in Settings.")
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("Color scheme", style = MaterialTheme.typography.titleSmall)
        Row(Modifier.fillMaxWidth().clip(CircleShape).background(glassControlFill()).padding(3.dp)) {
            ThemeMode.entries.forEach { mode ->
                val active = appearance.mode == mode
                Box(Modifier.weight(1f).clip(CircleShape)
                    .background(if (active) MaterialTheme.colorScheme.surfaceContainerLowest else androidx.compose.ui.graphics.Color.Transparent)
                    .pressable(role = Role.RadioButton) { model.setAppearance(appearance.copy(mode = mode)) }
                    .semantics { selected = active }.heightIn(min = 48.dp), contentAlignment = Alignment.Center) {
                    Text(mode.name, style = MaterialTheme.typography.titleSmall)
                }
            }
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("New chat wallpaper", style = MaterialTheme.typography.titleSmall)
        if (wallpaper.set) {
            val dark = LocalDarkTheme.current
            val render by produceState<WallpaperStore.Render?>(null, wallpaper, dark) { value = model.wallpaper.render(dark) }
            render?.let { Image(it.image, "Wallpaper preview", Modifier.fillMaxWidth().height(140.dp).clip(RoundedCornerShape(20.dp)), contentScale = ContentScale.Crop) }
        }
        WallpaperSettings(model, Modifier.fillMaxWidth())
    }
}

@Composable
private fun ColumnScope.DonePage(model: AppModel) {
    val appearance by model.appearance.collectAsState()
    val wallpaper by model.wallpaper.state.collectAsState()
    Spacer(Modifier.height(12.dp))
    Box(Modifier.align(Alignment.CenterHorizontally).size(52.dp).clip(RoundedCornerShape(20.dp))
        .background(MaterialTheme.colorScheme.primary.copy(alpha = 0.14f)), contentAlignment = Alignment.Center) {
        ZIcon(ZIcons.Check, null, Modifier.size(24.dp), tint = MaterialTheme.colorScheme.primary)
    }
    OnboardingHeading("You're all set", "Pick a project and start a session on any of your connected computers.", centered = true)
    Column {
        GroupedRow(0, 2, supportingContent = { Text(appearance.mode.name + if (wallpaper.set) " · Wallpaper" else "") }) { Text("Appearance") }
        GroupedRow(1, 2, supportingContent = { Text(if (model.isDemo) "Offline demo" else model.accountName) }) { Text("Sync") }
    }
}

@Composable
private fun OnboardingInfo(index: Int, count: Int, icon: Int, title: String, detail: String) {
    GroupedRow(index, count, leadingContent = { IconTile(icon) }, supportingContent = { Text(detail) }) { Text(title) }
}

@Composable
private fun OnboardingHeading(title: String, detail: String, modifier: Modifier = Modifier, centered: Boolean = false) {
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp),
        horizontalAlignment = if (centered) Alignment.CenterHorizontally else Alignment.Start) {
        Text(title, style = MaterialTheme.typography.headlineMedium,
            textAlign = if (centered) TextAlign.Center else TextAlign.Start)
        Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = if (centered) TextAlign.Center else TextAlign.Start)
    }
}
