package sh.zeron.android.voice

import android.app.*
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.Uri
import android.os.Build
import android.os.IBinder
import android.os.SystemClock
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import sh.zeron.android.MainActivity
import sh.zeron.android.R
import sh.zeron.android.ZeronApplication

/** Foreground microphone ownership survives navigation and locking, never process death. */
class VoiceCallService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val voice get() = (application as ZeronApplication).model.voice
    private var observing = false
    private var sessionId = 0L
    override fun onBind(intent: Intent?): IBinder? = null
    override fun onCreate() {
        super.onCreate()
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Voice calls", NotificationManager.IMPORTANCE_LOW).apply {
                description = "Ongoing Codex voice calls and call controls"
                setSound(null, null)
            })
    }
    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        sessionId = voice.state.value.sessionId
        // Even a queued start after hang-up must satisfy Android's foreground deadline.
        val types = if (Build.VERSION.SDK_INT >= 30) ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE or
            ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK else 0
        ServiceCompat.startForeground(this, ID, notification(voice.state.value), types)
        when (intent?.action?.takeIf { intent.getLongExtra("session", -1) == sessionId }) {
            END -> voice.stop()
            MUTE -> voice.toggleMute()
        }
        if (!voice.state.value.live) { stopSelf(); return START_NOT_STICKY }
        if (!observing) {
            observing = true
            scope.launch {
                voice.state.map { Notice(it.live, it.muted, it.hostName, it.activeSince) }.distinctUntilChanged().collect { state ->
                    if (!state.live) stopSelf()
                    else getSystemService(NotificationManager::class.java).notify(ID, notification(voice.state.value))
                }
            }
        }
        return START_NOT_STICKY
    }
    private data class Notice(val live: Boolean, val muted: Boolean, val host: String, val since: Long?)
    private fun notification(state: VoiceUiState): Notification {
        val open = Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            .putExtra("open-voice", true)
        fun action(action: String, code: Int) = PendingIntent.getService(this, code,
            Intent(this, VoiceCallService::class.java).setAction(action).putExtra("session", state.sessionId)
                .setData(Uri.parse("zerun-voice:${state.sessionId}/$code")), PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.zi_microphone)
            .setContentTitle("Codex voice · ${state.hostName}")
            .setContentText(if (state.muted) "Microphone muted" else if (state.activeSince == null) "Connecting…" else "Voice call in progress")
            .setContentIntent(PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
            .setDeleteIntent(action(END, 2))
            .setOngoing(true).setOnlyAlertOnce(true).setCategory(NotificationCompat.CATEGORY_CALL)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setUsesChronometer(state.activeSince != null)
            .setWhen(System.currentTimeMillis() - (SystemClock.elapsedRealtime() - (state.activeSince ?: SystemClock.elapsedRealtime())))
            .addAction(0, if (state.muted) "Unmute" else "Mute", action(MUTE, 1))
            .addAction(0, "End call", action(END, 2)).build()
    }
    override fun onTaskRemoved(rootIntent: Intent?) { voice.stop(); stopSelf() }
    override fun onDestroy() {
        scope.cancel()
        voice.stopSession(sessionId)
        super.onDestroy()
    }
    companion object {
        private const val CHANNEL = "voice-calls"
        private const val ID = 3461
        private const val END = "work.puqing.zerun.android.voice.END"
        private const val MUTE = "work.puqing.zerun.android.voice.MUTE"
        fun start(context: Context) { ContextCompat.startForegroundService(context, Intent(context, VoiceCallService::class.java)) }
        fun stop(context: Context) { context.stopService(Intent(context, VoiceCallService::class.java)) }
    }
}
