# Reliable remote sends

A remote message crosses three boundaries: publishing the chat registry row,
storing the session command, and waking the selected execution host. A visible
chat does not imply that the host has received its command. Sending and Queued
remain distinct from Working until the host owns the turn or streams output.
Provider startup, model discovery, and network latency can still delay output.

## Client and desktop recovery

Thin clients persist the registry row before sending session writes. A versioned
`viewer-delivery` job tracks the outstanding host wake. Discovery resumes rooms
with pending delivery jobs or outbox rows without requiring the user to open the
chat. Desktop senders persist equivalent `remote-wake` jobs and resume them after
workspace restoration. Wake failures retry with fresh credentials and bounded
backoff; a stalled destination yields capacity after a 12-second service window.

A sender removes its delivery receipt only when:

1. outgoing document rows were acknowledged before the final wake request;
2. the edge accepted that wake successfully;
3. the outbox remains empty and the captured job version still matches.

A wake that precedes publication can reach an empty room. The final post-ACK wake
closes this race. Version checks prevent an old request from clearing a newer
send. Recovery retains existing command IDs and host deduplication rather than
creating duplicate turns. Deleting a chat also removes its local delivery jobs.

## Edge handoff

Senders include the selected `hostDevice` on WebSocket joins and HTTPS pushes.
The chat room stores opaque command rows, a versioned host-wake receipt, and its
next alarm in one SQLite transaction before acknowledging the rows. It forwards
the wake to the device room; failed forwards retain the receipt and retry from
durable alarms, with backoff capped at one minute.

The Worker obtains the chat ID from the authenticated route and forwards the
verified owner identity. Host-authored output does not create wake loops. A
newer receipt prevents stale forwards from clearing pending work. Wake retries
share the existing durable alarm with backups, preserving the backup schedule.
The edge never parses session documents or executes providers.

After the edge accepts all command bytes, the phone can leave delivery to the
chat room. The device room then owns delivery to the host. Client receipts
remain a recovery path and preserve compatibility with older edge deployments.

## Mobile suspension and deployment

On iOS, pending or recent sends request up to 25 seconds of background execution,
ending when the app returns to the foreground, signs out, or iOS expires the
request. Force-quitting or losing connectivity before all command bytes reach
the edge requires reopening the app. Persisted receipts allow delivery to resume.
The composer shows “Sending to host…” while the host has not adopted the send.

Deploy the updated edge with the updated senders; deploying the edge first is
safe and requires no new bindings. Older edge deployments continue to use the
sender's wake and relaunch recovery, but cannot take ownership of the handoff
while a phone remains suspended.
