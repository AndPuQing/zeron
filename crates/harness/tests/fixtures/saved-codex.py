#!/usr/bin/env python3
"""Native import protocol fixture. Its entire history store is CODEX_HOME."""
import json
import os
import sys
from pathlib import Path

root = Path(os.environ["CODEX_HOME"])
state = json.loads((root / "fixture.json").read_text())
source = state["thread"]

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if "id" not in request:
        continue
    params = request.get("params", {})
    with (root / "calls.jsonl").open("a") as log:
        log.write(json.dumps(request) + "\n")
    error = None
    if method == "initialize":
        result = {}
    elif method == "thread/list":
        assert params["sourceKinds"] == ["cli", "vscode", "appServer"]
        result = {"data": [] if params["archived"] else [source], "nextCursor": None}
    elif method == "thread/read":
        result = {"thread": dict(source, id=params["threadId"])}
    elif method == "thread/turns/list":
        turns = state["turns"]
        if params["sortDirection"] == "desc":
            result = {"data": turns[-1:], "nextCursor": None}
        else:
            offset = int(params.get("cursor") or 0)
            end = offset + params["limit"]
            result = {"data": turns[offset:end], "nextCursor": str(end) if end < len(turns) else None}
            if state.get("loop"):
                result["nextCursor"] = "5"
            if state.get("unsupported"):
                error = {"code": -32601, "message": "unsupported history API"}
    elif method == "thread/fork":
        assert params["excludeTurns"] is True
        assert params["deferGoalContinuation"] is True
        assert params["lastTurnId"] == state["turns"][-1]["id"]
        assert params["cwd"] == source["cwd"]
        result = {"thread": dict(source, id="copy-native-id")}
    elif method == "thread/resume":
        if state.get("resumeError"):
            error = {"code": -32602, "message": "missing copied thread"}
            result = {}
        else:
            result = {"thread": dict(source, id="wrong-id" if state.get("wrongResume") else params["threadId"])}
    elif method == "thread/start":
        result = {"thread": dict(source, id="fresh-thread")}
    else:
        raise RuntimeError("Unexpected import or resume method: " + method)
    if state.get("oversized") and method == "thread/list":
        sys.stdout.write("x" * (8 * 1024 * 1024 + 1) + "\n")
        sys.stdout.flush()
        break
    emit({"jsonrpc": "2.0", "id": request["id"], **({"error": error} if error else {"result": result})})
