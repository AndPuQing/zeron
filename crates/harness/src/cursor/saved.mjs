// This file is appended to the pinned shim. It uses public LocalAgentStore
// APIs, preserving opaque checkpoint bytes and SDK metadata unchanged.
async function savedSessionsMain(input, sdk) {
  const MAX_SOURCE = 64 * 1024 * 1024,
    MAX_DISPLAY = 16 * 1024 * 1024;
  const root = fs.realpathSync(input.root),
    stateRoot = path.resolve(input.stateRoot);
  const fail = (message) => {
    throw new Error(message);
  };
  const exists = (file) => {
    try {
      return fs.lstatSync(file);
    } catch (e) {
      if (e.code === "ENOENT") return null;
      throw e;
    }
  };
  const inside = (base, candidate) => {
    const r = path.relative(base, candidate);
    return (
      r === "" ||
      (!r.startsWith(`..${path.sep}`) && r !== ".." && !path.isAbsolute(r))
    );
  };
  const safe = (value) =>
    typeof value === "string" &&
    value.length > 0 &&
    !/[\\/]/.test(value) &&
    value !== "." &&
    value !== "..";
  const stores = [];
  function dirs(directory) {
    const stat = exists(directory);
    if (!stat) return [];
    if (!stat.isDirectory() || stat.isSymbolicLink())
      fail("Cursor history must not traverse symbolic links.");
    return fs
      .readdirSync(directory, { withFileTypes: true })
      .filter((e) => e.isDirectory() && !e.isSymbolicLink())
      .map((e) => path.join(directory, e.name));
  }
  for (const project of dirs(path.join(root, "projects"))) {
    for (const directory of dirs(path.join(project, "sdk-agent-store"))) {
      if (exists(path.join(directory, "index.db")))
        stores.push({ directory, kind: "sqlite" });
    }
  }
  for (const directory of dirs(path.join(stateRoot, "agents"))) {
    if (exists(path.join(directory, "agents.ndjson")))
      stores.push({ directory, kind: "jsonl" });
  }
  if (stores.length > 5000) fail("Cursor history exceeds the scan limit.");
  const toSession = (document, directory, runs) => ({
    harness: "cursor",
    native_id: document.agentId,
    store_id: input.identity,
    title: document.name || "Cursor session",
    cwd: document.cwd,
    created_at_ms: document.createdAt,
    updated_at_ms: document.updatedAt,
    model: runs.find((r) => r.model)?.model?.id ?? null,
    locator: directory,
    running:
      document.status === "running" ||
      !!document.activeRunId ||
      runs.some((r) => ["running", "queued"].includes(r.status)),
  });
  const all = async (list, filter = {}) => {
    const rows = [],
      seen = new Set();
    let cursor;
    do {
      const page = await list({ filter: { ...filter, limit: 100, cursor } });
      rows.push(...page.items);
      if (rows.length > 20000)
        fail("Cursor native history exceeds the record limit.");
      cursor = page.nextCursor;
      if (cursor && seen.has(cursor))
        fail("Cursor returned a repeated history cursor.");
      seen.add(cursor);
    } while (cursor);
    return rows;
  };
  // Snapshot first: SDK schema initialization/migrations only touch this private
  // temporary tree. Compare every original file again before accepting it.
  async function openSource(entry) {
    const temporary = fs.mkdtempSync(
      path.join(input.scratchRoot, "cursor-history-"),
    );
    let bytes = 0;
    const originals = [];
    function copyTree(source, dest, depth = 0) {
      if (depth > 12) fail("Cursor store nesting exceeds the import limit.");
      fs.mkdirSync(dest, { recursive: true, mode: 0o700 });
      for (const child of fs.readdirSync(source, { withFileTypes: true })) {
        const from = path.join(source, child.name),
          to = path.join(dest, child.name);
        if (child.isSymbolicLink() || (!child.isDirectory() && !child.isFile()))
          fail("Cursor history contains an unsupported file.");
        if (child.isDirectory()) copyTree(from, to, depth + 1);
        else {
          const stat = fs.statSync(from);
          bytes += stat.size;
          if (bytes > MAX_SOURCE || originals.length >= 20000)
            fail("Cursor native store exceeds the import limit.");
          const data = fs.readFileSync(from);
          originals.push([
            from,
            crypto.createHash("sha256").update(data).digest("hex"),
          ]);
          fs.writeFileSync(to, data, { mode: 0o600 });
        }
      }
    }
    let store;
    function checkUnchanged() {
      const current = [];
      let total = 0;
      function scan(directory, depth = 0) {
        if (depth > 12) fail("Cursor store nesting exceeds the import limit.");
        for (const child of fs.readdirSync(directory, {
          withFileTypes: true,
        })) {
          const file = path.join(directory, child.name);
          if (
            child.isSymbolicLink() ||
            (!child.isDirectory() && !child.isFile())
          )
            fail("Cursor history contains an unsupported file.");
          if (child.isDirectory()) scan(file, depth + 1);
          else {
            total += fs.statSync(file).size;
            if (total > MAX_SOURCE || current.length >= 20000)
              fail("Cursor native store exceeds the import limit.");
            current.push([
              file,
              crypto
                .createHash("sha256")
                .update(fs.readFileSync(file))
                .digest("hex"),
            ]);
          }
        }
      }
      scan(entry.directory);
      const sort = (rows) => rows.sort((a, b) => a[0].localeCompare(b[0]));
      if (
        JSON.stringify(sort(current)) !== JSON.stringify(sort([...originals]))
      )
        fail(
          "Cursor history changed while copying. Finish its turn and scan again.",
        );
    }
    try {
      copyTree(entry.directory, temporary);
      checkUnchanged();
      if (entry.kind === "jsonl")
        store = new sdk.JsonlLocalAgentStore(temporary);
      else {
        const { SqliteLocalAgentStore } = await import("@cursor/sdk/sqlite");
        store = await SqliteLocalAgentStore.open({
          workspaceRef: "",
          stateRoot: temporary,
        });
      }
      return {
        store,
        checkUnchanged,
        close: async () => {
          await store.dispose?.();
          fs.rmSync(temporary, { recursive: true, force: true });
        },
      };
    } catch (e) {
      await store?.dispose?.();
      fs.rmSync(temporary, { recursive: true, force: true });
      throw e;
    }
  }
  if (input.action === "list") {
    const rows = [];
    for (const entry of stores) {
      const source = await openSource(entry);
      try {
        for (const doc of await all((p) => source.store.agents.list(p))) {
          if (!safe(doc.agentId)) fail("Invalid Cursor agent id.");
          const runs = await all((p) => source.store.runs.list(p), {
            agentIds: [doc.agentId],
          });
          if (doc.latestCheckpoint)
            rows.push(toSession(doc, entry.directory, runs));
        }
      } finally {
        await source.close();
      }
    }
    rows.sort(
      (a, b) =>
        b.updated_at_ms - a.updated_at_ms ||
        a.native_id.localeCompare(b.native_id),
    );
    const offset = input.cursor == null ? 0 : Number(input.cursor);
    if (!Number.isSafeInteger(offset) || offset < 0 || offset > rows.length)
      fail("Invalid Cursor history cursor.");
    const end = Math.min(offset + 50, rows.length);
    return {
      sessions: rows.slice(offset, end),
      next_cursor: end < rows.length ? String(end) : null,
    };
  }
  const session = input.session;
  if (
    !session ||
    !safe(session.native_id) ||
    session.store_id !== input.identity ||
    session.harness !== "cursor"
  )
    fail("Invalid Cursor history reference.");
  const entry = stores.find((s) => s.directory === session.locator);
  if (
    !entry ||
    (!inside(root, entry.directory) && !inside(stateRoot, entry.directory))
  )
    fail("The Cursor history location changed. Scan again.");
  const source = await openSource(entry);
  try {
    const store = source.store,
      doc = await store.agents.get({ agentId: session.native_id });
    if (!doc || doc.cwd !== session.cwd || !doc.latestCheckpoint)
      fail("The Cursor conversation checkpoint is unavailable.");
    const runs = await all((p) => store.runs.list(p), {
      agentIds: [doc.agentId],
    });
    if (input.action === "copy") {
      if (toSession(doc, entry.directory, runs).running)
        fail(
          "This Cursor session is still running. Finish its turn and scan again.",
        );
      const id = `agent-${crypto.randomUUID()}`,
        directory = path.join(stateRoot, "agents", crypto.randomUUID());
      const target = new sdk.JsonlLocalAgentStore(directory);
      let byteCount = 0;
      try {
        for (const blobId of await all((p) => store.checkpoints.list(p), {
          agentIds: [doc.agentId],
        })) {
          const data = await store.checkpoints.get({
            agentId: doc.agentId,
            blobId,
          });
          if (!data) fail("Cursor checkpoint blob is missing.");
          byteCount += data.byteLength;
          if (byteCount > MAX_SOURCE)
            fail("Cursor checkpoint exceeds the import limit.");
          await target.checkpoints.create({ agentId: id, blobId, data });
        }
        for (const run of runs) {
          const runId = crypto.randomUUID();
          await target.runs.create({ run: { ...run, agentId: id, runId } });
          let afterOffset;
          const seen = new Set();
          do {
            const page = await store.runEvents.list({
              runId: run.runId,
              limit: 100,
              afterOffset,
            });
            for (const event of page.items) {
              byteCount += Buffer.byteLength(JSON.stringify(event));
              if (byteCount > MAX_SOURCE)
                fail("Cursor run history exceeds the import limit.");
              await target.runEvents.append({ ...event, runId });
            }
            afterOffset = page.nextOffset;
            if (afterOffset && seen.has(afterOffset))
              fail("Cursor returned a repeated event cursor.");
            seen.add(afterOffset);
          } while (afterOffset);
        }
        const copied = await target.agents.create({
          agent: { ...doc, agentId: id, activeRunId: null },
        });
        source.checkUnchanged();
        const markers = path.join(stateRoot, "by-agent");
        fs.mkdirSync(markers, { recursive: true, mode: 0o700 });
        fs.writeFileSync(path.join(markers, id), directory, {
          flag: "wx",
          mode: 0o600,
        });
        return toSession(copied, directory, runs);
      } catch (e) {
        fs.rmSync(directory, { recursive: true, force: true });
        throw e;
      }
    }
    if (input.action !== "history") fail("Unknown Cursor import operation.");
    const messages = [];
    let offset = 0,
      bytes = 0,
      notice = null;
    const push = (role, parts) => {
      if (!parts.length) return;
      const message = {
        id: `cursor-${messages.length}`,
        role,
        timestamp_ms: doc.createdAt,
        parts,
      };
      bytes += Buffer.byteLength(JSON.stringify(message));
      if (bytes > MAX_DISPLAY || messages.length >= 20000)
        fail(
          "Cursor display history exceeds the import limit; the native session was not truncated.",
        );
      messages.push(message);
    };
    while (true) {
      const page = await sdk.Agent.messages.list(doc.agentId, {
        runtime: "local",
        cwd: doc.cwd,
        store,
        offset,
        limit: 100,
      });
      for (const row of page) {
        const turn = row.message?.turn;
        const conversation =
          turn?.case === "agentConversationTurn"
            ? turn.value
            : row.message?.agentConversationTurn;
        const shell =
          turn?.case === "shellConversationTurn"
            ? turn.value
            : row.message?.shellConversationTurn;
        if (conversation) {
          if (conversation.userMessage?.text)
            push("User", [{ Text: conversation.userMessage.text }]);
          const parts = [];
          for (const step of conversation.steps ?? []) {
            const value = step.message?.value ?? step.step?.value ?? step;
            const kind = step.message?.case ?? step.step?.case ?? step.type;
            if (kind === "assistantMessage" && value.text)
              parts.push({ Text: value.text });
            else if (kind === "thinkingMessage" && value.text)
              parts.push({ Reasoning: value.text });
            else if (kind === "toolCall") {
              const call = value.tool?.value ?? value,
                name = value.tool?.case ?? value.type ?? "Tool";
              parts.push({
                Tool: {
                  call: { kind: "unknown", name, input: call.args ?? null },
                  output: call.result ? JSON.stringify(call.result) : null,
                  is_error: call.result?.status === "error",
                },
              });
            } else
              notice =
                "Some non-text content remains in the native Cursor checkpoint.";
          }
          push("Assistant", parts);
        } else if (shell) {
          if (shell.shellCommand?.command)
            push("Assistant", [
              {
                Tool: {
                  call: { kind: "exec", command: shell.shellCommand.command },
                  output:
                    [shell.shellOutput?.stdout, shell.shellOutput?.stderr]
                      .filter(Boolean)
                      .join("\n") || null,
                  is_error: !!shell.shellOutput?.exitCode,
                },
              },
            ]);
        } else
          notice =
            "Some non-text content remains in the native Cursor checkpoint.";
      }
      offset += page.length;
      if (page.length < 100) break;
      if (offset > 20000) fail("Cursor conversation exceeds the import limit.");
    }
    if (!messages.length)
      fail("Cursor did not return any saved messages for this checkpoint.");
    return { messages, notice };
  } finally {
    await source.close();
  }
}
