// Synthetic protobuf checkpoints for SDK 1.0.32. Only field numbers from its
// generated schema are used; the unmodified SDK reads/resumes every fixture.
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { pathToFileURL } from "node:url";
process.on("uncaughtException", (e) => {
  console.error(e.message);
  process.exit(1);
});
process.on("unhandledRejection", (e) => {
  console.error(e.message);
  process.exit(1);
});
const [modulePath, action, root, kind, id] = process.argv.slice(2);
const sdk = await import(pathToFileURL(modulePath));
const cwd = root;
const varint = (value) => {
  const bytes = [];
  do {
    let b = value & 127;
    value >>>= 7;
    if (value) b |= 128;
    bytes.push(b);
  } while (value);
  return Buffer.from(bytes);
};
const field = (number, data) => {
  data = Buffer.from(data);
  return Buffer.concat([varint((number << 3) | 2), varint(data.length), data]);
};
async function checkpoint(store, agentId, text) {
  const blob = async (data) => {
    const hash = crypto.createHash("sha256").update(data).digest();
    if (
      !(await store.checkpoints.get({ agentId, blobId: hash.toString("hex") }))
    )
      await store.checkpoints.create({
        agentId,
        blobId: hash.toString("hex"),
        data,
      });
    return hash;
  };
  const user = await blob(field(1, text));
  const thought = await blob(field(3, field(1, "Native reasoning")));
  const answer = await blob(field(1, field(1, "Native Cursor answer")));
  const turn = await blob(
    field(
      1,
      Buffer.concat([field(1, user), field(2, thought), field(2, answer)]),
    ),
  );
  const root = await blob(field(8, turn));
  return { schemaVersion: 1, rootBlobId: root.toString("hex") };
}
if (action === "create") {
  let store;
  if (kind === "sqlite") {
    const { SqliteLocalAgentStore } = await import(
      pathToFileURL(path.join(path.dirname(modulePath), "sqlite.js"))
    );
    store = await SqliteLocalAgentStore.open({
      workspaceRef: cwd,
      stateRoot: path.join(
        root,
        "projects",
        "fixture",
        "sdk-agent-store",
        "fixture",
      ),
    });
  } else
    store = new sdk.JsonlLocalAgentStore(
      path.join(root, "zeron-state", "agents", "source"),
    );
  const agentId = "agent-source",
    latestCheckpoint = await checkpoint(store, agentId, "Remember teal");
  await store.agents.create({
    agent: {
      agentId,
      cwd,
      status: "idle",
      activeRunId: null,
      name: "Native Cursor fixture",
      createdAt: 1000,
      updatedAt: 2000,
      latestCheckpoint,
      sdkMetadata: { opaqueFixture: "preserved" },
    },
  });
  await store.runs.create({
    run: {
      runId: "run-source",
      agentId,
      turnNumber: 1,
      status: "finished",
      model: { id: "fixture-model" },
      createdAt: 1000,
      updatedAt: 2000,
      latestCheckpointRef: latestCheckpoint,
    },
  });
  await store.runEvents.append({
    runId: "run-source",
    eventType: "fixture",
    payload: { opaque: "native event preserved" },
  });
  await store.dispose?.();
} else {
  const directory = fs.readFileSync(
    path.join(root, "zeron-state", "by-agent", id),
    "utf8",
  );
  const store = new sdk.JsonlLocalAgentStore(directory);
  const agent = await sdk.Agent.resume(id, {
    cwd,
    local: { store },
    disallowedTools: ["askQuestion"],
  });
  if (agent.agentId !== id) throw Error("native resume changed identity");
  agent.close();
  const messages = await sdk.Agent.messages.list(id, {
    runtime: "local",
    cwd,
    store,
  });
  if (!JSON.stringify(messages).includes("Remember teal"))
    throw Error("native context missing");
  const doc = await store.agents.get({ agentId: id });
  if (doc.sdkMetadata.opaqueFixture !== "preserved")
    throw Error("opaque metadata lost");
  const latestCheckpoint = await checkpoint(
    store,
    id,
    "Independent continuation",
  );
  await store.agents.update({ agent: { ...doc, latestCheckpoint } });
  if (
    !JSON.stringify(
      await sdk.Agent.messages.list(id, { runtime: "local", cwd, store }),
    ).includes("Independent continuation")
  )
    throw Error("native append failed");
}
process.exit(0);
