// Native persistence only: no profile boot, credentials, model call, or migration
// publication. The installed dsh packages own decompression and format decoding.
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
import { readdir, realpath } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';

const MAX_SOURCE = 64 * 1024 * 1024;
const MAX_HISTORY = 16 * 1024 * 1024;
let input = '';
for await (const chunk of process.stdin) { input += chunk; if (input.length > 65536) throw new Error('Invalid import request'); }
const request = JSON.parse(input);
let require;
for (const base of [join(request.home, 'profiles', request.profile, 'package.json'), join(request.home, 'profiles', 'package.json')]) {
  const candidate = createRequire(base);
  try { candidate.resolve('@deepseek-ai/dsh-session-persistence-jsonl'); require = candidate; break; } catch {}
}
if (!require) throw new Error('Install the dsh profile packages before importing saved sessions.');
const load = async name => import(pathToFileURL(require.resolve(name)).href);
const [{ Context }, { default: Persistence }, { Session }, { buildForkSeed }, projections] = await Promise.all([
  load('@deepseek-ai/cordis'), load('@deepseek-ai/dsh-session-persistence-jsonl'),
  load('@deepseek-ai/dsh-session'), load('@deepseek-ai/dsh-session/fork'),
  load('@deepseek-ai/dsh-session-format-catalog/message-projections'),
]);
const root = await realpath(request.root);
let compression;
// Determine the store's physical encoding without opening any conversation.
let count = 0;
for (const project of await readdir(root, { withFileTypes: true })) {
  if (!project.isDirectory()) continue;
  for (const session of await readdir(join(root, project.name), { withFileTypes: true })) {
    if (!session.isDirectory()) continue;
    if (++count > 20000) throw new Error('dsh discovery exceeded 20000 sessions.');
    for (const file of await readdir(join(root, project.name, session.name), { withFileTypes: true })) {
      if (!file.isFile() || !/\.jsonl(?:\.zstd)?$/.test(file.name)) continue;
      const encoding = file.name.endsWith('.zstd') ? 'zstd' : 'none';
      if (compression && compression !== encoding) throw new Error('dsh store has mixed physical encodings.');
      compression = encoding;
    }
  }
}
const ctx = new Context();
const persistence = new Persistence(ctx, { root, compression: compression ?? 'zstd' });
const restored = (handle, events) => Session.fromRestore(handle.id, events, handle.header, handle.inheritedEventCount, 'shared-frozen', projections.currentSessionMessageProjections);
const text = content => typeof content === 'string' ? content : Array.isArray(content) ? content.map(p => p.text ?? '').filter(Boolean).join('\n') : '';
function running(events) {
  const turns = new Set();
  for (const event of events) {
    if (event.type === 'turn/start') turns.add(event.data.turn);
    if (event.type === 'turn/end') turns.delete(event.data.turn);
  }
  return turns.size > 0;
}
function metadata(handle, events) {
  const header = handle.header;
  let title = '';
  for (const event of events) {
    if (event.type === 'session/title') title = event.data.title;
    else if (!title && event.type === 'user/message') title = text(event.data.content).slice(0, 160);
  }
  const context = restored(handle, events).requestContext();
  return {
    harness: 'dsh', native_id: header.id, store_id: request.identity,
    title: title || 'Untitled session', cwd: header.cwd ?? '',
    created_at_ms: header.createdAt, updated_at_ms: events.at(-1)?.time ?? header.createdAt,
    // dsh ACP advertises provider-scoped model choices as JSON tuples.
    model: context ? JSON.stringify([context.provider, context.model]) : null,
    locator: null, running: running(events),
  };
}
async function open(id) {
  const snapshot = await persistence.stat(id);
  if (!snapshot || snapshot.sizeBytes > MAX_SOURCE) throw new Error('dsh session is missing or exceeds the import limit.');
  const handle = await persistence.open(id, 'read');
  try {
    const { events } = await handle.read();
    if (Buffer.byteLength(JSON.stringify(events)) > MAX_SOURCE) throw new Error('dsh decoded history exceeds the import limit.');
    return { handle, events };
  } catch (error) { await handle.close(); throw error; }
}
function history(handle, events) {
  // Interpret message projections with the native catalog, while retaining
  // append-origin messages that a later compaction removed from model context.
  const session = restored(handle, events);
  const messages = []; const tools = new Map(); let notice;
  for (const event of events) {
    if (event.type.includes('compact') || (event.surfaceOp && event.surfaceOp !== 'append')) notice = 'dsh compacted or revised context; all native records remain in the copy.';
    if (event.type === 'tool/call') {
      let args; try { args = JSON.parse(event.data.arguments); } catch { args = event.data.arguments; }
      const part = { Tool: { call: { kind: 'unknown', name: event.data.name, input: args }, output: null, is_error: false } };
      tools.set(event.data.callId, part.Tool);
      messages.push({ id: `event-${event.seq}`, role: 'Assistant', timestamp_ms: event.time, parts: [part] });
      continue;
    }
    if (event.type === 'tool/result') {
      const result = event.data.message; const tool = tools.get(result.toolCallId);
      if (tool) { tool.output = text(result.content); tool.is_error = result.isError === true || event.data.error !== undefined; }
      continue;
    }
    if (!['user/message', 'assistant/message'].includes(event.type) || (event.surfaceOp && event.surfaceOp !== 'append')) continue;
    const message = session.deriveEventMessage(event); if (!message) continue;
    const parts = [];
    if (typeof message.content === 'string') parts.push({ Text: message.content });
    else for (const part of message.content ?? []) {
      if (part.type === 'text') parts.push({ Text: part.text });
      else if (part.type === 'reasoning' || part.type === 'thinking') parts.push({ Reasoning: part.text ?? part.thinking ?? '' });
      else if (part.type !== 'tool-call') notice = 'Attachments and unsupported blocks remain in dsh’s native copy and are omitted from the imported transcript.';
    }
    if (parts.length) messages.push({ id: `event-${event.seq}`, role: event.type === 'user/message' ? 'User' : 'Assistant', timestamp_ms: event.time, parts });
  }
  const result = { messages, notice: notice ?? null };
  if (messages.length > 20000 || Buffer.byteLength(JSON.stringify(result)) > MAX_HISTORY) throw new Error('dsh display history exceeds the import limit.');
  return result;
}
try {
  let result;
  if (request.action === 'list') {
    const all = (await persistence.list()).filter(s => s.header.origin !== 'subagent' && !s.header.parentSession).sort((a,b) => b.header.createdAt - a.header.createdAt || a.header.id.localeCompare(b.header.id));
    const offset = request.cursor === null ? 0 : Number(request.cursor);
    if (!Number.isSafeInteger(offset) || offset < 0 || offset > all.length) throw new Error('Invalid dsh history cursor.');
    const sessions = [];
    for (const snapshot of all.slice(offset, offset + 50)) {
      const { handle, events } = await open(snapshot.header.id);
      try { sessions.push(metadata(handle, events)); } finally { await handle.close(); }
    }
    result = { sessions, next_cursor: offset + 50 < all.length ? String(offset + 50) : null };
  } else {
    const { handle, events } = await open(request.session.native_id);
    try {
      if (handle.header.cwd !== request.session.cwd || handle.header.origin === 'subagent' || handle.header.parentSession) throw new Error('dsh source identity or working directory changed. Scan again.');
      if (request.action === 'history') result = history(handle, events);
      else if (request.action === 'copy') {
        if (running(events)) throw new Error('This session is still running. Finish or stop its current turn, then scan again.');
        if (!events.length || !history(handle, events).messages.length) throw new Error('dsh session has no conversation to copy.');
        // Mark the entire source as inherited. No original child ownership is
        // adopted by the independent top-level copy (ACP requires no parent).
        const seed = buildForkSeed(events, events.length - 1);
        const header = { ...handle.header, id: `session-${randomUUID()}`, createdAt: Date.now(), isSeeded: true };
        delete header.parentSession; delete header.origin;
        const clone = Session.create(header.id, seed, header, events.length, projections.currentSessionMessageProjections);
        const destination = await persistence.create(header, { inheritedEventCount: events.length });
        try { await destination.append(clone.snapshotEvents()); await destination.flush(); result = metadata(destination, clone.snapshotEvents()); }
        finally { await destination.close(); }
      } else throw new Error('Unknown dsh import operation.');
    } finally { await handle.close(); }
  }
  process.stdout.write(JSON.stringify(result));
} finally { await ctx.fiber.dispose(); }
