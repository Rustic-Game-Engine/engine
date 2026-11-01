import importlib.util
from pathlib import Path, PurePosixPath, PureWindowsPath
import shlex
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


if __name__ == '__main__':
    unittest.main()
