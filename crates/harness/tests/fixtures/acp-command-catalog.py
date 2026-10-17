#!/usr/bin/env python3
"""Offline ACP catalog variants; no model turns or provider credentials."""
import json
import pathlib
import sys


def command(name):
    return {"name": name, "description": "Fixture command", "input": {"hint": "project arguments"}}


def emit(value):
    print(json.dumps(value), flush=True)


for line in sys.stdin:
    message = json.loads(line)
    if "id" not in message:
        continue
    method = message.get("method")
    params = message.get("params", {})
    if method == "initialize":
        emit({"id": message["id"], "result": {"protocolVersion": 1,
              "agentCapabilities": {"_meta": {"availableCommands": [command("initialize-command")]}}}})
    elif method == "session/new":
        mode = pathlib.Path(params["cwd"]).name
        result = {"sessionId": "catalog-session"}
        if mode != "initialize":
            result["_meta"] = {"availableCommands": [] if mode == "empty" else [command("workspace-command")]}
        emit({"id": message["id"], "result": result})
        if mode.startswith("notification"):
            emit({"method": "session/update", "params": {"sessionId": "catalog-session", "update": {
                "sessionUpdate": "available_commands_update",
                "availableCommands": [] if mode == "notification-empty" else [command("updated-command")]
            }}})
    else:
        emit({"id": message["id"], "result": {}})
