import importlib.util
from pathlib import Path, PurePosixPath, PureWindowsPath
import shlex
import tempfile
import unittest

script = Path(__file__).resolve().parents[1] / 'build-browser-runtime.py'
spec = importlib.util.spec_from_file_location('browser_build', script)
browser_build = importlib.util.module_from_spec(spec)
spec.loader.exec_module(browser_build)


class BindgenArgumentsTests(unittest.TestCase):
    def test_windows_sdk_path_preserves_drive_and_separators(self):
        root = PureWindowsPath(r'D:\a\engine\Engine\.tools\emsdk\upstream\emscripten\cache\sysroot')
        actual = shlex.split(browser_build.bindgen_sysroot_arguments(root))
        self.assertEqual(actual, ['--sysroot=D:/a/engine/Engine/.tools/emsdk/upstream/emscripten/cache/sysroot'])

    def test_sdk_paths_with_spaces_remain_a_single_argument(self):
        for root in [PureWindowsPath(r'C:\Program Files\Emscripten SDK\cache\sysroot'),
                     PurePosixPath('/opt/Emscripten SDK/cache/sysroot'),
                     PurePosixPath("/opt/developer's SDK/cache/sysroot")]:
            with self.subTest(root=root):
                self.assertEqual(shlex.split(browser_build.bindgen_sysroot_arguments(root)),
                                 ['--sysroot=' + root.as_posix()])


class EmscriptenToolTests(unittest.TestCase):
    def test_uses_resolved_sdk_executables_instead_of_legacy_batch_names(self):
        with tempfile.TemporaryDirectory(prefix='Emscripten SDK ') as directory:
            launchers = {}
            for name in ['emcc', 'em++', 'emar']:
                path = Path(directory) / f'{name}.exe'
                path.touch()
                launchers[name] = str(path)
            actual = browser_build.emscripten_tool_environment(launchers.get)
            self.assertEqual(actual['CC_wasm32_unknown_emscripten'], str(Path(launchers['emcc']).resolve()))
            self.assertEqual(actual['CXX_wasm32_unknown_emscripten'], str(Path(launchers['em++']).resolve()))
            self.assertEqual(actual['AR_wasm32_unknown_emscripten'], str(Path(launchers['emar']).resolve()))
            self.assertEqual(actual['CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER'], actual['CXX_wasm32_unknown_emscripten'])

    def test_missing_sdk_tools_fail_before_starting_cargo(self):
        with self.assertRaisesRegex(RuntimeError, 'SDK tool is missing: emcc'):
            browser_build.emscripten_tool_environment(lambda name: None)


if __name__ == '__main__':
    unittest.main()
