package sh.zeron.android.ui

import android.Manifest
import android.animation.ValueAnimator
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.SystemClock
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.scale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.delay
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.*
import sh.zeron.android.voice.*
import uniffi.zeron_core.*

@Composable
private fun rememberVoiceStart(model: AppModel): () -> Unit {
    val context = LocalContext.current
    val notifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { model.voice.start() }
    fun start() {
        if (Build.VERSION.SDK_INT >= 33 && ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        } else model.voice.start()
    }
    val microphone = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        if (it) start() else model.voice.permissionDenied()
    }
    return {
        if (model.voice.state.value.live) model.voice.openStage()
        else {
            model.voice.refresh()
            if (model.voice.state.value.startHost == null) model.voice.openSettings()
            else if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
                microphone.launch(Manifest.permission.RECORD_AUDIO)
            } else start()
        }
    }
}

/** The same call is reachable from home, an open session, settings and its notification. */
@Composable
fun VoiceEntry(model: AppModel, modifier: Modifier = Modifier) {
    val voice by model.voice.state.collectAsState()
    val start = rememberVoiceStart(model)
    if (!voice.available) return
    GlassSurface(onClick = start, modifier = modifier.size(44.dp).semantics { contentDescription = if (voice.live) "Open voice call" else "Start voice call" }) {
        Box(contentAlignment = Alignment.Center) {
            if (voice.live) VoiceOrbView(voice, Modifier.size(28.dp))
            else ZIcon(ZIcons.Microphone, null, Modifier.size(24.dp))
        }
    }
}

@Composable
fun VoiceCallStrip(model: AppModel, modifier: Modifier = Modifier) {
    val voice by model.voice.state.collectAsState()
    GlassSurface(onClick = model.voice::openStage, modifier = modifier.fillMaxWidth()) {
        Row(Modifier.padding(start = 16.dp, end = 6.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            VoiceOrbView(voice, Modifier.size(32.dp))
            Column(Modifier.weight(1f).padding(horizontal = 10.dp)) {
                Text("Codex voice · ${elapsed(voice.activeSince)}", style = MaterialTheme.typography.titleSmall)
                Text(voice.status, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
            }
            VoiceCircle(if (voice.muted) ZIcons.MicrophoneOff else ZIcons.Microphone, if (voice.muted) "Unmute microphone" else "Mute microphone", onClick = model.voice::toggleMute)
            VoiceCircle(ZIcons.HangUp, "End call", onClick = model.voice::stop, tint = Color(0xFFFF453A))
        }
    }
}

@Composable
fun VoiceSettingsRow(model: AppModel) {
    val voice by model.voice.state.collectAsState()
    GlassSurface(modifier = Modifier.padding(horizontal = 16.dp).fillMaxWidth().semantics { contentDescription = "Voice settings" }, shape = RoundedCornerShape(20.dp), onClick = model.voice::openSettings) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            ZIcon(ZIcons.Microphone, null, Modifier.size(24.dp))
            Column(Modifier.weight(1f)) {
                Text("Voice", style = MaterialTheme.typography.titleMedium)
                Text(voice.startHost?.name ?: "Choose a device and voice", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            ZIcon(ZIcons.ChevronRight, null, Modifier.size(18.dp))
        }
    }
}

@Composable
fun VoiceOverlays(model: AppModel, onTranscript: (String) -> Unit) {
    val voice by model.voice.state.collectAsState()
    val start = rememberVoiceStart(model)
    if (voice.settingsOpen) VoiceDialog(model.voice::closeSettings) { VoiceSettings(model, voice, start) }
    else if (voice.stageOpen) VoiceDialog(model.voice::closeStage) { VoiceStage(model, voice, start, onTranscript) }
}

@Composable
private fun VoiceDialog(onDismiss: () -> Unit, content: @Composable () -> Unit) {
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        androidx.compose.material3.Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) { content() }
    }
}

