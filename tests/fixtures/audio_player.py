#!/usr/bin/python3
"""Synthetic mpv IPC peer; never opens an audio device or reads user files."""
import json, os, pathlib, select, socket, sys, time

base = pathlib.Path(__file__)
base.with_suffix('.pid').write_text(str(os.getpid()))
mode = pathlib.Path(sys.argv[-1]).read_text()
base.with_suffix('.snapshot').write_text(sys.argv[-1])
base.with_suffix('.args').write_text(json.dumps(sys.argv[1:]))
with base.with_suffix('.starts').open('a') as log:
    log.write(mode + '\n')
if mode == 'exit':
    sys.exit(3)
if mode == 'stall':
    time.sleep(30)
    sys.exit(0)
def emit(value):
    os.write(0, json.dumps(value).encode() + b'\n')
if mode == 'malformed':
    os.write(0, b'x' * 70000 + b'\n')
    time.sleep(30)
    sys.exit(0)
state = {'pause': True, 'speed': 1.0, 'duration': 20.0, 'time-pos': 0.0}
emit({'event': 'file-loaded'})
pending = b''
stall_at = time.monotonic() + .4
while True:
    window = base.with_suffix('.window')
    if window.exists():
        event = json.loads(window.read_text())
        window.unlink()
        if event['event'] == 'property-change':
            state[event['name']] = event['data']
        emit(event)
        if event['event'] == 'end-file':
            sys.exit(0)
    if mode == 'late-stall' and time.monotonic() >= stall_at:
        time.sleep(30)
        sys.exit(0)
    if select.select([0], [], [], .04)[0]:
        chunk = os.read(0, 4096)
        if not chunk:
            break
        pending += chunk
        while b'\n' in pending:
            line, pending = pending.split(b'\n', 1)
            value = json.loads(line)
            command = value['command']
            if mode == 'quit-after-observe' and command[0] == 'observe_property' and command[2] == 'speed':
                peer = socket.socket(fileno=0)
                peer.shutdown(socket.SHUT_RD)
                emit({'request_id': value['request_id'], 'error': 'success'})
                emit({'event': 'end-file', 'reason': 'quit'})
                time.sleep(30)
                sys.exit(0)
            if mode == 'quit-on-speed' and command[:2] == ['set_property', 'speed']:
                peer = socket.socket(fileno=0)
                peer.shutdown(socket.SHUT_RD)
                emit({'event': 'end-file', 'reason': 'quit'})
                time.sleep(30)  # parent must stop/reap without writing another control
                sys.exit(0)
            if mode == 'eof-on-health' and command[0] == 'get_property':
                emit({'event': 'end-file', 'reason': 'eof'})
                sys.exit(0)
            error = 'success'
            if command[0] == 'observe_property':
                name = command[2]
                emit({'event': 'property-change', 'name': name, 'data': state.get(name)})
            elif command[0] == 'set_property':
                if mode == 'reject':
                    error = 'property unavailable'
                else:
                    state[command[1]] = command[2]
                    emit({'event': 'property-change', 'name': command[1], 'data': command[2]})
            emit({'request_id': value['request_id'], 'error': error})
    if not state['pause']:
        state['time-pos'] += .04 * state['speed']
        emit({'event': 'property-change', 'name': 'time-pos', 'data': state['time-pos']})
        if mode == 'eof' and state['time-pos'] >= .2:
            emit({'event': 'end-file', 'reason': 'eof'})
            break
