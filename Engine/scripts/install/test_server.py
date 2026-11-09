import json
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
