"""Language-neutral custom installer host. Python 3.10+, standard library only."""
import argparse
import hmac
import json
from pathlib import Path
import secrets
import shutil
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Installer:
    def __init__(self, bundle):
        self.bundle = Path(bundle).resolve()
        self.config = json.loads((self.bundle / 'installer.json').read_text(encoding='utf-8'))
        self.payload = (self.bundle / self.config.get('payload', '../game')).resolve()
        if not self.payload.is_dir():
            raise ValueError('Payload directory does not exist')
        self.lock = threading.RLock()
        self.cancel = threading.Event()
        self.state = dict(status='ready', percentage=0, currentTask=None,
                          location=str(Path(self.config['location']).expanduser().absolute()),
                          options={o['id']: o.get('default', False) for o in self.config.get('options', [])},
                          logs=[], error=None)

    def snapshot(self):
        with self.lock:
            return json.loads(json.dumps(self.state))

    def configure(self, values):
        with self.lock:
            if self.state['status'] != 'ready':
                raise ValueError('Configuration is locked after installation starts')
            if set(values) - {'location', 'options'}:
                raise ValueError('Unknown configuration field')
            location = self.state['location']
            if 'location' in values:
                if not self.config.get('allowLocationChange', True):
                    raise ValueError('Location changes are disabled')
                if not isinstance(values['location'], str) or not values['location'].strip():
                    raise ValueError('Location must be a nonempty absolute path')
                path = Path(values['location']).expanduser()
                if not path.is_absolute():
                    raise ValueError('Location must be absolute')
                destination = path.resolve()
                if destination == Path(destination.anchor) or destination == self.payload or destination in self.payload.parents or self.payload in destination.parents:
                    raise ValueError('Location must not overlap the payload or be a filesystem root')
                location = str(destination)
            options = values.get('options', self.state['options'])
            if not isinstance(options, dict) or set(options) - self.state['options'].keys() or any(type(v) is not bool for v in options.values()):
                raise ValueError('Options must contain known IDs and boolean values')
            self.state.update(location=location, options={**self.state['options'], **options})

    def start(self):
        with self.lock:
            if self.state['status'] != 'ready':
                raise ValueError('Installation has already started')
            # Validate the default location as well as user changes.
            location = self.state['location']
            allowed = self.config.get('allowLocationChange', True)
            self.config['allowLocationChange'] = True
            try:
                self.configure({'location': location})
            finally:
                self.config['allowLocationChange'] = allowed
            self.state['status'] = 'installing'
            threading.Thread(target=self.run, daemon=True).start()

    def run(self):
        try:
            destination = Path(self.state['location'])
            entries = sorted(self.payload.rglob('*'))
            if any(p.is_symlink() for p in entries):
                raise ValueError('Payload symlinks are not supported')
            files = [p for p in entries if p.is_file()]
            for option in self.config.get('options', []):
                prefix = option.get('pathPrefix')
                if prefix:
                    prefix_path = Path(prefix)
                    if prefix_path.is_absolute() or '..' in prefix_path.parts:
                        raise ValueError('Option pathPrefix must be relative to payload')
                    if not self.state['options'][option['id']]:
                        files = [p for p in files if not p.relative_to(self.payload).is_relative_to(prefix_path)]
            destination.mkdir(parents=True, exist_ok=True)
            for index, source in enumerate(files):
                if self.cancel.is_set():
                    with self.lock:
                        self.state.update(status='cancelled', currentTask=None)
                    return
                relative = source.relative_to(self.payload)
                target = destination / relative
                # Reject existing symlinks/junctions escaping the selected directory.
                if not target.resolve().is_relative_to(destination.resolve()):
                    raise ValueError('Destination file escapes the installation directory')
                with self.lock:
                    self.state['currentTask'] = str(relative)
                target.parent.mkdir(parents=True, exist_ok=True)
                if target.exists():
                    raise ValueError(f'Destination already exists: {relative}')
                # Exclusive creation preserves existing user files.
                with source.open('rb') as src, target.open('xb') as dst:
                    shutil.copyfileobj(src, dst)
                with self.lock:
                    self.state['percentage'] = round((index + 1) * 100 / max(1, len(files)), 2)
                    self.state['logs'].append(f'Installed {relative}')
                    self.state['logs'] = self.state['logs'][-1000:]
            with self.lock:
                self.state.update(status='completed', percentage=100, currentTask=None)
        except Exception as error:
            with self.lock:
                self.state.update(status='failed', error=str(error), currentTask=None)
                self.state['logs'].append(str(error))


def serve(installer, port=0):
    token = secrets.token_urlsafe(32)

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass  # Do not log the capability token.

        def reply(self, status, value):
            body = json.dumps(value).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path == '/':
                body = (installer.bundle / 'index.html').read_bytes()
                self.send_response(200)
                self.send_header('Content-Type', 'text/html; charset=utf-8')
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            self.api(False)

        def do_POST(self):
            self.api(True)

        def api(self, post):
            if not hmac.compare_digest(self.headers.get('Authorization', ''), 'Bearer ' + token):
                self.reply(401, {'error': 'Bearer token required'})
                return
            route = self.path.removeprefix('/scripts/install')
            try:
                if not self.path.startswith('/scripts/install/'):
                    self.reply(404, {'error': 'Unknown route'})
                    return
                if post:
                    size = int(self.headers.get('Content-Length', '0'))
                    if not 0 <= size <= 65536:
                        raise ValueError('Request exceeds 64 KiB')
                    data = json.loads(self.rfile.read(size) or b'{}')
                    if not isinstance(data, dict):
                        raise ValueError('Expected JSON object')
                    if route == '/configure':
                        installer.configure(data)
                    elif route == '/start':
                        installer.start()
                    elif route == '/cancel':
                        with installer.lock:
                            if installer.state['status'] != 'installing':
                                raise ValueError('No active installation to cancel')
                            installer.cancel.set()
                    else:
                        self.reply(404, {'error': 'Unknown route'})
                        return
                elif route == '/config':
                    self.reply(200, installer.config)
                    return
                elif route not in ('/status', '/logs'):
                    self.reply(404, {'error': 'Unknown route'})
                    return
                state = installer.snapshot()
                self.reply(200, state['logs'] if route == '/logs' else state)
            except (ValueError, TypeError) as error:
                self.reply(400, {'error': str(error)})

    server = ThreadingHTTPServer(('127.0.0.1', port), Handler)
    return server, token


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bundle', type=Path, default=Path(__file__).parent)
    parser.add_argument('--port', type=int, default=0)
    args = parser.parse_args()
    server, token = serve(Installer(args.bundle), args.port)
    print(json.dumps({'url': f'http://127.0.0.1:{server.server_port}/', 'token': token}), flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        server.server_close()
