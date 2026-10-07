package sh.zeron.android.update

import android.app.Activity
import android.app.Application
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.SystemClock
import android.provider.Settings
import android.util.Log
import androidx.core.content.FileProvider
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import sh.zeron.android.BuildConfig
import uniffi.zeron_core.authProductionEdgeUrl
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.net.URL
import java.security.MessageDigest
import javax.net.ssl.HttpsURLConnection

data class UpdateState(
    val currentVersion: String = BuildConfig.VERSION_NAME,
    val latest: AndroidRelease? = null,
    val checking: Boolean = false,
    val checked: Boolean = false,
    val downloading: Boolean = false,
    val downloadedBytes: Long = 0,
    val totalBytes: Long? = null,
    val ready: Boolean = false,
    val prompt: Boolean = false,
    val error: String? = null,
)

/** APKs stay private until verified, then Android's installer asks the user. */
class AndroidUpdater(private val app: Application) {
    private val scope = MainScope()
    private val prefs = app.getSharedPreferences("updates", 0)
    private val directory = File(app.cacheDir, "updates")
    private val base = "${authProductionEdgeUrl().trimEnd('/')}/releases"
    private val _state = MutableStateFlow(UpdateState())
    val state = _state.asStateFlow()
    private var checkJob: Job? = null
    private var downloadJob: Job? = null
    private var installJob: Job? = null
    private var dismissedVersion: String? = null
    private val restoration = scope.launch {
        val restored = withContext(Dispatchers.IO) {
            directory.mkdirs()
            directory.listFiles()?.filter { it.name.endsWith(".part") }?.forEach { it.delete() }
            runCatching {
                val saved = JSONObject(prefs.getString("ready", null) ?: return@runCatching null)
                val release = UpdatePolicy.release(saved.getString("version"), saved.getString("sha256"))
                verifyApk(apk(release), release)
                release
            }.getOrNull()
        }
        if (restored != null) _state.value = _state.value.copy(latest = restored, ready = true)
        else prefs.edit().remove("ready").remove("pendingInstall").apply()
        // Installed/obsolete packages and incomplete downloads must not accumulate.
        withContext(Dispatchers.IO) {
            directory.listFiles()?.filter { it.name != restored?.fileName }?.forEach { it.delete() }
        }
    }

    fun onForeground() = check(manual = false)

    fun dismiss() {
        dismissedVersion = _state.value.latest?.version
        _state.value = _state.value.copy(prompt = false)
    }

    fun check(manual: Boolean = true) {
        if (manual) _state.value = _state.value.copy(prompt = true)
        if (checkJob?.isActive == true || downloadJob?.isActive == true) return
        val elapsed = System.currentTimeMillis() - prefs.getLong("lastCheck", 0)
        if (!manual && elapsed in 0 until 60 * 60 * 1000L) return
        checkJob = scope.launch {
            restoration.join()
            _state.value = _state.value.copy(checking = true, error = null)
            try {
                val found = withContext(Dispatchers.IO) { fetchRelease() }
                val previous = _state.value
                // A stale feed must not discard an already verified newer download.
                val latest = if (previous.ready && (found == null || previous.latest!!.versionCode > found.versionCode)) {
                    previous.latest
                } else found
                _state.value = previous.copy(
                    checking = false, checked = true, latest = latest,
                    ready = previous.ready && previous.latest == latest,
                    prompt = previous.prompt || (latest != null && latest.version != dismissedVersion),
                )
                prefs.edit().putLong("lastCheck", System.currentTimeMillis()).apply()
            } catch (e: Exception) {
                if (e is CancellationException) throw e
                Log.w("ZerunUpdate", "Release check failed", e)
                _state.value = _state.value.copy(checking = false, error = "Could not check for updates. Check your connection and try again.")
            }
        }
    }

    fun download() {
        val release = _state.value.latest ?: return
        if (downloadJob?.isActive == true || _state.value.ready) return
        downloadJob = scope.launch {
            _state.value = _state.value.copy(downloading = true, error = null, downloadedBytes = 0, totalBytes = null)
            try {
                withContext(Dispatchers.IO) { downloadApk(release) }
                prefs.edit().putString("ready", JSONObject().put("version", release.version).put("sha256", release.sha256).toString()).apply()
                _state.value = _state.value.copy(downloading = false, ready = true)
            } catch (e: Exception) {
                if (e is CancellationException) throw e
                Log.w("ZerunUpdate", "APK download failed", e)
                _state.value = _state.value.copy(downloading = false, ready = false, error = e.message ?: "Download failed. Please try again.")
            }
        }
    }

    fun install(activity: Activity) {
        if (!_state.value.ready) return
        prefs.edit().putBoolean("pendingInstall", true).apply()
        if (!app.packageManager.canRequestPackageInstalls()) {
            try {
                activity.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${app.packageName}")))
            } catch (_: ActivityNotFoundException) {
                _state.value = _state.value.copy(error = "Allow Zerun to install apps in Android Settings, then tap Install again.")
            }
            return
        }
        launchInstaller(activity)
    }

    fun onResume(activity: Activity) {
        scope.launch {
            restoration.join()
            if (prefs.getBoolean("pendingInstall", false) && app.packageManager.canRequestPackageInstalls()) {
                launchInstaller(activity)
            }
        }
    }

