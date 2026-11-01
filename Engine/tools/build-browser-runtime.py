#!/usr/bin/env python3
"""Build the shared game runtime once for direct web and Android exports."""
import argparse
import os
from pathlib import Path
import shutil
import shlex
import subprocess


def bindgen_sysroot_arguments(sysroot):
    # bindgen parses this variable with POSIX shell rules, including on Windows.
    # Forward slashes preserve drive paths; quoting preserves spaces in SDK paths.
    return shlex.join(['--sysroot=' + sysroot.as_posix()])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', default='distribution')
    parser.add_argument('--toolchain', default=None)
    parser.add_argument('--install-sdk', action='store_true', help='Install the pinned Emscripten SDK under .tools if it is missing')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    if shutil.which('emcc') is None and args.install_sdk:
        sdk = root / '.tools' / 'emsdk'
        if not sdk.exists():
            sdk.parent.mkdir(parents=True, exist_ok=True)
            subprocess.run(['git', 'clone', '--depth', '1', '--branch', '6.0.13',
                            'https://github.com/emscripten-core/emsdk.git', str(sdk)], check=True)
        import sys
        command = [sys.executable, str(sdk / 'emsdk.py')]
        subprocess.run(command + ['install', '6.0.13'], check=True)
        subprocess.run(command + ['activate', '6.0.13'], check=True)
        os.environ['EMSDK'] = str(sdk)
        os.environ['EM_CONFIG'] = str(sdk / '.emscripten')
        os.environ['PATH'] = str(sdk / 'upstream' / 'emscripten') + os.pathsep + os.environ['PATH']
    if shutil.which('emcc') is None:
        parser.error('Activate Emscripten SDK 6.0.13 with emsdk_env before building the browser runtime.')
    cargo = ['cargo'] + ([f'+{args.toolchain}'] if args.toolchain else [])
    target = 'wasm32-unknown-emscripten'
    rustup = ['rustup', 'target', 'add', target]
    if args.toolchain:
        rustup.extend(['--toolchain', args.toolchain])
    subprocess.run(rustup, cwd=root, check=True)
    environment = os.environ.copy()
    if os.name == 'nt' and 'LIBCLANG_PATH' not in environment:
        llvm = Path(environment.get('ProgramFiles', 'C:/Program Files')) / 'LLVM' / 'bin'
        if (llvm / 'libclang.dll').is_file():
            environment['LIBCLANG_PATH'] = str(llvm)
    options = [
        '-sALLOW_MEMORY_GROWTH=1', '-sINITIAL_MEMORY=67108864', '-sSTACK_SIZE=8388608',
        '-sENVIRONMENT=web', '-sEXIT_RUNTIME=0', '-sFORCE_FILESYSTEM=1',
        '-sMIN_WEBGL_VERSION=2', '-sMAX_WEBGL_VERSION=2', '-sFULL_ES3=1',
        '-sEXPORTED_FUNCTIONS=["_main","_rustic_tick"]',
        '-sEXPORTED_RUNTIME_METHODS=["FS","addRunDependency","removeRunDependency"]', '-lEGL',
    ]
    flags = ['-C', 'panic=abort']
    for option in options:
        flags.extend(['-C', f'link-arg={option}'])
    environment['CARGO_ENCODED_RUSTFLAGS'] = '\x1f'.join(flags[:2])
    environment['CARGO_INCREMENTAL'] = '0'
    environment['CARGO_TARGET_WASM32_UNKNOWN_EMSCRIPTEN_LINKER'] = 'em++'
    environment['CXXFLAGS_wasm32_unknown_emscripten'] = '-fwasm-exceptions'
    environment.setdefault('BINDGEN_EXTRA_CLANG_ARGS', bindgen_sysroot_arguments(Path(shutil.which('emcc')).resolve().parent / 'cache' / 'sysroot'))
    environment[f'CARGO_PROFILE_{args.profile.upper().replace("-", "_")}_LTO'] = 'false'
    subprocess.run(cargo + ['rustc', '-p', 'rustic-browser', '--bin', 'rustic-browser', '--target', target,
                            '--profile', args.profile, '--'] + flags[2:], cwd=root, env=environment, check=True)
    source = root / 'target' / target / args.profile
    destination = root / 'target' / args.profile / 'browser-runtime'
    destination.mkdir(parents=True, exist_ok=True)
    for extension in ['js', 'wasm']:
        artifact = source / f'rustic-browser.{extension}'
        if not artifact.is_file():
            artifact = source / 'deps' / f'rustic_browser.{extension}'
        shutil.copyfile(artifact, destination / f'rustic-browser.{extension}')
    print(f'Browser runtime ready: {destination}')


if __name__ == '__main__':
    main()
