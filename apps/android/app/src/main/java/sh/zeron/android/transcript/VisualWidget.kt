package sh.zeron.android.transcript

import android.animation.ValueAnimator
import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.toggleable
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import sh.zeron.android.design.LocalDarkTheme
import uniffi.zeron_core.ControlKind
import uniffi.zeron_core.PointerPhase
import uniffi.zeron_core.SurfaceFrame
import uniffi.zeron_core.SurfaceGesture
import uniffi.zeron_core.VisualControl
import uniffi.zeron_core.VisualDisplay
import uniffi.zeron_core.VisualListener
import uniffi.zeron_core.VisualSession
import uniffi.zeron_core.VisualSurface
import kotlin.math.roundToInt

/** Native paint and touch targets for the Luau runtime already shared with iOS. */
@Composable
internal fun VisualWidget(state: TranscriptState, id: String, actions: TranscriptActions) {
    val session = remember(state, id) { state.engine.visual(id) } ?: return
    val title = remember(session) { session.title() }
    val density = LocalDensity.current.density
    val sink = remember(session, density) { VisualSink(density.coerceAtMost(2f)) }
    val dark = LocalDarkTheme.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var visible by remember { mutableStateOf(false) }

    DisposableEffect(session, sink, lifecycle, visible, dark) {
        var attached = false
        fun update() {
            val wanted = visible && lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)
            if (wanted == attached) return
            attached = wanted
            if (wanted) session.attach(sink, dark, !ValueAnimator.areAnimatorsEnabled()) else session.detach()
        }
        val observer = LifecycleEventObserver { _, _ -> update() }
        lifecycle.addObserver(observer)
        update()
        onDispose {
            lifecycle.removeObserver(observer)
            if (attached) session.detach()
        }
    }
    DisposableEffect(session, sink) {
        onDispose { sink.close() }
    }
    // UniFFI releases the handle after cancelled touch coroutines finish;
    // the transcript hub owns and shuts down the actual worker.

    Box(
        Modifier.fillMaxSize().clipToBounds()
            .onGloballyPositioned {
                val bounds = it.boundsInWindow()
                visible = bounds.width > 0 && bounds.height > 0
            }
            .semantics { contentDescription = title },
    ) {
        val display = sink.display ?: return@Box
        VisualBitmap(sink.base, Modifier.fillMaxSize())
        for (surface in display.surfaces) {
            key(surface.id) {
                VisualSurfaceContent(session, surface, density, sink)
            }
        }
        // Later controls take precedence over a tappable container below them.
        for (control in display.controls) {
            key(control.id) { VisualControlTarget(session, control, density, actions.openUrl) }
        }
    }
}

@Composable
private fun VisualSurfaceContent(session: VisualSession, surface: VisualSurface, density: Float, sink: VisualSink) {
    val pixels = sink.frames[surface.id]
    Box(
        Modifier.offset(surface.x.dp, surface.y.dp).size(surface.w.dp, surface.h.dp)
            .clipToBounds().surfaceTouches(session, surface, density),
    ) {
        VisualBitmap(pixels?.image, Modifier.fillMaxSize())
        VisualBitmap(pixels?.paint, Modifier.fillMaxSize())
    }
}

@Composable
private fun VisualBitmap(bitmap: Bitmap?, modifier: Modifier) {
    if (bitmap == null) return
    val image = remember(bitmap) { bitmap.asImageBitmap() }
    Canvas(modifier) {
        drawImage(image, dstSize = androidx.compose.ui.unit.IntSize(size.width.roundToInt(), size.height.roundToInt()))
    }
}

private data class SurfacePixels(val image: Bitmap?, val paint: Bitmap?)

/** Worker callbacks coalesce to one main-loop update, retaining only the newest frame per surface. */
private class VisualSink(private val scale: Float) : VisualListener {
    var display by mutableStateOf<VisualDisplay?>(null)
        private set
    var base by mutableStateOf<Bitmap?>(null)
        private set
    val frames = mutableStateMapOf<ULong, SurfacePixels>()
    private val main = Handler(Looper.getMainLooper())
    private val fonts = StyleFonts()
    private val lock = Any()
    private var sizes = emptyMap<ULong, Pair<Float, Float>>()
    private var pendingDisplay: Pair<VisualDisplay, Bitmap?>? = null
    private val pendingFrames = HashMap<ULong, SurfacePixels>()
    private var closed = false
    private var scheduled = false
    private val drain = Runnable {
        val (next, updates) = synchronized(lock) {
            scheduled = false
            val batch = pendingDisplay to pendingFrames.toMap()
            pendingDisplay = null
            pendingFrames.clear()
            if (closed) return@Runnable
            batch
        }
        next?.let { (d, image) ->
            display = d
            base = image
            val live = d.surfaces.mapTo(HashSet()) { it.id }
            frames.keys.retainAll(live)
        }
        val live = display?.surfaces?.mapTo(HashSet()) { it.id }.orEmpty()
        for ((id, pixels) in updates) if (id in live) frames[id] = pixels
    }

    override fun display(display: VisualDisplay) {
        fonts.update(display.styles, 1f)
        val image = VisualPainter.render(display.paint, display.width, display.height, scale, fonts)
        synchronized(lock) {
            if (closed) return
            sizes = display.surfaces.associate { it.id to (it.w to it.h) }
            pendingDisplay = display to image
            schedule()
        }
    }

    override fun frame(frame: SurfaceFrame) {
        val size = synchronized(lock) { if (closed) null else sizes[frame.surface] } ?: return
        fonts.update(frame.styles, 1f)
        val pixels = SurfacePixels(
            frame.image?.let(VisualPainter::image),
            VisualPainter.render(frame.paint, size.first, size.second, scale, fonts),
        )
        synchronized(lock) {
            if (closed) return
            pendingFrames[frame.surface] = pixels
            schedule()
        }
    }

