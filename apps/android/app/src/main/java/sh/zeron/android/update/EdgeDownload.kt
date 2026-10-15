package sh.zeron.android.update

import uniffi.zeron_core.reportEdgeTransportFailure
import uniffi.zeron_core.resolveProductionEdgeUrl
import java.io.IOException
import javax.net.ssl.HttpsURLConnection

/** Parsing, storage and package validation errors must not affect route health. */
internal class EdgeTransportFailure(cause: IOException) : IOException(cause.message, cause)

internal inline fun <T> edgeNetwork(block: () -> T): T = try {
    block()
} catch (e: IOException) {
    throw EdgeTransportFailure(e)
}

/** Safe GETs only. The native updater shares the Rust router's selection. */
internal suspend fun <T> withEdgeRoute(
    path: String,
    resolveOrigin: suspend () -> String = ::resolveProductionEdgeUrl,
    reportFailure: (String) -> Unit = ::reportEdgeTransportFailure,
    openConnection: (String, String) -> HttpsURLConnection,
    block: suspend (HttpsURLConnection) -> T,
): T {
    var origin = resolveOrigin()
    repeat(2) { attempt ->
        val connection = openConnection(origin, path)
        try {
            val status = edgeNetwork { connection.responseCode }
            if (status in setOf(408, 502, 503, 504)) {
                throw EdgeTransportFailure(IOException("Release source returned HTTP $status."))
            }
            if (status != 200) throw IOException("Release source returned HTTP $status.")
            return block(connection)
        } catch (e: EdgeTransportFailure) {
            reportFailure(connection.url.toString())
            if (attempt == 1) throw e
            val next = resolveOrigin()
            if (next == origin) throw e
            origin = next
        } finally {
            connection.disconnect()
        }
    }
    error("Bounded route attempts exhausted")
}
