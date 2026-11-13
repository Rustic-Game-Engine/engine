import json
import sys
from pathlib import Path
import tempfile
import threading
import time
import unittest
from urllib.request import Request, urlopen
from urllib.error import HTTPError
from server import Installer, serve


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'payload/extra').mkdir(parents=True)
        (self.root / 'payload/game.exe').write_bytes(b'game')
        (self.root / 'payload/extra/a').write_bytes(b'optional')
        (self.root / 'installer.json').write_text(json.dumps(dict(
            payload='payload', location=str(self.root / 'installed'),
            options=[dict(id='extra', default=False, pathPrefix='extra')]
        )))
        self.installer = Installer(self.root)

    def finish(self):
        self.installer.start()
        for _ in range(200):
            state = self.installer.snapshot()
            if state['status'] != 'installing':
                return state
            time.sleep(.01)
        self.fail('Installer did not finish')

    def test_copy_options_and_progress(self):
        state = self.finish()
        self.assertEqual(state['status'], 'completed')
        self.assertEqual(state['percentage'], 100)
        self.assertEqual((self.root / 'installed/game.exe').read_bytes(), b'game')
        self.assertFalse((self.root / 'installed/extra').exists())
        self.assertTrue(state['logs'])
        with self.assertRaises(ValueError):
            self.installer.configure({'location': str(self.root / 'other')})
        with self.assertRaises(ValueError):
            self.installer.start()

    def test_no_overwrite(self):
        (self.root / 'installed').mkdir()
        (self.root / 'installed/game.exe').write_bytes(b'user')
        self.assertEqual(self.finish()['status'], 'failed')
        self.assertEqual((self.root / 'installed/game.exe').read_bytes(), b'user')

    def test_location_validation_and_options(self):
        for location in ['relative', str(self.root), str(self.root / 'payload/sub')]:
            with self.assertRaises(ValueError):
                self.installer.configure({'location': location})
        self.installer.configure({'location': str(self.root / 'custom'), 'options': {'extra': True}})
        self.assertEqual(self.finish()['status'], 'completed')
        self.assertTrue((self.root / 'custom/extra/a').exists())

    def test_script_context_events_and_phase_order(self):
        script = self.root / 'hook.py'
        script.write_text("""import json, sys
from pathlib import Path
context = json.loads(sys.stdin.readline())
assert context['options']['extra'] is False
assert not (Path(context['location']) / 'game.exe').exists()
(Path(context['location']) / 'hook.txt').write_text(context['task'])
print(json.dumps({'percentage': 50, 'log': 'Script is working'}), flush=True)
""")
        self.installer.config['tasks'] = [dict(id='prepare', phase='beforeCopy',
            command=[sys.executable, str(script)])]
        state = self.finish()
        self.assertEqual(state['status'], 'completed', state['error'])
        self.assertIn('Script is working', state['logs'])
        self.assertEqual((self.root / 'installed/hook.txt').read_text(), 'prepare')

    def test_after_copy_and_disabled_task(self):
        self.installer.config['tasks'] = [
            dict(id='skipped', option='extra', command=['missing-executable']),
            dict(id='verify-copy', command=[sys.executable, '-c',
                "import json,sys;from pathlib import Path;c=json.loads(sys.stdin.readline());assert (Path(c['location'])/'game.exe').read_bytes()==b'game'"])
        ]
        state = self.finish()
        self.assertEqual(state['status'], 'completed', state['error'])

    def test_script_failure_and_timeout(self):
        for command, timeout in [([sys.executable, '-c', 'raise SystemExit(7)'], 10),
                                 ([sys.executable, '-c', 'import time;time.sleep(10)'], .1)]:
            installer = Installer(self.root)
            installer.config['tasks'] = [dict(id='failure', phase='beforeCopy',
                command=command, timeoutSeconds=timeout)]
            self.installer = installer
            state = self.finish()
            self.assertEqual(state['status'], 'failed')
            self.assertIn('failure', state['error'])
            self.assertFalse((self.root / 'installed/game.exe').exists())

    def test_script_cancellation(self):
        self.installer.config['tasks'] = [dict(id='waiting', phase='beforeCopy',
            command=[sys.executable, '-c', 'import time;time.sleep(10)'])]
        self.installer.start()
        for _ in range(200):
            if self.installer.snapshot()['currentTask'] == 'waiting':
                break
            time.sleep(.01)
        self.installer.cancel.set()
        for _ in range(200):
            if self.installer.snapshot()['status'] == 'cancelled':
                break
            time.sleep(.01)
        self.assertEqual(self.installer.snapshot()['status'], 'cancelled')

    def test_authenticated_http(self):
        server, token = serve(self.installer)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        url = f'http://127.0.0.1:{server.server_port}/scripts/install/status'
        with self.assertRaises(HTTPError) as caught:
            urlopen(url)
        self.assertEqual(caught.exception.code, 401)
        with urlopen(Request(url, headers={'Authorization': 'Bearer ' + token})) as response:
            self.assertEqual(json.load(response)['status'], 'ready')


if __name__ == '__main__':
    unittest.main()
