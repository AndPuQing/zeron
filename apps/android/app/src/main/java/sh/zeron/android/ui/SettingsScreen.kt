package sh.zeron.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.ThemeMode
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import uniffi.zeron_core.coreVersion

@Composable
fun SettingsScreen(model: AppModel, onProviders: () -> Unit) {
    val update by model.updates.state.collectAsState()
    val appearance by model.appearance.collectAsState()
    val workspace by model.workspace.collectAsState()
    val devices = workspace?.devices.orEmpty()
    val list = androidx.compose.foundation.lazy.rememberLazyListState()
    Box(Modifier.fillMaxSize()) {
        LazyColumn(
            Modifier.fillMaxSize(), state = list,
            contentPadding = PaddingValues(top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding() + 8.dp, bottom = 200.dp),
        ) {
            item { ScreenHeader("Settings", null) }
            section("Account")
            item {
                GroupedRow(0, 1, Modifier.padding(horizontal = 20.dp),
                    leadingContent = {
                        Box(Modifier.size(22.dp).clip(CircleShape).background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.1f)), contentAlignment = Alignment.Center) {
                            Text(model.accountName.take(1).uppercase(), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }, supportingContent = { Text(model.accountDetail) },
                ) { Text(model.accountName) }
            }
            section("Agents")
            item {
                GroupedRow(0, 1, modifier = Modifier.padding(horizontal = 20.dp).testTag("settings-providers"),
                    onClick = onProviders,
                    leadingContent = { IconTile(ZIcons.Laptop) },
                    supportingContent = { Text("Manage agents and accounts on your computers") },
                    trailingContent = { ZIcon(ZIcons.ChevronRight, null, Modifier.size(14.dp)) },
                ) { Text("Providers") }
            }
            section("Voice")
            item { VoiceSettingsRow(model) }
            if (devices.isNotEmpty()) {
                section("Devices")
                item {
                    Column(Modifier.padding(horizontal = 20.dp)) {
                        devices.forEachIndexed { i, device ->
                            GroupedRow(i, devices.size,
                                leadingContent = { IconTile(when {
                                    !device.isExecutionHost -> ZIcons.Phone
                                    device.platform == "linux" -> ZIcons.Server
                                    else -> ZIcons.Laptop
                                }) },
                                supportingContent = { Text(listOfNotNull(
                                    if (device.isSelf) "This device" else if (device.online) "Online" else "Offline",
                                    device.version?.let { "v$it" },
                                    if (device.sessionCount > 0u) "${device.sessionCount} sessions" else null,
                                ).joinToString(" · ")) },
                                trailingContent = { Box(Modifier.size(8.dp).clip(CircleShape).background(
                                    if (device.online || device.isSelf) successColor() else MaterialTheme.colorScheme.outline)) },
                            ) { Text(device.name) }
                        }
                    }
                }
            }
            section("Appearance")
            item {
                Column(Modifier.padding(horizontal = 20.dp)) {
                    val modes = listOf(
                        Triple(ThemeMode.System, "System", ZIcons.Monitor),
                        Triple(ThemeMode.Light, "Light", ZIcons.Sun),
                        Triple(ThemeMode.Dark, "Dark", ZIcons.Moon),
                    )
                    modes.forEachIndexed { index, (mode, label, icon) ->
                        GroupedRow(index, modes.size,
                            modifier = Modifier.semantics { selected = appearance.mode == mode; role = Role.RadioButton },
                            onClick = { model.setAppearance(appearance.copy(mode = mode)) },
                            leadingContent = { IconTile(icon) },
                            trailingContent = {
                                if (appearance.mode == mode) ZIcon(ZIcons.Check, "Selected", Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary)
                            },
                        ) { Text(label) }
                    }
                }
            }
            section("Wallpaper")
            item { WallpaperSettings(model) }
            section("About")
            item {
                Column(Modifier.padding(horizontal = 20.dp)) {
                    GroupedRow(0, 3,
                        leadingContent = { IconTile(ZIcons.Info) },
                        supportingContent = { Text("Zerun ${update.currentVersion} · core ${coreVersion()}") },
                    ) { Text("Version") }
                    GroupedRow(1, 3,
                        onClick = { model.updates.check() },
                        leadingContent = { IconTile(ZIcons.Refresh) },
                        supportingContent = { Text(when {
                            update.downloading -> "Downloading update…"
                            update.checking -> "Checking…"
                            update.ready -> "Version ${update.latest?.version} is ready to install"
                            update.latest != null -> "Version ${update.latest?.version} is available"
                            update.error != null -> "Check failed — tap to retry"
                            update.checked -> "You're up to date"
                            else -> "Check for a newer version of Zerun"
                        }) },
                        trailingContent = { ZIcon(ZIcons.ChevronRight, null, Modifier.size(14.dp), tint = MaterialTheme.colorScheme.outline) },
                    ) { Text("Check for updates") }
                    GroupedRow(2, 3,
                        onClick = { model.signOut() },
                        leadingContent = { IconTile(ZIcons.Logout, content = MaterialTheme.colorScheme.error) },
                    ) { Text(if (model.isDemo) "Leave demo" else "Sign out", color = MaterialTheme.colorScheme.error) }
                }
            }
        }
        StatusBarScrim(scrolled = list.firstVisibleItemIndex > 0 || list.firstVisibleItemScrollOffset > 0)
    }
}