    private fun schedule() {
        if (!scheduled) {
            scheduled = true
            main.post(drain)
        }
    }

    fun close() = synchronized(lock) {
        closed = true
        pendingDisplay = null
        pendingFrames.clear()
        sizes = emptyMap()
        main.removeCallbacks(drain)
    }
}

@Composable
private fun VisualControlTarget(session: VisualSession, control: VisualControl, density: Float, openUrl: (String) -> Unit) {
    var modifier = Modifier.offset(control.x.dp, control.y.dp).size(control.w.dp, control.h.dp)
        .semantics { contentDescription = control.label }
    modifier = when (val kind = control.kind) {
        is ControlKind.Slider -> modifier.sliderTouches(session, control, kind, density)
        is ControlKind.Tab -> modifier.selectable(kind.selected, role = Role.Tab) { session.activate(control.id) }
        is ControlKind.Toggle -> modifier.toggleable(kind.on, role = Role.Switch) { session.activate(control.id) }
        is ControlKind.Link -> modifier.clickable(role = Role.Button) { openUrl(kind.url) }
        else -> modifier.clickable(role = Role.Button) { session.activate(control.id) }
    }
    Box(modifier)
}

private fun Modifier.sliderTouches(session: VisualSession, c: VisualControl, s: ControlKind.Slider, density: Float): Modifier {
    fun slide(x: Float) {
        if (s.trackW > 0) session.slide(c.id, ((c.x + x / density - s.trackX) / s.trackW).coerceIn(0f, 1f))
    }
    fun progress(value: Float): Boolean {
        if (s.max <= s.min) return false
        session.slide(c.id, ((value - s.min) / (s.max - s.min)).toFloat().coerceIn(0f, 1f))
        return true
    }
    return semantics {
        stateDescription = s.shown
        progressBarRangeInfo = ProgressBarRangeInfo(
            s.value.toFloat(), s.min.toFloat()..s.max.toFloat(),
            if (s.step > 0) ((s.max - s.min) / s.step).roundToInt().minus(1).coerceAtLeast(0) else 0,
        )
        setProgress { progress(it) }
    }.onKeyEvent {
        if (it.type != KeyEventType.KeyDown) return@onKeyEvent false
        val direction = when (it.key) {
            Key.DirectionRight, Key.DirectionUp -> 1
            Key.DirectionLeft, Key.DirectionDown -> -1
            else -> return@onKeyEvent false
        }
        progress((s.value + direction * (if (s.step > 0) s.step else (s.max - s.min) / 10)).toFloat())
    }.focusable()
        .pointerInput(session, c.id, c.x, s.trackX, s.trackW, density) {
            detectTapGestures { slide(it.x) }
        }
        .pointerInput(session, c.id, c.x, s.trackX, s.trackW, density) {
            var x = 0f
            detectHorizontalDragGestures(
                onDragStart = { x = it.x; slide(x) },
                onHorizontalDrag = { change, _ -> change.consume(); x = change.position.x; slide(x) },
            )
        }
}

@Composable
private fun Modifier.surfaceTouches(session: VisualSession, surface: VisualSurface, density: Float): Modifier {
    val scope = rememberCoroutineScope()
    val readout = remember { ReadoutTimer() }
    fun send(phase: PointerPhase, p: Offset) = session.pointer(surface.id, phase, p.x / density, p.y / density)
    fun cancelReadout() { readout.job?.cancel(); readout.job = null }
    DisposableEffect(session, surface.id, surface.gesture) {
        onDispose {
            cancelReadout()
            send(PointerPhase.CANCEL, Offset.Zero)
        }
    }
    return when (surface.gesture) {
        SurfaceGesture.NONE -> this
        SurfaceGesture.TAP -> pointerInput(session, surface.id) {
            detectTapGestures { send(PointerPhase.DOWN, it); send(PointerPhase.UP, it) }
        }
        SurfaceGesture.DRAG -> pointerInput(session, surface.id) {
            awaitEachGesture {
                val down = awaitFirstDown()
                down.consume()
                var p = down.position
                var ended = false
                send(PointerPhase.DOWN, p)
                try {
                    while (true) {
                        val change = awaitPointerEvent(PointerEventPass.Main).changes.firstOrNull { it.id == down.id } ?: break
                        p = change.position
                        if (change.isConsumed) break
                        change.consume()
                        if (!change.pressed) { send(PointerPhase.UP, p); ended = true; break }
                        send(PointerPhase.MOVE, p)
                    }
                } finally {
                    if (!ended) send(PointerPhase.CANCEL, p)
                }
            }
        }
        SurfaceGesture.HORIZONTAL_DRAG -> pointerInput(session, surface.id) {
            detectTapGestures { p ->
                cancelReadout()
                send(PointerPhase.DOWN, p)
                readout.job = scope.launch { delay(2500); send(PointerPhase.UP, p) }
            }
        }.pointerInput(session, surface.id) {
            var p = Offset.Zero
            detectHorizontalDragGestures(
                onDragStart = { cancelReadout(); p = it; send(PointerPhase.DOWN, p) },
                onDragEnd = { send(PointerPhase.UP, p) },
                onDragCancel = { send(PointerPhase.CANCEL, p) },
                onHorizontalDrag = { change, _ -> change.consume(); p = change.position; send(PointerPhase.MOVE, p) },
            )
        }
    }
}

private class ReadoutTimer { var job: Job? = null }
