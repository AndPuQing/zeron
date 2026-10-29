#!/usr/bin/env python3
"""Stateful offline peer: a global/native priority survives omitted fields."""
import json
import pathlib
import sys

tier = "priority"
turn = 0
root = None

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    if "id" not in message:
        continue
    method, params = message["method"], message.get("params", {})
    result, events = {}, []
    if method == "initialize":
        result = {"userAgent": "service-tier-fixture"}
    elif method in ("thread/start", "thread/resume", "turn/start"):
        root = pathlib.Path(params.get("cwd") or root)
        with (root / "wire.jsonl").open("a") as log:
            log.write(json.dumps(message) + "\n")
        if method == "thread/resume":
            tier = json.loads((root / "native-tier.json").read_text())
        if "serviceTier" in params:
            tier = params["serviceTier"]
            if tier == "fast":
                tier = "priority"
        (root / "native-tier.json").write_text(json.dumps(tier))
        if method != "turn/start":
            result = {"thread": {"id": "tier-thread", "cwd": str(root)}}
        else:
            turn += 1
            tid = f"turn-{turn}"
            result = {"turn": {"id": tid}}
            events = [
                {"method": "turn/started", "params": {"threadId": "tier-thread", "turn": {"id": tid}}},
                {"method": "item/agentMessage/delta", "params": {"threadId": "tier-thread", "turnId": tid, "itemId": tid, "delta": str(tier)}},
                {"method": "turn/completed", "params": {"threadId": "tier-thread", "turn": {"id": tid, "status": "completed"}}},
            ]
    emit({"id": message["id"], "result": result})
    for event in events:
        emit(event)
