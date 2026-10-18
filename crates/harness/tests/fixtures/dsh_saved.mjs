// Opt-in fixture: uses installed native modules, isolated history, no profile boot.
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';
const [home, action, compression = 'zstd', copyID] = process.argv.slice(2);
const require = createRequire(join(home, 'profiles', 'package.json'));
const load = async name => import(pathToFileURL(require.resolve(name)).href);
const [{ Context }, { default: Persistence }, { Session, SESSION_FORMAT_VERSION }, projections] = await Promise.all([
  load('@deepseek-ai/cordis'), load('@deepseek-ai/dsh-session-persistence-jsonl'), load('@deepseek-ai/dsh-session'), load('@deepseek-ai/dsh-session-format-catalog/message-projections'),
]);
const ctx = new Context();
const persistence = new Persistence(ctx, { root: join(home, 'sessions'), compression });
try {
  if (action === 'create') {
    for (const busy of [false, true]) {
      const header = { version: SESSION_FORMAT_VERSION, id: busy ? 'session-busy' : 'session-source', createdAt: 1000, cwd: home, isSeeded: false, delegationDepth: 0 };
      const session = Session.create(header.id, [], header);
      session.append('turn/start', { turn: 1 });
      session.append('step/start', { turn: 1, step: 1 });
      session.append('request/context', { provider:'fixture', model:'test' });
      session.append('user/message', { role: 'user', id: 'u', source: { kind: 'user' }, content: [{ type: 'text', text: 'remember teal' }] }, { surfaceOp: 'append' });
      if (!busy) {
        session.append('assistant/message', { turn: 1, step: 1, message: { role: 'assistant', id: 'tool-message', source: { kind: 'model', provider: 'fixture', model: 'test' }, content: [{ type: 'tool-call', id: 'call', name: 'bash', arguments: '{"command":"pwd"}' }] }, stream: [] }, { surfaceOp: 'append' });
        session.append('tool/call', { turn: 1, step: 1, callId: 'call', name: 'bash', arguments: '{"command":"pwd"}' });
        session.append('tool/result', { turn: 1, step: 1, message: { role: 'tool', id: 'r', source: { kind:'tool', callId:'call' }, toolCallId: 'call', content: [{ type: 'text', text: '/project' }] } }, { surfaceOp: 'append' });
        session.append('assistant/message', { turn: 1, step: 1, message: { role: 'assistant', id: 'a', source: { kind: 'model', provider: 'fixture', model: 'test' }, content: [{ type: 'text', text: 'Native answer' }] }, stream: [] }, { surfaceOp: 'append' });
        session.append('step/end', { turn: 1, step: 1 });
        session.append('turn/end', { turn: 1, reason: { kind: 'completed' } });
      }
      const handle = await persistence.create(header);
      try { await handle.append(session.snapshotEvents()); await handle.flush(); } finally { await handle.close(); }
    }
  } else {
    const handle = await persistence.open(copyID, 'write');
    try {
      const { events } = await handle.read();
      const session = Session.fromRestore(handle.id, events, handle.header, handle.inheritedEventCount, 'shared-frozen', projections.currentSessionMessageProjections);
      if (handle.header.parentSession !== undefined || handle.header.id === 'session-source') throw new Error('copy is not independent');
      if (session.ownEvents().length !== 1) throw new Error('original events incorrectly owned by copy');
      const from = events.length;
      session.append('turn/start', { turn: 2 });
      session.append('turn/end', { turn: 2, reason: { kind: 'completed' } });
      await handle.append(session.snapshotEvents(from)); await handle.flush();
      if (!JSON.stringify(session.snapshotEvents()).includes('Native answer')) throw new Error('native context lost');
    } finally { await handle.close(); }
  }
} finally { await ctx.fiber.dispose(); }
