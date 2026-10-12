#!/usr/bin/env python3
"""Native Copilot ACP shapes, without authentication or a model service."""
import json
import sys

if sys.argv[1:] == ['--version']:
    print('GitHub Copilot CLI 1.0.95.')
    sys.exit(0)
assert sys.argv[1:] == ['--acp'], sys.argv


def emit(message):
    print(json.dumps(dict(jsonrpc='2.0', **message)), flush=True)


def reply(id, result):
    emit(dict(id=id, result=result))


def update(value):
    emit(dict(method='session/update', params=dict(sessionId=sid, update=value)))


def options():
    return [
        dict(type='select', id='model', name='Model', category='model', currentValue=model,
             options=[dict(value='auto', name='Auto', description='Let Copilot pick the best model'),
                      dict(value='auto', name='Auto'), dict(value='gpt-4.1', name='GPT-4.1')]),
        dict(type='select', id='reasoning_effort', name='Reasoning effort',
             category='thought_level', currentValue=effort,
             options=[dict(value='low', name='Low'), dict(value='high', name='High')]),
        dict(type='select', id='allow_all', name='Allow All', category='permissions',
             currentValue='off', options=[dict(value='on', name='On'), dict(value='off', name='Off')]),
    ]


sid, model, effort = 'copilot-session', 'auto', 'low'
pending, permission = None, False
for line in sys.stdin:
    message = json.loads(line)
    method, params, id = message.get('method'), message.get('params', {}), message.get('id')
    if method == 'initialize':
        assert params['protocolVersion'] == 1
        reply(id, dict(protocolVersion=1,
                       agentCapabilities=dict(loadSession=True, promptCapabilities=dict(image=True)),
                       authMethods=[dict(id='copilot-login', name='Log in with Copilot CLI')]))
    elif method in ('session/new', 'session/load'):
        if params.get('sessionId') in ('missing', 'signed-out'):
            code = -32000 if params['sessionId'] == 'signed-out' else -32602
            emit(dict(id=id, error=dict(code=code, message='Authentication required' if code == -32000 else 'Unknown session')))
            continue
        sid = params.get('sessionId', 'copilot-session')
        if method == 'session/load':
            update(dict(sessionUpdate='agent_message_chunk', content=dict(type='text', text='OLD HISTORY')))
        reply(id, dict(sessionId=sid, configOptions=options()))
        update(dict(sessionUpdate='available_commands_update', availableCommands=[
            dict(name='compact', description='Summarize conversation history'),
            dict(name='plan', description='Create an implementation plan', input=dict(hint='prompt'))]))
    elif method == 'session/set_config_option':
        if params['configId'] == 'model':
            model = params['value']
        elif params['configId'] == 'reasoning_effort':
            effort = params['value']
        reply(id, dict(configOptions=options()))
    elif method == 'session/prompt':
        text = ''.join(block.get('text', '') for block in params['prompt'])
        images = sum(block['type'] == 'image' for block in params['prompt'])
        update(dict(sessionUpdate='agent_message_chunk', content=dict(type='text', text=f'{text};model={model};effort={effort};images={images}')))
        if text == 'wait':
            pending = id
        elif text == 'tool':
            pending, permission = id, True
            call = dict(toolCallId='copilot-tool', kind='execute', title='Read local marker',
                        status='pending', rawInput=dict(command="printf 'copilot-ok'"))
            update(dict(sessionUpdate='tool_call', **call))
            emit(dict(id=900, method='session/request_permission', params=dict(
                sessionId=sid, toolCall=call, options=[
                    dict(optionId='allow_once', kind='allow_once', name='Allow once'),
                    dict(optionId='allow_always', kind='allow_always', name='Always allow'),
                    dict(optionId='reject_once', kind='reject_once', name='Deny')])))
        else:
            reply(id, dict(stopReason='end_turn'))
    elif method == 'session/cancel':
        if pending is not None:
            reply(pending, dict(stopReason='cancelled'))
            pending = None
        if id is not None:
            reply(id, {})
    elif id == 900 and permission:
        assert message['result']['outcome']['outcome'] == 'selected'
        assert message['result']['outcome']['optionId'].startswith('allow_')
        update(dict(sessionUpdate='tool_call_update', toolCallId='copilot-tool', status='completed',
                    content=[dict(type='content', content=dict(type='text', text='copilot-ok'))]))
        reply(pending, dict(stopReason='end_turn'))
        pending, permission = None, False
    elif id is not None:
        emit(dict(id=id, error=dict(code=-32601, message=f'Unsupported method {method}')))
