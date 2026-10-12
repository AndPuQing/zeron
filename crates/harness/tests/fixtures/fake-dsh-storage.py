#!/usr/bin/env python3
"""ACP peer that persists session, workspace, projection and search records."""

import json
import os
from pathlib import Path
import sys
import time

if "--version" in sys.argv:
    print("0.2.0")
    sys.exit(0)

fixture = Path(__file__).parent
home = Path(os.environ["DSH_HOME"])
roots = {"sessions": home / "sessions", "storages": home / "storages"}
search = str(home / "session-index.sqlite")
overlay = None
if "--patch" in sys.argv:
    overlay = Path(sys.argv[sys.argv.index("--patch") + 1])
    for patch in json.loads(overlay.read_text()):
        config = patch["config"]
        if patch["id"] == "session-persistence-jsonl":
            roots["sessions"] = Path(config["root"])
        elif patch["id"] == "storage-json":
            roots["storages"] = Path(config["root"])
        elif patch["id"] == "storage-domain":
            assert config["backend"] == "json"
        elif patch["id"] == "session-query-sqlite":
            assert config["openAt"] == "never"
            search = config["path"]


def emit(frame):
    print(json.dumps(dict(jsonrpc="2.0", **frame)), flush=True)


for line in sys.stdin:
    req = json.loads(line)
    method = req.get("method")
    result = {}
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {}}
    elif method == "session/new":
        for name, root in roots.items():
            root.mkdir(parents=True, exist_ok=True)
            (root / f"probe-{os.getpid()}.json").write_text(name)
        if search != ":memory:":
            Path(search).write_text("search index")
        with (fixture / "launches.jsonl").open("a") as log:
            log.write(json.dumps({
                "scratch": str(overlay.parent) if overlay else None,
                "home": str(home),
                "cwd": req["params"]["cwd"],
                "roots": {key: str(value) for key, value in roots.items()},
                "search": search,
            }) + "\n")
        state = (fixture / "state").read_text()
        if state == "timeout":
            time.sleep(60)
        if state == "exit":
            sys.exit(1)
        if state == "error":
            emit({"id": req["id"], "error": {"code": -32603, "message": "probe failed"}})
            continue
        result = {"sessionId": "fixture-session", "configOptions": [{
            "id": "model", "name": "Model", "category": "model", "type": "select",
            "currentValue": "fixture-model",
            "options": [{"value": "fixture-model", "name": "Fixture Model"}],
        }]}
    elif method == "session/prompt":
        result = {"stopReason": "end_turn"}
    emit({"id": req["id"], "result": result})
    if method == "session/new":
        emit({"method": "session/update", "params": {
            "sessionId": "fixture-session", "update": {
                "sessionUpdate": "available_commands_update",
                "availableCommands": [{"name": "plan", "description": "Plan carefully"}],
            },
        }})
    if method == "session/prompt":
        break
