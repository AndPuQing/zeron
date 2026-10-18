"""Use the installed Hermes persistence API against a synthetic, isolated DB."""
import json,os,sys
from pathlib import Path
sys.path.insert(0,os.environ['ZERON_HERMES_TEST_SOURCE'])
from hermes_state import SessionDB
home=Path(os.environ['HERMES_HOME']);action=sys.argv[1];sid=sys.argv[2] if len(sys.argv)>2 else 'hermes-native-source'
db=SessionDB()
if action=='create':
    home.joinpath('config.yaml').write_text('model:\n  default: fixture-model\n  provider: custom\n  base_url: http://127.0.0.1:9/v1\nmcp_servers: {}\n')
    db.create_session(sid,source='acp',model='fixture-model',model_config={'cwd':str(home),'provider':'custom','base_url':'http://127.0.0.1:9/v1'})
    db.replace_messages(sid,[{'role':'user','content':'Remember teal'},{'role':'assistant','reasoning_content':'Native reasoning','content':'','tool_calls':[{'id':'tool-1','type':'function','function':{'name':'terminal','arguments':'{"command":"pwd"}'}}]},{'role':'tool','tool_call_id':'tool-1','tool_name':'terminal','content':'/fixture'},{'role':'assistant','content':'Native Hermes answer'}])
elif action=='snapshot':
    print(json.dumps({'row':db.get_session(sid),'messages':db.get_messages_as_conversation(sid)},sort_keys=True,default=str))
elif action=='append':
    conversation=db.get_messages_as_conversation(sid)
    assert any(m.get('content')=='Remember teal' for m in conversation)
    conversation.extend([{'role':'user','content':'Independent continuation'},{'role':'assistant','content':'Native continued answer'}])
    db.replace_messages(sid,conversation)
