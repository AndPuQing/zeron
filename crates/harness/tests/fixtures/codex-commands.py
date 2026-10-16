#!/usr/bin/env python3
"""Offline app-server peer: synchronous commands deliberately emit no turn events."""
import json
import sys

goal = None
mode = None
turn_number = 0

def emit(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    if 'id' not in message:
        continue
    method = message['method']
    params = message.get('params') or {}
    result = {}
    events = []
    if method == 'initialize':
        result = {'userAgent': 'command-fixture'}
    elif method in ('thread/start', 'thread/resume', 'thread/read'):
        result = {'thread': {'id': 'commands-thread', 'cwd': '/fixture', 'status': {'type': 'idle'}}}
    elif method == 'thread/goal/set':
        assert params['origin'] == 'user'
        if 'objective' in params:
            goal = {'threadId': params['threadId'], 'objective': params['objective'], 'status': 'active',
                    'tokensUsed': 123, 'timeUsedSeconds': 7, 'tokenBudget': params.get('tokenBudget')}
        assert goal is not None
        goal['status'] = params.get('status', goal['status'])
        result = {'goal': goal}
    elif method == 'thread/goal/get':
        result = {'goal': goal}
    elif method == 'thread/goal/clear':
        assert params['origin'] == 'user'
        goal = None
    elif method == 'thread/settings/update':
        mode = params['collaborationMode']['mode']
    elif method == 'thread/backgroundTerminals/list':
        if params.get('cursor'):
            result = {'data': [{'processId': 'process-2', 'command': 'second command'}], 'nextCursor': None}
        else:
            result = {'data': [{'processId': 'process-1', 'command': 'first command'}], 'nextCursor': 'next-page'}
    elif method == 'thread/backgroundTerminals/clean':
        result = {}
    elif method == 'skills/list':
        assert len(params['cwds']) == 1 and params['cwds'][0].startswith('/')
        result = {'data': [{'skills': [{'name': 'fixture-skill', 'path': '/fixture-project/SKILL.md'}]}]}
    elif method == 'turn/start':
        text = params['input'][0]['text']
        assert not text.startswith('/'), 'commands must never be submitted as raw prompts'
        mode = params.get('collaborationMode', {}).get('mode', mode)
        turn_number += 1
        tid = f'turn-{turn_number}'
        result = {'turn': {'id': tid}}
        events = [
            {'method': 'turn/started', 'params': {'threadId': 'commands-thread', 'turn': {'id': tid}}},
            {'method': 'item/agentMessage/delta', 'params': {'threadId': 'commands-thread', 'turnId': tid,
                 'itemId': tid, 'delta': 'Fixture response; mode=' + str(mode)}},
            {'method': 'turn/completed', 'params': {'threadId': 'commands-thread', 'turn': {'id': tid, 'status': 'completed'}}},
        ]
    else:
        emit({'id': message['id'], 'error': {'code': -32601, 'message': 'unsupported fixture method'}})
        continue
    emit({'id': message['id'], 'result': result})
    for event in events:
        emit(event)
