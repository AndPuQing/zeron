# Chat relay protocol

The generation-3 relay implementation is available for development. It remains
disabled by default in this fork: `ClientConfig::relay_layer` is false and
`CHAT_RELAY_LAYER` is empty. Mobile builds also require explicit debug opt-in.
Existing clients continue to use stored logs.

## Roles and negotiation

The host owns execution and the canonical chat document. Viewers keep their
own local copies. The edge room forwards live frames, retains queued sends
and serves checkpoints; it does not execute agent work.

A client announces relay support to the registry and each chat room. A host
may migrate a chat only when the room, host and known account clients support
the layer. The chat generation decides the transport, rather than the version
of whichever client opens it. An old client or host holds the chat on stored
rows; loss of support returns subsequent sends to that path.

Generation changes must preserve operations already relayed. A stored row
that depends on relayed operations cannot be treated as a complete history
on its own. Host reconciliation and checkpoints cover those dependencies
before ordinary readers are allowed to proceed.

## Local durability

A send enters the local outbox before transmission. The room's receipt and a
local save retire it only after the document durably holds it. Viewers journal
received frames in `chat_inbox`; a save retires only rows covered by that save's
export. Startup loads the snapshot, pending outbox and inbox before opening
network connections. Deleting a snapshot clears its inbox as well.

The room's queue lets a viewer send while the host is absent. The host takes
each queued operation once and publishes the resulting document changes.
Waiting for a host or network path is distinct from a failed send. Explicit
removal cancels a held send, including one whose upload is still in flight.

## Catch-up and checkpoints

Peers exchange document versions to find missing operations. Transfers are
resumable and bounded. The current implementation uses the room for chat
traffic; terminal connections retain their separate direct path.

A checkpoint is a durable compacted document plus its cursor. Clients share
an in-flight checkpoint fetch rather than download it once for bootstrap and
again for a socket. Only attached views fetch checkpoints while the host is
away, and a checkpoint that cannot advance the local document is skipped.
The Rust host uploads large checkpoints through the chunked checkpoint API.

The edge remembers that a room has carried relayed operations across socket
lifetimes. Losing a socket therefore does not silently downgrade a partially
relayed document into an incomplete stored-log history.

## Lifecycle and limits

Network changes close routes that are no longer usable. Foregrounding an app
refreshes stale connections; backgrounding saves local changes. Suspended
phones do not keep sockets alive and recover through version exchange after
resuming. A chat never stored on a viewer still needs a reachable host or a
usable checkpoint. Subagent transcripts are host RPCs, not offline replicas.

See [relay-mobile.md](relay-mobile.md) for the mobile lifecycle contract and
[cloud-attachments.md](cloud-attachments.md) for staged attachment delivery.