private fun LazyListScope.section(title: String) {
    item {
        Text(title, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(start = 36.dp, top = 24.dp, bottom = 8.dp))
    }
}

/** Settings use unboxed, secondary glyphs, matching the iOS grouped cells. */
@Composable
fun IconTile(icon: Int, container: Color = Color.Transparent, content: Color = MaterialTheme.colorScheme.onSurfaceVariant) {
    Box(Modifier.size(22.dp), contentAlignment = Alignment.Center) { ZIcon(icon, null, Modifier.size(20.dp), tint = content) }
}

@Composable
internal fun WallpaperSettings(model: AppModel, modifier: Modifier = Modifier.padding(horizontal = 20.dp)) {
    val store = model.wallpaper
    val state by store.state.collectAsState()
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    val picker = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia(),
    ) { uri -> if (uri != null) scope.launch { store.set(uri, "Photo") } }
    var effects by androidx.compose.runtime.remember { androidx.compose.runtime.mutableStateOf(false) }
    val rows = if (state.set) 3 else 1
    Column(modifier) {
        GroupedRow(0, rows,
            onClick = { picker.launch(androidx.activity.result.PickVisualMediaRequest(androidx.activity.result.contract.ActivityResultContracts.PickVisualMedia.ImageOnly)) },
            leadingContent = { IconTile(ZIcons.Image) },
            supportingContent = { Text(if (state.set) state.name ?: "Photo" else "Shown behind new sessions and the sessions list") },
            trailingContent = { ZIcon(ZIcons.ChevronRight, null, Modifier.size(14.dp), tint = MaterialTheme.colorScheme.outline) },
        ) { Text(if (state.set) "Change wallpaper" else "Choose wallpaper") }
        if (state.set) {
            Box {
                GroupedRow(1, rows,
                    onClick = { effects = true },
                    leadingContent = { IconTile(ZIcons.Magic) },
                    supportingContent = { Text("${sh.zeron.android.core.WallpaperStore.label(state.effect)} — ${sh.zeron.android.core.WallpaperStore.detail(state.effect)}") },
                    trailingContent = { ZIcon(ZIcons.ChevronRight, null, Modifier.size(14.dp), tint = MaterialTheme.colorScheme.outline) },
                ) { Text("Effect") }
                ChoiceMenu(effects, { effects = false }, listOf(MenuSection("Effect", sh.zeron.android.core.WallpaperStore.effects.map { e ->
                    MenuChoice(sh.zeron.android.core.WallpaperStore.label(e), e == state.effect) { store.setEffect(e) }
                })))
            }
            GroupedRow(2, rows, onClick = { store.remove() },
                leadingContent = { IconTile(ZIcons.Delete, content = MaterialTheme.colorScheme.error) },
            ) { Text("Remove wallpaper", color = MaterialTheme.colorScheme.error) }
        }
    }
}
