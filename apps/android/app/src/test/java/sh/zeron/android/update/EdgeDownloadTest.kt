package sh.zeron.android.update

import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import java.io.IOException
import java.net.URL
import java.security.cert.Certificate
import javax.net.ssl.HttpsURLConnection

class EdgeDownloadTest {
    private class Connection(origin: String, private val status: Int) : HttpsURLConnection(URL("$origin/releases/manifest.json")) {
        var closed = false
        override fun getResponseCode() = status
        override fun disconnect() { closed = true }
        override fun usingProxy() = false
        override fun connect() {}
        override fun getCipherSuite() = "test"
        override fun getLocalCertificates(): Array<Certificate>? = null
        override fun getServerCertificates(): Array<Certificate> = emptyArray()
    }

    private val direct = "https://direct.example"
    private val relay = "https://relay.example"

    @Test fun gatewayFailureSwitchesOnceAndClosesBothConnections() = runBlocking {
        var selected = direct
        val opened = mutableListOf<Connection>()
        val failures = mutableListOf<String>()
        val result = withEdgeRoute("manifest.json", { selected }, {
            failures += it
            selected = relay
        }, { origin, _ -> Connection(origin, if (origin == direct) 503 else 200).also { opened += it } }) {
            "verified response"
        }
        assertEquals("verified response", result)
        assertEquals(listOf("$direct/releases/manifest.json"), failures)
        assertEquals(listOf(direct, relay), opened.map { "https://${it.url.host}" })
        assertTrue(opened.all { it.closed })
    }

    @Test fun interruptedBodyRestartsOnAlternateEntrance() = runBlocking {
        var selected = direct
        var attempts = 0
        val result = withEdgeRoute("package.apk", { selected }, { selected = relay },
            { origin, _ -> Connection(origin, 200) }) { connection ->
            attempts++
            edgeNetwork {
                if (connection.url.host == "direct.example") throw IOException("mid-body reset")
                "complete replacement bytes"
            }
        }
        assertEquals("complete replacement bytes", result)
        assertEquals(2, attempts)
    }

    @Test fun authRateLimitAndValidationFailuresDoNotRetry() = runBlocking {
        for (status in listOf(401, 403, 404, 429, 200)) {
            var opened = 0
            var reported = 0
            try {
                withEdgeRoute("package.apk", { direct }, { reported++ }, { origin, _ ->
                    opened++
                    Connection(origin, status)
                }) { throw IOException("checksum or package identity mismatch") }
                fail("failure should reach the caller")
            } catch (_: IOException) { }
            assertEquals(1, opened)
            assertEquals(0, reported)
        }
    }

    @Test fun failureOfBothEntrancesStopsAfterTwoAttempts() = runBlocking {
        var selected = direct
        var opened = 0
        var reported = 0
        try {
            withEdgeRoute("manifest.json", { selected }, {
                reported++
                selected = if (selected == direct) relay else direct
            }, { origin, _ ->
                opened++
                Connection(origin, 502)
            }) { fail("failed gateway must not return a body") }
            fail("failure should reach the caller")
        } catch (_: IOException) { }
        assertEquals(2, opened)
        assertEquals(2, reported)
    }
}
