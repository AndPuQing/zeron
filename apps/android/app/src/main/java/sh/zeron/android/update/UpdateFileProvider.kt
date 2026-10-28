package sh.zeron.android.update

import android.content.ClipData
import android.content.Context
import android.content.Intent
import androidx.core.content.FileProvider
import java.io.File
import java.io.IOException

/** A distinct component: Android identifies local providers by class, not authority. */
class UpdateFileProvider : FileProvider()

/** Exercise the same URI the installer will read, after the APK itself has been verified. */
internal fun updateInstallIntent(context: Context, file: File): Intent {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.updates", file)
    try {
        context.contentResolver.openFileDescriptor(uri, "r")?.use { descriptor ->
            if (descriptor.statSize != file.length()) throw IOException("The shared update has an unexpected size.")
        } ?: throw IOException("The update provider returned no file.")
    } catch (e: Exception) {
        throw IOException("Could not share the verified update with Android's installer.", e)
    }
    return Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, "application/vnd.android.package-archive")
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        .apply { clipData = ClipData.newRawUri("Zerun update", uri) }
}
