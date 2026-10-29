package sh.zeron.android.core

import java.util.concurrent.Executor

/**
 * Main-thread-owned scheduler. Only clientRevision/build run on [background];
 * Compose state and client ownership are read and written on [main]. Bursts
 * retain one latest request, including a replacement client during sign-in.
 */
internal class SnapshotScheduler<C : Any, S : Any>(
    private val background: Executor,
    private val main: Executor,
    private val currentClient: () -> C?,
    private val published: () -> S?,
    private val setPublished: (S) -> Unit,
    private val revisionOf: (S) -> ULong,
    private val clientRevision: (C) -> ULong,
    private val build: (C) -> S,
) {
    private data class Request<C>(val client: C, val onPublished: () -> Unit)
    private var publishedBy: C? = null
    private var running = false
    private var pending: Request<C>? = null

    /** Called on main. A newer client replaces, rather than loses to, queued work. */
    fun schedule(client: C, onPublished: () -> Unit) {
        pending = Request(client, onPublished)
        drain()
    }

    private fun drain() {
        if (running) return
        val request = pending ?: return
        pending = null
        if (request.client !== currentClient()) return
        val knownRevision = published()?.takeIf { publishedBy === request.client }?.let(revisionOf)
        running = true
        background.execute {
            val snapshot = runCatching {
                if (knownRevision != null && clientRevision(request.client) == knownRevision) null
                else build(request.client)
            }.getOrNull()
            main.execute {
                running = false
                if (snapshot != null && tryPublish(request.client, snapshot)) request.onPublished()
                drain()
            }
        }
    }

    /** Explicit callers need their mutation reflected before this returns. */
    fun refresh(client: C) {
        val current = published()
        if (current != null && publishedBy === client && clientRevision(client) == revisionOf(current)) return
        tryPublish(client, build(client))
    }

    private fun tryPublish(client: C, snapshot: S): Boolean {
        if (client !== currentClient()) return false
        val current = published()
        if (current != null && publishedBy === client && revisionOf(snapshot) <= revisionOf(current)) return false
        publishedBy = client
        setPublished(snapshot)
        return true
    }
}
