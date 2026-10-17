#!/usr/bin/env python3
"""Offline DSH ACP peer emitting the bundled bridge's generic tool frames."""
import json
from pathlib import Path
import sys

cases = json.loads(Path(__file__).with_name("dsh-tool-calls.json").read_text())

def emit(frame):
    print(json.dumps(dict(jsonrpc="2.0", **frame)), flush=True)

def update(value):
    emit({"method": "session/update", "params": {"sessionId": "dsh-tools", "update": value}})

for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request.get("method")
    result = {}
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {}}
    elif method == "session/new":
        result = {"sessionId": "dsh-tools"}
    elif method == "session/prompt":
        for i, case in enumerate(cases):
            update({"sessionUpdate": "tool_call", "toolCallId": str(i), "title": case["name"],
                    "kind": "other", "status": "in_progress", "rawInput": case["input"]})
            update({"sessionUpdate": "tool_call_update", "toolCallId": str(i), "status": "completed",
                    "content": [{"type": "content", "content": {"type": "text", "text": "fixture-secret-output"}}]})
        result = {"stopReason": "end_turn"}
    emit({"id": request["id"], "result": result})