@Composable
private fun VoiceSettings(model: AppModel, voice: VoiceUiState, start: () -> Unit) {
    LazyColumn(Modifier.fillMaxSize().safeDrawingPadding().padding(horizontal = 20.dp), contentPadding = PaddingValues(bottom = 24.dp)) {
        item {
            Row(Modifier.fillMaxWidth().padding(vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Voice", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.weight(1f))
                VoiceCircle(ZIcons.Close, "Close voice settings", onClick = model.voice::closeSettings)
            }
            Text("Audio is sent to OpenAI for voice. Codex, its sign-in and tools run on the device you choose.",
                style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            SectionHeader("Codex runs on")
        }
        if (voice.devices.isEmpty()) item { Text("No execution device yet. Open Zerun on your computer or server with remote voice enabled.", Modifier.padding(16.dp)) }
        voice.devices.forEach { device -> item(key = "host-${device.id}") {
            val ready = device.online && VoiceUiState.compatible(device)
            VoiceChoice(device.name, if (!device.online) "Offline" else if (!ready) "Update Zerun and enable remote voice" else "Ready",
                device.id == voice.startHost?.id, ready && !voice.live) { model.voice.selectHost(device.id) }
        } }
        item {
            if (voice.live) Text("End the call to change devices.", Modifier.padding(12.dp), style = MaterialTheme.typography.bodySmall)
            SectionHeader("Voice · applies to your next call")
            VoiceChoice("Codex default", selected = voice.selectedStyle == null) { model.voice.selectStyle(null) }
        }
        voice.styles.forEach { style -> item(key = "style-$style") {
            VoiceChoice(style.replaceFirstChar { it.titlecase() }, selected = voice.selectedStyle == style) { model.voice.selectStyle(style) }
        } }
        item {
            Spacer(Modifier.height(24.dp))
            GlassSurface(onClick = start, enabled = voice.live || voice.startHost != null, modifier = Modifier.fillMaxWidth()) {
                Text(if (voice.live) "Return to call" else "Start voice call", Modifier.padding(18.dp), textAlign = TextAlign.Center,
                    color = if (voice.live || voice.startHost != null) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Text("Calls continue while the screen is locked. An audio interruption ends the call. Connected headsets take priority over the speaker.",
                Modifier.padding(16.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun VoiceChoice(title: String, subtitle: String? = null, selected: Boolean, enabled: Boolean = true, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).clickable(enabled = enabled, onClick = onClick).padding(horizontal = 16.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurface.copy(alpha = if (enabled) 1f else 0.45f))
            subtitle?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
        if (selected) ZIcon(ZIcons.Check, "Selected", Modifier.size(20.dp), MaterialTheme.colorScheme.primary)
    }
}

@Composable
private fun VoiceStage(model: AppModel, voice: VoiceUiState, start: () -> Unit, onTranscript: (String) -> Unit) {
    val context = LocalContext.current
    Column(Modifier.fillMaxSize().safeDrawingPadding().padding(horizontal = 20.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(Modifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            VoiceCircle(ZIcons.ChevronDown, "Back to Zerun", onClick = model.voice::closeStage)
            Column(Modifier.weight(1f).padding(horizontal = 12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(voice.hostName, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.bodySmall)
                Text(if (voice.live) elapsed(voice.activeSince) else "Call ended", fontFamily = GeistMono, fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            VoiceCircle(ZIcons.Chat, "Open voice transcript", enabled = voice.call?.chatId != null, tint =
                if (voice.call?.work == VoiceCallWork.AWAITING_INPUT) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface) {
                voice.call?.chatId?.let { model.voice.closeStage(); onTranscript(it) }
            }
        }
        Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.Center) {
            VoiceOrbView(voice, Modifier.fillMaxWidth().sizeIn(maxWidth = 340.dp, maxHeight = 340.dp).aspectRatio(1f))
        }
        Text(if (voice.endReason == null) voice.status else "Call ended", style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
        Text(voice.endReason?.let { voiceEndMessage(it, voice.hostName) } ?: voice.call?.caption.orEmpty().takeLast(240),
            Modifier.fillMaxWidth().heightIn(min = 100.dp).padding(top = 12.dp, bottom = 20.dp),
            style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center,
            maxLines = 5, overflow = TextOverflow.Ellipsis)
        GlassSurface(modifier = Modifier.padding(bottom = 20.dp)) {
            Row(Modifier.padding(10.dp), horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.CenterVertically) {
                if (voice.live) {
                    VoiceCircle(if (voice.muted) ZIcons.MicrophoneOff else ZIcons.Microphone, if (voice.muted) "Unmute microphone" else "Mute microphone", size = 60.dp, onClick = model.voice::toggleMute,
                        tint = if (voice.muted) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface)
                    VoiceCircle(ZIcons.Speaker, if (voice.speaker) "Use earpiece" else "Use speaker", size = 60.dp, onClick = model.voice::toggleSpeaker,
                        tint = if (voice.speaker) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface)
                    VoiceCircle(ZIcons.HangUp, "End call", size = 72.dp, tint = Color(0xFFFF453A), onClick = model.voice::stop)
                } else {
                    Text("Close", Modifier.clip(CircleShape).clickable { model.voice.dismissReason(); model.voice.closeStage() }.padding(18.dp))
                    if (voice.endReason == VoiceEndReason.MICROPHONE_DENIED) {
                        Text("Android Settings", Modifier.clip(CircleShape).clickable {
                            context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:${context.packageName}")))
                        }.padding(18.dp), color = MaterialTheme.colorScheme.primary)
                    } else Text("Call again", Modifier.clip(CircleShape).clickable(onClick = start).padding(18.dp), color = MaterialTheme.colorScheme.primary)
                }
            }
        }
    }
}

@Composable
private fun VoiceCircle(icon: Int, label: String, modifier: Modifier = Modifier, size: Dp = 44.dp,
    enabled: Boolean = true, tint: Color = MaterialTheme.colorScheme.onSurface, onClick: () -> Unit) {
    GlassSurface(onClick = onClick, enabled = enabled, contentColor = tint, modifier = modifier.size(size)) {
        Box(contentAlignment = Alignment.Center) {
            ZIcon(icon, label, Modifier.size(if (size >= 60.dp) 28.dp else 24.dp), tint.copy(alpha = if (enabled) 1f else 0.25f))
        }
    }
}

@Composable
private fun elapsed(since: Long?): String {
    var now by remember { mutableLongStateOf(SystemClock.elapsedRealtime()) }
    LaunchedEffect(since) { while (true) { now = SystemClock.elapsedRealtime(); delay(1_000) } }
    val seconds = since?.let { ((now - it).coerceAtLeast(0) / 1000).toInt() } ?: 0
    return if (seconds >= 3600) "%d:%02d:%02d".format(seconds / 3600, seconds / 60 % 60, seconds % 60)
        else "%d:%02d".format(seconds / 60, seconds % 60)
}

/** Paint the shared Rust orb, with the same geometry and crossfades as iOS. */
@Composable
private fun VoiceOrbView(voice: VoiceUiState, modifier: Modifier) {
    val renderer = remember { OrbRenderer(OrbPreset.HERO, voice.orb) }
    val dark = LocalDarkTheme.current
    val lifecycle by LocalLifecycleOwner.current.lifecycle.currentStateFlow.collectAsState()
    var frame by remember { mutableStateOf(renderer.nextFrame(false, true)) }
    LaunchedEffect(voice.orb) { renderer.setOrb(voice.orb) }
    LaunchedEffect(voice.call?.microphone, voice.call?.speaker) { renderer.setAudioLevels(voice.call?.microphone ?: 0f, voice.call?.speaker ?: 0f) }
    LaunchedEffect(renderer, lifecycle) {
        if (lifecycle.isAtLeast(Lifecycle.State.STARTED)) {
            var previous = 0L
            while (true) withFrameNanos { now -> if (now - previous > 33_000_000) {
                frame = renderer.nextFrame(true, !ValueAnimator.areAnimatorsEnabled()); previous = now
            } }
        } else frame = renderer.nextFrame(false, true)
    }
    DisposableEffect(renderer) { onDispose { renderer.close() } }
    Canvas(modifier) {
        val factor = size.minDimension / frame.size
        scale(factor, factor, pivot = Offset.Zero) {
            for (i in frame.lines.indices step 7) {
                val f = frame.lines; val ink = if (dark) 1 - f[i + 5] else f[i + 5]
                drawLine(Color(ink, ink, ink, f[i + 6]), Offset(f[i], f[i + 1]), Offset(f[i + 2], f[i + 3]), f[i + 4], StrokeCap.Round)
            }
            for (i in frame.dots.indices step 5) {
                val f = frame.dots; val ink = if (dark) 1 - f[i + 3] else f[i + 3]
                drawCircle(Color(ink, ink, ink, f[i + 4]), f[i + 2], Offset(f[i], f[i + 1]))
            }
        }
    }
}
