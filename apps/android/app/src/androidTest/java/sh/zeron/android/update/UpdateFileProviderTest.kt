package sh.zeron.android.update

import android.content.Intent
import android.provider.OpenableColumns
import androidx.core.content.FileProvider
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.util.UUID
import org.junit.Assert.*
import org.junit.Test

class UpdateFileProviderTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Test fun installerUriReadsTheVerifiedBytesAndCarriesReadPermission() {
        val bytes = "APK transport fixture".toByteArray()
        withFixture("updates", "update.apk", bytes) { file ->
            val intent = updateInstallIntent(context, file)
            val uri = requireNotNull(intent.data)
            assertEquals(Intent.ACTION_VIEW, intent.action)
            assertEquals("application/vnd.android.package-archive", intent.type)
            assertEquals("${context.packageName}.updates", uri.authority)
            assertEquals(Intent.FLAG_GRANT_READ_URI_PERMISSION, intent.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION)
            assertEquals(0, intent.flags and Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            assertEquals(uri, intent.clipData!!.getItemAt(0).uri)
            assertEquals(intent.type, context.contentResolver.getType(uri))
            context.contentResolver.openInputStream(uri)!!.use { assertArrayEquals(bytes, it.readBytes()) }
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)!!.use {
                assertTrue(it.moveToFirst())
                assertEquals(file.name, it.getString(0))
                assertEquals(bytes.size.toLong(), it.getLong(1))
            }
        }
    }

    @Test fun bothProvidersRemainReadableAndUpdateAuthorityCannotExposeSharedImages() {
        withFixture("updates", "update.apk", byteArrayOf(1, 2, 3)) { apk ->
            withFixture("shared", "image.png", byteArrayOf(4, 5, 6)) { image ->
                val imageUri = FileProvider.getUriForFile(context, "${context.packageName}.files", image)
                context.contentResolver.openInputStream(imageUri)!!.use { assertArrayEquals(image.readBytes(), it.readBytes()) }
                val apkUri = requireNotNull(updateInstallIntent(context, apk).data)
                context.contentResolver.openInputStream(apkUri)!!.use { assertArrayEquals(apk.readBytes(), it.readBytes()) }
                assertThrows(IllegalArgumentException::class.java) {
                    FileProvider.getUriForFile(context, "${context.packageName}.updates", image)
                }
            }
        }
    }

    private fun withFixture(root: String, name: String, bytes: ByteArray, block: (File) -> Unit) {
        val staging = File(context.cacheDir, "provider-test-${UUID.randomUUID()}").apply { mkdirs() }
        val destination = File(File(context.cacheDir, root).apply { mkdirs() }, staging.name)
        try {
            File(staging, name).writeBytes(bytes)
            // Publish a populated directory atomically so startup's stale-file cleanup
            // cannot remove the test file while it is being prepared.
            check(staging.renameTo(destination))
            block(File(destination, name))
        } finally {
            staging.deleteRecursively()
            destination.deleteRecursively()
        }
    }
}
