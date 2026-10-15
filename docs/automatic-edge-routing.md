# Automatic edge routing

Zerun automatically chooses between `https://edge.550w.host` and
`https://edge.puqing.work`. Both entrances serve the same backend and account
data. There is no route setting to configure.

Desktop, iOS/iPadOS and Android share the Rust router in `zeron-sync`. It covers
authentication, document and workspace synchronization, device relay connections,
preview signaling, and desktop updates. Android's native updater uses the same
route selection through the mobile bindings.

On the first request, both entrances are probed concurrently using unauthenticated
`GET /health` requests. The first valid response wins; each probe has a three
second deadline. TLS verification remains enabled. Only the two production
origins participate; development and custom endpoints retain their configured
destination. A local `network-route.json` hint remembers the last working
entrance. It contains no account information or credentials.

Later requests trigger a background check when measurements are at least a
minute old. OS network-path events and detected system resume invalidate the
measurements. Healthy established sockets stay connected. A faster entrance must
win two successive checks by at least 100 ms and 30%, with at least two minutes
between performance-driven changes. Transport failure bypasses that hold and
puts the failed entrance on a 30 second cooldown. Failures from older connections
cannot switch traffic back to an entrance still on cooldown.

WebSocket upgrades try the entrances sequentially, with at most eight seconds
per entrance and twenty seconds overall. HTTP GET/HEAD requests may retry once
on the alternate entrance after transport failure or HTTP 408/502/503/504.
HTTP 401/403/429 do not cause a route change. Authentication code exchange,
token refresh and other mutations are sent only once by the router; the existing
single-flight refresh and durable delivery protocols retain ownership of retries.
Document reconnects retain their batch IDs, outbox and synchronization cursors.
Uncertain device RPC mutations are not automatically replayed.

Interrupted update downloads can restart on the alternate entrance using a clean
temporary file. Downloads retain inactivity timeouts rather than a total transfer
deadline, and retain their existing checksum and package identity checks.

The relay must proxy HTTP and WebSocket traffic to the same backend, preserve
authentication and query parameters, and keep verified HTTPS on both hops. Adding
an entrance to the allowlist requires a client release; DNS failover alone does
not change a client's selected origin.
