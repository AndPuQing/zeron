#!/usr/bin/env python3
"""Local OpenAI-compatible SSE peer for the opt-in native Copilot test."""
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Model(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        messages = body.get('messages', [])
        users = [m for m in messages if m['role'] == 'user']
        text = str(users[-1]['content']) if users else ''
        content = users[-1]['content'] if users else ''
        image = isinstance(content, list) and any(part.get('type') == 'image_url' for part in content)
        stalled = 'native-wait' in text
        tool_done = any(m['role'] == 'tool' for m in messages)
        delta, finish = dict(role='assistant', content='LOCAL-DONE' + (' IMAGE-OK' if image else '')), 'stop'
        if stalled:
            delta = dict(role='assistant', content='WAITING')
        elif 'native-tool' in text and not tool_done:
            delta = dict(role='assistant', tool_calls=[dict(index=0, id='local-tool', type='function',
                         function=dict(name='bash', arguments=json.dumps(dict(
                             command="printf 'copilot-ok'", description='Read local marker'))))])
            finish = 'tool_calls'
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()
        try:
            for change, reason in [(delta, None), ({}, finish)]:
                chunk = dict(id='local-model', object='chat.completion.chunk', created=int(time.time()),
                             model='gpt-4.1', choices=[dict(index=0, delta=change, finish_reason=reason)])
                self.wfile.write(('data: ' + json.dumps(chunk) + '\n\n').encode())
                self.wfile.flush()
                if stalled and reason is None:
                    time.sleep(30)
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


server = ThreadingHTTPServer(('127.0.0.1', 0), Model)
print(server.server_port, flush=True)
server.serve_forever()
