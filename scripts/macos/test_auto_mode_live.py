#!/usr/bin/env python3
"""Opt-in real Claude Auto Mode test; requires GUI authorization and uses plan quota.

Uses a synthetic project and isolated native configuration. SessionStart records only
the server-classifier flag and loopback endpoint. No permission bypass or fake verdicts.
"""
from pathlib import Path
import json
import os
import shlex
import subprocess
import tempfile
import uuid

launcher=Path.home()/'.local/bin/claude-gpt'
report=[]
with tempfile.TemporaryDirectory(prefix='bridge-auto-mode-live-') as directory:
    base=Path(directory)
    project=base/'project'; project.mkdir()
    config=base/'native-config'; config.mkdir()
    receipt=base/'child-environment.txt'
    session=str(uuid.uuid4())
    hook="printf '%s\\n%s\\n' \"$CLAUDE_CODE_AUTO_MODE_SERVER\" \"$ANTHROPIC_BASE_URL\" > "+shlex.quote(str(receipt))
    supplied=json.dumps({'env':{'CLAUDE_CODE_AUTO_MODE_SERVER':'1'},'hooks':{'SessionStart':[{'hooks':[{'type':'command','command':hook}]}]}})
    env=os.environ|{'CLAUDE_CONFIG_DIR':str(config),'CLAUDE_CODE_AUTO_MODE_SERVER':'1'}
    cases=[
        ('normal','Reply exactly AUTO MODE OK.',['--session-id',session,'--tools','']),
        ('resume','What exact phrase did you just reply? Repeat only that phrase.',['--resume',session,'--tools','']),
        ('continue','Use Bash once to run exactly: printf "AUTO TOOL OK\\n" . Then reply exactly AUTO CONTINUE OK.',['--continue','--tools','Bash']),
    ]
    for name,prompt,extra in cases:
        receipt.unlink(missing_ok=True)
        arguments=[str(launcher),'-p',prompt,'--permission-mode','auto','--settings',supplied,'--output-format','stream-json','--verbose','--strict-mcp-config','--mcp-config','{"mcpServers":{}}','--max-turns','6',*extra]
        result=subprocess.run(arguments,cwd=project,env=env,capture_output=True,timeout=180)
        lines=[json.loads(line) for line in result.stdout.decode().splitlines() if line.startswith('{')]
        final=next((event for event in reversed(lines) if event.get('type')=='result'),{})
        environment=receipt.read_text().splitlines() if receipt.exists() else []
        assert environment and environment[0]=='0',name+': native SessionStart did not receive flag 0'
        assert len(environment)==2 and environment[1].startswith('http://127.0.0.1:'),name+': unexpected local endpoint'
        if result.returncode or final.get('is_error'):
            print(json.dumps({'case':name,'returncode':result.returncode,'result_subtype':final.get('subtype'),'error_output_withheld':True}),flush=True)
        assert result.returncode==0 and final and not final.get('is_error'),name+': real native request failed'
        assert "isn't eligible" not in result.stdout.decode()+result.stderr.decode(),name+': classifier eligibility warning remains'
        expected='AUTO CONTINUE OK' if name=='continue' else 'AUTO MODE OK'
        assert expected in final.get('result',''),name+': text/session recovery mismatch'
        calls={block.get('name') for event in lines if event.get('type')=='assistant' for block in event.get('message',{}).get('content',[]) if block.get('type')=='tool_use'}
        if name=='continue': assert 'Bash' in calls,name+': native auto-mode tool call missing'
        report.append({'case':name,'child_auto_server':'0','native_permission_mode':'auto','completed':True,'native_tools':sorted(calls)})
        print('PASS real native '+name+'; SessionStart env=0; Auto Mode; no eligibility warning',flush=True)
print(json.dumps({'real_claude':True,'real_upstream':True,'cases':report,'new_oauth':False,'permission_bypass':False}))
