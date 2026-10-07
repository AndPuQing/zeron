package sh.zeron.android.update

import java.io.IOException

data class AndroidRelease(
    val version: String,
    val versionCode: Long,
    val fileName: String,
    val sha256: String,
)

/** Validation shared by the feed, saved downloads, and APK inspection. */
object UpdatePolicy {
    private val versionPattern = Regex("(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)")
    private val digestPattern = Regex("[a-fA-F0-9]{64}")

    fun versionCode(version: String): Long {
        val parts = versionPattern.matchEntire(version)?.groupValues?.drop(1)
            ?.map { it.toLongOrNull() ?: throw IOException("Invalid release version.") }
            ?: throw IOException("Invalid release version.")
        if (parts[0] > 2100 || parts[1] >= 1000 || parts[2] >= 1000) {
            throw IOException("Release version is out of range.")
        }
        val code = parts[0] * 1_000_000 + parts[1] * 1000 + parts[2]
        if (code !in 1..2_100_000_000L) throw IOException("Release version is out of range.")
        return code
    }

    fun release(version: String, sha256: String): AndroidRelease {
        val code = versionCode(version)
        if (!digestPattern.matches(sha256)) throw IOException("The release checksum is missing or invalid.")
        return AndroidRelease(version, code, "zerun-$version-android.apk", sha256.lowercase())
    }

    fun verifyChecksum(release: AndroidRelease, actual: String) {
        if (!release.sha256.equals(actual, ignoreCase = true)) {
            throw IOException("The download failed its integrity check. Please download it again.")
        }
    }

    fun verifyPackage(
        release: AndroidRelease,
        packageName: String,
        versionCode: Long,
        versionName: String?,
        signerDigests: List<String>,
        expectedPackage: String,
        expectedSigner: String,
        installedVersionCode: Long,
    ) {
        if (packageName != expectedPackage) throw IOException("This APK belongs to a different application.")
        if (versionCode != release.versionCode || versionName != release.version) {
            throw IOException("The APK version does not match the release.")
        }
        if (versionCode <= installedVersionCode) throw IOException("This APK is not a newer version.")
        if (signerDigests.size != 1 || !signerDigests.single().equals(expectedSigner, ignoreCase = true)) {
            throw IOException("The APK does not have the trusted release signature.")
        }
    }
}
