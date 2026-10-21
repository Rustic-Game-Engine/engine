use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

pub(super) fn find_executable(candidates: &[&str]) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH");
    let extensions: Vec<OsString> = if cfg!(windows) {
        std::env::var_os("PATHEXT")
            .unwrap_or_else(|| ".EXE;.CMD;.BAT;.COM".into())
            .to_string_lossy()
            .split(';')
            .map(OsString::from)
            .collect()
    } else {
        vec![OsString::new()]
    };
    find_with_fallback(candidates, paths.as_deref(), &extensions, || {
        #[cfg(windows)]
        {
            windows_install_directories()
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    })
}

fn find_with_fallback(
    candidates: &[&str],
    paths: Option<&OsStr>,
    extensions: &[OsString],
    fallback: impl FnOnce() -> Vec<PathBuf>,
) -> Option<PathBuf> {
    let process_directories = paths.map(std::env::split_paths).into_iter().flatten();
    find_in_directories(candidates, process_directories, extensions)
        .or_else(|| find_in_directories(candidates, fallback(), extensions))
}

fn find_in_directories(
    candidates: &[&str],
    directories: impl IntoIterator<Item = PathBuf>,
    extensions: &[OsString],
) -> Option<PathBuf> {
    for directory in directories {
        for candidate in candidates {
            let direct = directory.join(candidate);
            if direct.is_file() {
                return Some(direct);
            }
            for extension in extensions {
                let mut filename = OsString::from(candidate);
                filename.push(extension);
                let path = directory.join(filename);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[cfg(windows)]
fn windows_install_directories() -> Vec<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };

    // Read on every probe: the editor or its launcher may predate installation.
    // Keep the process PATH first, without changing the editor's global environment.
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let mut directories = Vec::new();
    for (hive, key) in [
        (
            &machine,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
        (&user, r"Environment"),
    ] {
        if let Ok(path) = hive
            .open_subkey(key)
            .and_then(|key| key.get_value::<String, _>("Path"))
        {
            let expanded = expand_environment(&path, |name| std::env::var_os(name));
            directories.extend(std::env::split_paths(&expanded));
        }
    }

    // The official LLVM installer can omit PATH entirely. Its registry entry also
    // covers custom installation directories; inspect both registry views.
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        for hive in [&machine, &user] {
            if let Ok(root) = hive
                .open_subkey_with_flags(r"SOFTWARE\LLVM\LLVM", KEY_READ | view)
                .and_then(|key| key.get_value::<String, _>(""))
            {
                directories.push(PathBuf::from(root).join("bin"));
            }
        }
    }
    for variable in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(variable) {
            directories.push(PathBuf::from(root).join("LLVM/bin"));
        }
    }
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        directories.push(PathBuf::from(root).join("Programs/LLVM/bin"));
    }
    directories
}

#[cfg(any(windows, test))]
fn expand_environment(path: &str, lookup: impl Fn(&str) -> Option<OsString>) -> OsString {
    let mut expanded = OsString::new();
    let mut remaining = path;
    while let Some(start) = remaining.find('%') {
        expanded.push(&remaining[..start]);
        remaining = &remaining[start..];
        let Some(end) = remaining[1..].find('%').map(|index| index + 1) else {
            break;
        };
        if let Some(value) = lookup(&remaining[1..end]) {
            expanded.push(value);
        } else {
            expanded.push(&remaining[..=end]);
        }
        remaining = &remaining[end + 1..];
    }
    expanded.push(remaining);
    expanded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_llvm_is_found_with_stale_or_missing_process_path() {
        let root = tempfile::tempdir().unwrap();
        let stale = root.path().join("old-path");
        let installed = root.path().join("Program Files/LLVM/bin");
        std::fs::create_dir_all(&installed).unwrap();
        let stale_path = std::env::join_paths([stale]).unwrap();
        let extensions = [OsString::from(".exe")];
        for compiler in ["clang", "clang++"] {
            let executable = installed.join(format!("{compiler}.exe"));
            std::fs::write(&executable, []).unwrap();
            for paths in [None, Some(stale_path.as_os_str())] {
                assert_eq!(
                    find_with_fallback(&[compiler], paths, &extensions, || vec![installed.clone()]),
                    Some(executable.clone())
                );
            }
        }
    }

    #[test]
    fn process_path_keeps_precedence_over_installed_toolchains() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("clang++.exe");
        std::fs::write(&executable, []).unwrap();
        let paths = std::env::join_paths([root.path()]).unwrap();
        assert_eq!(
            find_with_fallback(&["clang++"], Some(&paths), &[".exe".into()], || {
                panic!("installed locations should only be searched after PATH")
            }),
            Some(executable)
        );
    }

    #[test]
    fn installation_is_rechecked_and_requires_an_actual_compiler() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("clang++.exe");
        let probe = || {
            find_with_fallback(&["clang++"], None, &[".exe".into()], || {
                vec![root.path().into()]
            })
        };
        assert_eq!(probe(), None);
        std::fs::write(root.path().join("clang.exe"), []).unwrap();
        assert_eq!(probe(), None, "C alone does not satisfy C++");
        std::fs::write(&executable, []).unwrap();
        assert_eq!(probe(), Some(executable.clone()));
        std::fs::remove_file(executable).unwrap();
        assert_eq!(probe(), None);
    }

    #[test]
    fn persisted_path_expands_variables_and_preserves_unknown_values() {
        let path = "%ProgramFiles%/LLVM/bin;%CUSTOM%/bin;%UNKNOWN%;trailing%";
        assert_eq!(
            expand_environment(path, |name| match name {
                "ProgramFiles" => Some("C:/Program Files".into()),
                "CUSTOM" => Some("D:/工具".into()),
                _ => None,
            }),
            OsString::from("C:/Program Files/LLVM/bin;D:/工具/bin;%UNKNOWN%;trailing%")
        );
    }
}
