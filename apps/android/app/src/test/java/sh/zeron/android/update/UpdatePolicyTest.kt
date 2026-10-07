package sh.zeron.android.update

import java.io.IOException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class UpdatePolicyTest {
    private val signer = "8".repeat(64)
    private val release = UpdatePolicy.release("0.3.2", "a".repeat(64))

    @Test fun versionsAreOrderedNumericallyIncludingMajorTransitions() {
        assertEquals(3010L, UpdatePolicy.versionCode("0.3.10"))
        assertEquals(1_000_000L, UpdatePolicy.versionCode("1.0.0"))
        assertEquals(999_999L, UpdatePolicy.versionCode("0.999.999"))
        assertEquals(2_100_000_000L, UpdatePolicy.versionCode("2100.0.0"))
    }

    @Test fun malformedAndAmbiguousVersionsCannotSelectADownloadPath() {
        for (version in listOf("", "../0.3.2", "0.3.2/evil", "0.3.2-rc1", "0.3", "v0.3.2", "0.03.2", "0.1000.0", "2100.0.1", "999999999999999999999.0.0", "0.0.0")) {
            assertThrows(version, IOException::class.java) { UpdatePolicy.versionCode(version) }
        }
    }

    @Test fun missingOrInvalidChecksumsAreRejected() {
        for (sha in listOf("", "a".repeat(63), "g".repeat(64), "a".repeat(65))) {
            assertThrows(IOException::class.java) { UpdatePolicy.release("0.3.2", sha) }
        }
        assertEquals("zerun-0.3.2-android.apk", release.fileName)
    }

    @Test fun corruptAndTruncatedDownloadsNeverReachTheInstaller() {
        UpdatePolicy.verifyChecksum(release, "A".repeat(64))
        assertThrows(IOException::class.java) { UpdatePolicy.verifyChecksum(release, "b".repeat(64)) }
        assertThrows(IOException::class.java) { UpdatePolicy.verifyChecksum(release, "") }
    }

    private fun verify(
        pkg: String = "work.puqing.zerun.android", code: Long = 3002,
        name: String? = "0.3.2", signers: List<String> = listOf(signer), installed: Long = 3001,
    ) = UpdatePolicy.verifyPackage(release, pkg, code, name, signers,
        "work.puqing.zerun.android", signer, installed)

    @Test fun aMatchingNewProductionApkIsAccepted() = verify()

    @Test fun aDifferentAppWrongVersionOrDowngradeIsRejected() {
        assertThrows(IOException::class.java) { verify(pkg = "sh.zeron.android") }
        assertThrows(IOException::class.java) { verify(code = 3003) }
        assertThrows(IOException::class.java) { verify(name = "0.3.3") }
        assertThrows(IOException::class.java) { verify(name = null) }
        assertThrows(IOException::class.java) { verify(installed = 3002) }
        assertThrows(IOException::class.java) { verify(installed = 4000) }
    }

    @Test fun unsignedDebugAndAdditionalSignerApksAreRejected() {
        assertThrows(IOException::class.java) { verify(signers = emptyList()) }
        assertThrows(IOException::class.java) { verify(signers = listOf("d".repeat(64))) }
        assertThrows(IOException::class.java) { verify(signers = listOf(signer, "d".repeat(64))) }
        verify(signers = listOf(signer.uppercase()))
    }
}
