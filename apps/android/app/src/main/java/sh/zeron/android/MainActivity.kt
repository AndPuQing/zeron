package sh.zeron.android

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import sh.zeron.android.core.LaunchOptions
import sh.zeron.android.ui.ZeronRoot
import uniffi.zeron_core.authCallbackScheme

/**
 * Launch extras mirror the iOS launch arguments, e.g.
 * `adb shell am start -n sh.zeron.android/.MainActivity --ez demo true --es route chat:chat-veil`
 * (`--es wallpaper <path>` sets the wallpaper from a file the app can read).
 */
class MainActivity : ComponentActivity() {
    private val model get() = (application as ZeronApplication).model
    private val lifecycleObserver = object : DefaultLifecycleObserver {
        override fun onStart(owner: LifecycleOwner) = model.onForeground()
        override fun onStop(owner: LifecycleOwner) = model.onBackground()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        installSplashScreen()
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val extras = intent.extras
        model.boot(
            LaunchOptions(
                demo = extras?.getBoolean("demo") == true,
                fast = extras?.getBoolean("fast") == true,
                longReply = extras?.getBoolean("longreply") == true,
                big = extras?.getBoolean("big") == true,
                huge = extras?.getBoolean("huge") == true,
                noProjects = extras?.getBoolean("noprojects") == true,
                signedOut = extras?.getBoolean("signedout") == true,
                route = extras?.getString("route"),
                wallpaper = extras?.getString("wallpaper"),
                wallpaperEffect = extras?.getString("wallpaper-effect"),
                voicePreview = extras?.getBoolean("voice-preview") == true,
                onboarding = if (savedInstanceState == null) extras?.getString("onboarding") else null,
            ),
        )
        handleCallback(intent)
        if (intent.getBooleanExtra("open-voice", false) && model.voice.state.value.live) model.voice.openStage()
        ProcessLifecycleOwner.get().lifecycle.addObserver(lifecycleObserver)
        setContent { ZeronRoot(model) }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleCallback(intent)
        if (intent.getBooleanExtra("open-voice", false) && model.voice.state.value.live) model.voice.openStage()
    }

    override fun onResume() {
        super.onResume()
        model.updates.onResume(this)
    }

    override fun onDestroy() {
        ProcessLifecycleOwner.get().lifecycle.removeObserver(lifecycleObserver)
        super.onDestroy()
    }

    private fun handleCallback(intent: Intent?) {
        val data = intent?.data ?: return
        if (data.scheme != authCallbackScheme()) return
        when (data.host) {
            "callback" -> model.handleCallback(data.toString())
            "signed-in" -> model.agentSignIn.browserReturned()
        }
    }
}
