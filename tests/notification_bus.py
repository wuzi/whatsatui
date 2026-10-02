"""Exercise native notifications on a private bus, without desktop popups."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import warnings
warnings.filterwarnings("ignore", category=DeprecationWarning)

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
parser.add_argument('--target-dir', type=Path, default=root / 'target')
args = parser.parse_args()
if not args.inside:
    raise SystemExit(subprocess.call([
        'dbus-run-session', '--', sys.executable, __file__, '--inside',
        '--target-dir', str(args.target_dir.resolve()),
    ]))

from gi.repository import Gio, GLib

interface = 'org.freedesktop.Notifications'
xml = '''<node><interface name="org.freedesktop.Notifications">
<method name="GetCapabilities"><arg type="as" direction="out"/></method>
<method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/></method>
<method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/><arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method>
</interface></node>'''
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
owner = bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'RequestName', GLib.Variant('(su)', (interface, 4)), None, Gio.DBusCallFlags.NONE, 1000, None)
assert owner.unpack() == (1,), 'Refusing to run against an existing notification service'
received = []
pending = []
senders = []
pids = []
prior_connected = []
def method_call(connection, sender, path, iface, method, params, invocation):
    if method == 'GetCapabilities':
        invocation.return_value(GLib.Variant('(as)', (['body', 'body-markup'],)))
    elif method == 'GetServerInformation':
        invocation.return_value(GLib.Variant('(ssss)', ('synthetic', 'test', '1.0', '1.2')))
    elif method == 'Notify':
        received.append(params.unpack())
        pids.append(bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'GetConnectionUnixProcessID', GLib.Variant('(s)', (sender,)), None, Gio.DBusCallFlags.NONE, 1000, None).unpack()[0])
        if senders:
            prior_connected.append(bus.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'NameHasOwner', GLib.Variant('(s)', (senders[0],)), None, Gio.DBusCallFlags.NONE, 1000, None).unpack()[0])
        senders.append(sender)
        title = params.unpack()[3]
        if title == 'Failure':
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', 'Synthetic service failure')
        elif title in ('Timeout', 'Cancelled'):
            # Keep the invocation alive; the Rust caller must stop waiting.
            pending.append(invocation)
        else:
            invocation.return_value(GLib.Variant('(u)', (len(received),)))

bus.register_object('/org/freedesktop/Notifications', Gio.DBusNodeInfo.new_for_xml(xml).interfaces[0], method_call, None, None)
loop = GLib.MainLoop()
results = []
def test():
    env = os.environ.copy()
    env['WHATSAPP_TUI_PRIVATE_NOTIFICATION_BUS'] = '1'
    try:
        results.append(subprocess.run(['cargo', 'test', '--offline', '--locked', '--target-dir', str(args.target_dir), '-j', '2', '--lib', 'notifications::native::tests::native_notifier_private_bus_smoke', '--', '--ignored', '--exact', '--test-threads=2'], cwd=root, env=env, text=True, capture_output=True, timeout=180))
    finally:
        GLib.idle_add(loop.quit)
thread = threading.Thread(target=test)
thread.start()
loop.run()
thread.join()
assert results, 'native smoke did not complete'
result = results[0]
print(result.stdout)
print(result.stderr)
assert result.returncode == 0, 'native notifier smoke failed'
assert [r[3] for r in received] == [
    'Synthetic notification', 'Another conversation', 'Failure', 'Timeout',
    'After timeout', 'Cancelled', 'After cancellation', 'Reconnected',
], received
assert len(set(pids)) == 1, f'Notifications came from separate processes: {pids}'
assert len(set(senders[:-1])) == 1 and all(prior_connected[:-1]), f'Notification connection must stay alive: {senders}, {prior_connected}'
assert senders[-1] != senders[0] and not prior_connected[-1], 'Closed connection must reconnect'
app, replace, icon, title, body, actions, hints, expire = received[0]
assert app == 'whatsapp-tui' and title == 'Synthetic notification'
assert body == '&lt;tag&gt; &amp; literal', body
assert hints['suppress-sound'] is True, hints
assert hints['urgency'] == 1, hints
assert hints['category'] == 'im.received', hints
assert not actions and expire == 7000 and replace == 0
assert icon == 'mail-message-new'
print(json.dumps({'result': 'PASS', 'notifications': len(received), 'one_process': len(set(pids)) == 1, 'one_live_connection_until_disconnect': len(set(senders[:-1])) == 1 and all(prior_connected[:-1]), 'reconnected': senders[-1] != senders[0], 'title': title, 'body': body, 'hints': hints, 'real_desktop_accessed': False}, sort_keys=True))