    private fun launchInstaller(activity: Activity) {
        val release = _state.value.latest ?: return
        if (!_state.value.ready || installJob?.isActive == true) return
        installJob = scope.launch {
            try {
                // Recheck after process recreation or a round trip through Settings.
                withContext(Dispatchers.IO) { verifyApk(apk(release), release) }
                val uri = FileProvider.getUriForFile(app, "${app.packageName}.updates", apk(release))
                val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
                    .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                prefs.edit().remove("pendingInstall").apply()
                activity.startActivity(intent)
            } catch (e: Exception) {
                if (e is CancellationException) throw e
                prefs.edit().remove("pendingInstall").apply()
                if (e is IOException) {
                    prefs.edit().remove("ready").apply()
                    _state.value = _state.value.copy(ready = false)
                }
                Log.w("ZerunUpdate", "Installer launch failed", e)
                _state.value = _state.value.copy(error = e.message ?: "Could not open the Android installer. Please try again.")
            }
        }
    }

    private fun apk(release: AndroidRelease) = File(directory, release.fileName)

    private fun connection(path: String): HttpsURLConnection {
        val url = URL("$base/$path")
        if (url.protocol != "https") throw IOException("Updates require an HTTPS release source.")
        return (url.openConnection() as HttpsURLConnection).apply {
            connectTimeout = 20_000
            readTimeout = 30_000
            useCaches = false
            instanceFollowRedirects = false
            setRequestProperty("Cache-Control", "no-cache")
            setRequestProperty("User-Agent", "Zerun-Android/${BuildConfig.VERSION_NAME}")
        }
    }

    private fun fetchRelease(): AndroidRelease? {
        val connection = connection("manifest.json")
        try {
            if (connection.responseCode != 200) throw IOException("Release source returned HTTP ${connection.responseCode}.")
            val bytes = ByteArrayOutputStream()
            connection.inputStream.use { input ->
                val buffer = ByteArray(8192)
                while (true) {
                    val count = input.read(buffer)
                    if (count < 0) break
                    if (bytes.size() + count > 1024 * 1024) throw IOException("Release manifest is too large.")
                    bytes.write(buffer, 0, count)
                }
            }
            val manifest = JSONObject(bytes.toString("UTF-8"))
            val version = manifest.getString("version")
            if (UpdatePolicy.versionCode(version) <= BuildConfig.VERSION_CODE) return null
            val name = "zerun-$version-android.apk"
            val metadata = manifest.getJSONObject("files").optJSONObject(name)
                ?: throw IOException("The Android package is not available for this release yet.")
            return UpdatePolicy.release(version, metadata.getString("sha256"))
        } finally {
            connection.disconnect()
        }
    }

    private suspend fun downloadApk(release: AndroidRelease) {
        directory.mkdirs()
        val part = File(directory, "${release.fileName}.part")
        val connection = connection(release.fileName)
        try {
            if (connection.responseCode != 200) throw IOException("Download returned HTTP ${connection.responseCode}. Please try again.")
            val total = connection.contentLengthLong.takeIf { it > 0 }
            val maxBytes = 512 * 1024 * 1024L
            if (total != null && total > maxBytes) throw IOException("The update exceeds the download size limit.")
            var received = 0L
            var lastProgress = 0L
            connection.inputStream.use { input ->
                part.outputStream().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val count = input.read(buffer)
                        if (count < 0) break
                        received += count
                        if (received > maxBytes) throw IOException("The update exceeds the download size limit.")
                        output.write(buffer, 0, count)
                        val now = SystemClock.elapsedRealtime()
                        if (now - lastProgress >= 250) {
                            withContext(Dispatchers.Main) { _state.value = _state.value.copy(downloadedBytes = received, totalBytes = total) }
                            lastProgress = now
                        }
                    }
                }
            }
            if (total != null && received != total) throw IOException("The download was incomplete. Please try again.")
            verifyApk(part, release)
            if (!part.renameTo(apk(release))) throw IOException("Could not save the update. Please try again.")
            directory.listFiles()?.filter { it.name != release.fileName }?.forEach { it.delete() }
        } finally {
            connection.disconnect()
            part.delete()
        }
    }

    @Suppress("DEPRECATION")
    private fun verifyApk(file: File, release: AndroidRelease) {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(64 * 1024)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                digest.update(buffer, 0, count)
            }
        }
        UpdatePolicy.verifyChecksum(release, digest.digest().hex())
        val info = app.packageManager.getPackageArchiveInfo(file.path, PackageManager.GET_SIGNING_CERTIFICATES)
            ?: throw IOException("Android could not read the downloaded APK.")
        val signers = info.signingInfo?.apkContentsSigners.orEmpty().map {
            MessageDigest.getInstance("SHA-256").digest(it.toByteArray()).hex()
        }
        UpdatePolicy.verifyPackage(release, info.packageName, info.longVersionCode, info.versionName,
            signers, app.packageName, BuildConfig.RELEASE_CERTIFICATE_SHA256, BuildConfig.VERSION_CODE.toLong())
    }

    private fun ByteArray.hex() = joinToString("") { "%02x".format(it.toInt() and 255) }
}
