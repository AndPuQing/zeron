# Mobile relay support

The relay protocol is implemented by `crates/client` and `crates/sync`.
Installed iOS and Android builds advertise relay support by default, and
the edge offers it to all users with `CHAT_RELAY_LAYER=1`. See
[relay-layer.md](relay-layer.md) for negotiation and compatibility. A chat
stays on stored logs while any required participant lacks relay support.

## Local development

A debug iOS build accepts `-no-relay-layer`; a debuggable Android build
accepts `--ez no-relay-layer true`. These switches simulate an older client
without relay support. The room must also enable the protocol; advertising
client support alone cannot migrate a chat. Use an isolated profile and a
development edge when exercising negotiation.

## Platform responsibilities

The shared client owns the durable inbox, held sends, delivery receipts,
checkpoint fetching and recovery. Platform code reports:

- Foreground and background transitions, so local changes are saved and stale
  connections are retried when the app returns.
- Network availability and path changes, so a lost route parks dials instead
  of repeatedly using the radio.
- Whether a session view is attached, which controls checkpoint fetching and
  prioritizes the chat on screen.

Android's delivery service may keep the process alive after a background
transition. Neither platform should create a foreground service or periodic
save timer solely to keep chat relay sockets alive. Process death may arrive
without a lifecycle callback; the durable inbox and outbox provide recovery.

## Empty transcript and delivery states

`ComposerState.transcript_load` distinguishes ready, syncing, reconnecting,
offline and host-away states. Existing local messages stay visible. A chat
never opened on this device can remain unavailable until its host returns.
Warning states do not show a spinner when nothing can make progress.

A saved turn is interruptible only when both `turn_running` and
`can_interrupt` are true. A pending send waiting for a network path or host
does not age into a delivery failure merely because that path is absent.

## Durability and compatibility

Local sends enter the outbox before transmission. Relay frames enter the
chat inbox and remain until a snapshot covers them. Opening a chat restores
its snapshot, outbox and inbox before networking. An acknowledged send is
saved before its outbox row is retired.

A room with relay disabled, an older host, or an account containing a client
without relay support keeps the stored-log path. Losing host capability or
changing generation switches subsequent sends back to stored rows. Subagent
transcripts and terminals remain host RPCs with their own connection paths.
