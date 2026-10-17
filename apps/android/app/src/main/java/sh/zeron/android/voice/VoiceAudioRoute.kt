package sh.zeron.android.voice

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager

/** Own audio focus and routing only while the confirmed call is active. */
internal class VoiceAudioRoute(context: Context, private val onInterruption: () -> Unit) : SensorEventListener {
    private val audio = context.getSystemService(AudioManager::class.java)
    private val sensors = context.getSystemService(SensorManager::class.java)
    private val power = context.getSystemService(PowerManager::class.java)
    private val main = Handler(Looper.getMainLooper())
    private val proximity = sensors?.getDefaultSensor(Sensor.TYPE_PROXIMITY)
    private var wake: PowerManager.WakeLock? = null
    private var active = false
    private var near = false
    private var speaker = true
    private var previousMode = AudioManager.MODE_NORMAL
    private var previousSpeaker = false
    private var previousDevice: AudioDeviceInfo? = null
    private val focus = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
        .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build())
        .setOnAudioFocusChangeListener({ change ->
            if (active && change < AudioManager.AUDIOFOCUS_GAIN) onInterruption()
        }, main).build()
    private val devices = object : AudioDeviceCallback() {
        override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>) = route()
        override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>) = route()
    }

    fun activate() {
        if (active) return
        checkNotNull(audio)
        check(audio.requestAudioFocus(focus) == AudioManager.AUDIOFOCUS_REQUEST_GRANTED) { "Audio focus unavailable" }
        previousMode = audio.mode
        @Suppress("DEPRECATION")
        previousSpeaker = audio.isSpeakerphoneOn
        if (Build.VERSION.SDK_INT >= 31) previousDevice = audio.communicationDevice
        active = true
        audio.mode = AudioManager.MODE_IN_COMMUNICATION
        audio.registerAudioDeviceCallback(devices, main)
        proximity?.let { sensors?.registerListener(this, it, SensorManager.SENSOR_DELAY_NORMAL) }
        if (power?.isWakeLockLevelSupported(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK) == true) {
            wake = power.newWakeLock(PowerManager.PROXIMITY_SCREEN_OFF_WAKE_LOCK, "Zerun:VoiceProximity")
        }
        route()
    }

    fun setSpeaker(enabled: Boolean) { speaker = enabled; route() }

    @Suppress("DEPRECATION")
    private fun route() {
        if (!active || audio == null) return
        var external = false
        if (Build.VERSION.SDK_INT >= 31) {
            val available = audio.availableCommunicationDevices
            val headset = available.firstOrNull { it.type in externalTypes }
            external = headset != null
            val type = if (speaker && !near) AudioDeviceInfo.TYPE_BUILTIN_SPEAKER else AudioDeviceInfo.TYPE_BUILTIN_EARPIECE
            (headset ?: available.firstOrNull { it.type == type }
                ?: available.firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER })?.let {
                if (audio.communicationDevice?.id != it.id) audio.setCommunicationDevice(it)
            }
        } else {
            external = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS).any { it.type in externalTypes }
            audio.isSpeakerphoneOn = !external && speaker && !near
        }
        val lock = wake
        if (near && !external && lock?.isHeld == false) lock.acquire()
        else if ((!near || external) && lock?.isHeld == true) lock.release()
    }

    override fun onSensorChanged(event: SensorEvent) {
        near = event.values[0] < (proximity?.maximumRange ?: 0f)
        route()
    }
    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) = Unit

    @Suppress("DEPRECATION")
    fun close() {
        if (!active) return
        active = false
        sensors?.unregisterListener(this)
        wake?.let { if (it.isHeld) it.release() }; wake = null
        audio?.unregisterAudioDeviceCallback(devices)
        if (Build.VERSION.SDK_INT >= 31) {
            audio?.clearCommunicationDevice()
            previousDevice?.let { audio?.setCommunicationDevice(it) }
        } else audio?.isSpeakerphoneOn = previousSpeaker
        audio?.mode = previousMode
        audio?.abandonAudioFocusRequest(focus)
    }

    private companion object {
        val externalTypes = setOf(AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
            AudioDeviceInfo.TYPE_USB_HEADSET, AudioDeviceInfo.TYPE_USB_DEVICE, AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
            AudioDeviceInfo.TYPE_BLE_HEADSET, AudioDeviceInfo.TYPE_HEARING_AID)
    }
}
