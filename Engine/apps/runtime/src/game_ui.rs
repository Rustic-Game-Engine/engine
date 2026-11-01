use resvg::{tiny_skia, usvg};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct GameUi {
    tree: Option<usvg::Tree>,
}

impl GameUi {
    pub fn load(root: &Path) -> Result<Self, String> {
        let directory = root.join("ui");
        if !directory.is_dir() {
            return Ok(Self { tree: None });
        }
        let mut files = collect_files(&directory)?;
        files.sort();
        // PHP behavior assets run through the gameplay host, which supplies the
        // SDK and callback protocol. Running them again as pages bypasses that
        // host and can fail on SDK imports or emit protocol JSON into the UI.
        let php_behaviors = php_behavior_paths(root)?;
        let css = files
            .iter()
            .filter(|p| extension(p) == "css")
            .map(std::fs::read_to_string)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("could not read game UI stylesheet: {e}"))?
            .join("\n");
        let mut html = String::new();
        for path in files {
            match extension(&path).as_str() {
                "html" | "htm" => html.push_str(
                    &std::fs::read_to_string(&path)
                        .map_err(|e| format!("could not read {}: {e}", path.display()))?,
                ),
                "php" if !php_behaviors.contains(&path) => html.push_str(&render_php(&path)?),
                _ => {}
            }
        }
        if html.trim().is_empty() {
            return Ok(Self { tree: None });
        }
        let svg = html_to_svg(&html, &css);
        let mut options = usvg::Options::default();
        let mut fonts = usvg::fontdb::Database::new();
        #[cfg(not(target_os = "emscripten"))]
        fonts.load_system_fonts();
        #[cfg(target_os = "emscripten")]
        {
            fonts.load_font_data(epaint_default_fonts::UBUNTU_LIGHT.to_vec());
            fonts.set_serif_family("Ubuntu");
            fonts.set_sans_serif_family("Ubuntu");
            options.font_family = "Ubuntu".into();
        }
        options.fontdb = std::sync::Arc::new(fonts);
        let tree = usvg::Tree::from_str(&svg, &options)
            .map_err(|e| format!("could not layout game UI: {e}"))?;
        Ok(Self { tree: Some(tree) })
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "screen dimensions are bounded by GPU texture limits and converted to float layout coordinates"
    )]
    pub fn composite_rgba(&self, width: u32, height: u32, target: &mut [u8]) -> Result<(), String> {
        let Some(tree) = &self.tree else {
            return Ok(());
        };
        let mut overlay = tiny_skia::Pixmap::new(width, height).ok_or("invalid game UI size")?;
        resvg::render(
            tree,
            tiny_skia::Transform::from_scale(width as f32 / 640.0, height as f32 / 360.0),
            &mut overlay.as_mut(),
        );
        for (dst, src) in target
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(overlay.data().as_chunks::<4>().0.iter())
        {
            let a = u16::from(src[3]);
            for channel in 0..3 {
                dst[channel] = u8::try_from(
                    (u16::from(src[channel]) * a + u16::from(dst[channel]) * (255 - a)) / 255,
                )
                .unwrap_or(u8::MAX);
            }
        }
        Ok(())
    }
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() && !kind.is_symlink() {
                pending.push(entry.path());
            } else if kind.is_file() {
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn php_behavior_paths(root: &Path) -> Result<HashSet<PathBuf>, String> {
    let path = root.join("config/scripts.ron");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    let loaded = engine_scripting::load_manifest(&bytes)
        .map_err(|error| format!("could not load {}: {error}", path.display()))?;
    Ok(loaded
        .manifest
        .scripts
        .into_iter()
        .filter(|entry| entry.language == engine_scripting::ScriptLanguage::Php)
        .map(|entry| root.join(entry.relative_path))
        .collect())
}

fn render_php(path: &Path) -> Result<String, String> {
    // Resolve before changing cwd so relative snapshot roots do not repeat ui/.
    let path = std::path::absolute(path)
        .map_err(|e| format!("could not resolve PHP UI {}: {e}", path.display()))?;
    let output = Command::new("php")
        .args(["-d", "display_errors=stderr"])
        .arg(&path)
        .current_dir(path.parent().unwrap_or(Path::new(".")))
        .output()
        .map_err(|e| format!("could not render {} (PHP is required): {e}", path.display()))?;
    if !output.status.success() {
        return Err(php_failure(&path, &output));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("PHP UI output is not UTF-8: {e}"))
}

fn php_failure(path: &Path, output: &std::process::Output) -> String {
    let mut message = format!("PHP UI {} failed ({}).", path.display(), output.status);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stderr.trim().is_empty() {
        let _ = write!(message, "\nstderr: {}", stderr.trim());
    }
    if !stdout.trim().is_empty() {
        let _ = write!(message, "\nstdout: {}", stdout.trim());
    }
    if stderr.trim().is_empty() && stdout.trim().is_empty() {
        message.push_str(
            " PHP produced no diagnostic output; check the page and PHP CLI configuration.",
        );
    }
    message
}

fn html_to_svg(html: &str, css: &str) -> String {
    let html = without_blocks(html, "script");
    let html = without_blocks(&html, "style");
    let background = css_value(css, "background-color")
        .or_else(|| css_value(css, "background"))
        .unwrap_or("none");
    let color = css_value(css, "color").unwrap_or("white");
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><rect width="640" height="360" fill="{}"/>"#,
        escape(background)
    );
    let mut y = 32;
    let mut rest = html.as_str();
    while let Some(start) = rest.find('<') {
        let text = strip_tags(&rest[..start]);
        rest = &rest[start + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = rest[..end].trim().to_ascii_lowercase();
        rest = &rest[end + 1..];
        if !text.trim().is_empty() {
            let _ = write!(
                svg,
                r#"<text x="20" y="{}" fill="{}" font-family="sans-serif" font-size="18">{}</text>"#,
                y,
                escape(color),
                escape(text.trim())
            );
            y += 28;
        }
        if tag.starts_with("button") || tag.starts_with("input") {
            let _ = write!(
                svg,
                r##"<rect x="16" y="{}" width="180" height="32" rx="5" fill="#334155" stroke="#94a3b8"/>"##,
                y - 23
            );
        }
    }
    let tail = strip_tags(rest);
    if !tail.trim().is_empty() {
        let _ = write!(
            svg,
            r#"<text x="20" y="{}" fill="{}" font-family="sans-serif" font-size="18">{}</text>"#,
            y,
            escape(color),
            escape(tail.trim())
        );
    }
    svg.push_str("</svg>");
    svg
}
fn without_blocks(value: &str, tag: &str) -> String {
    let mut output = value.to_owned();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    while let Some(start) = output.to_ascii_lowercase().find(&open) {
        let lower = output.to_ascii_lowercase();
        let end = lower[start..]
            .find(&close)
            .map_or(output.len(), |offset| start + offset + close.len());
        output.replace_range(start..end, "");
    }
    output
}
fn css_value<'a>(css: &'a str, name: &str) -> Option<&'a str> {
    let pos = css.find(name)? + name.len();
    let rest = &css[pos..];
    let colon = rest.find(':')?;
    Some(rest[colon + 1..].split([';', '}']).next()?.trim())
}
fn strip_tags(value: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in value.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}
fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_scripting::{
        ScriptApiVersion, ScriptId, ScriptLanguage, ScriptManifest, ScriptManifestEntry,
        save_manifest_atomic,
    };

    #[test]
    fn registered_php_behaviors_are_not_executed_as_ui_pages() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("ui/nested")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        let mut manifest = ScriptManifest::default();
        // Include unused/disabled behaviors as well: registration identifies the
        // asset's role even when no instance currently runs it.
        for relative_path in ["ui/hardware.php", "ui/nested/unused.PHP"] {
            std::fs::write(
                root.join(relative_path),
                "<?php require __DIR__ . '/rustic.php'; exit(99);",
            )
            .unwrap();
            manifest.scripts.push(ScriptManifestEntry {
                id: ScriptId::new(),
                language: ScriptLanguage::Php,
                relative_path: relative_path.into(),
                api_version: ScriptApiVersion::CURRENT,
                public_properties: Vec::new(),
            });
        }
        save_manifest_atomic(&root.join("config/scripts.ron"), &manifest).unwrap();
        assert!(GameUi::load(root).unwrap().tree.is_none());
        std::fs::write(root.join("ui/index.html"), "<p>Game HUD</p>").unwrap();
        assert!(GameUi::load(root).unwrap().tree.is_some());
    }

    #[test]
    fn php_failure_includes_status_and_both_output_streams() {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;

        let mut output = std::process::Output {
            status: std::process::ExitStatus::from_raw(256),
            stdout: b"Fatal error: missing SDK".to_vec(),
            stderr: Vec::new(),
        };
        let message = php_failure(Path::new("ui/hardware.php"), &output);
        assert!(message.contains("ui/hardware.php"));
        assert!(message.contains(&output.status.to_string()));
        assert!(message.contains("stdout: Fatal error: missing SDK"));
        output.stderr = b"PHP startup error".to_vec();
        let message = php_failure(Path::new("ui/hardware.php"), &output);
        assert!(message.contains("stderr: PHP startup error"));
        assert!(message.contains("stdout: Fatal error: missing SDK"));
        output.stdout.clear();
        output.stderr.clear();
        assert!(
            php_failure(Path::new("ui/hardware.php"), &output)
                .contains("PHP produced no diagnostic output")
        );
    }

    #[test]
    fn standalone_php_pages_render_and_report_errors() {
        if !Command::new("php")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
        {
            eprintln!("skipping PHP page integration check: PHP CLI is unavailable");
            return;
        }
        // Keep the path relative and include spaces to exercise command/cwd handling.
        let directory = tempfile::Builder::new()
            .prefix("php ui ")
            .tempdir_in(".")
            .unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("ui")).unwrap();
        let page = root.join("ui/index.php");
        std::fs::write(&page, "<?php echo '<p>PHP HUD</p>';").unwrap();
        assert_eq!(render_php(&page).unwrap(), "<p>PHP HUD</p>");
        assert!(GameUi::load(root).unwrap().tree.is_some());
        std::fs::write(&page, "<?php echo 'stdout failure'; exit(23);").unwrap();
        assert!(render_php(&page).unwrap_err().contains("stdout failure"));
        std::fs::write(&page, "<?php missing_ui_function();").unwrap();
        assert!(
            render_php(&page)
                .unwrap_err()
                .contains("missing_ui_function")
        );
        std::fs::write(&page, "<?php exit(23);").unwrap();
        let error = render_php(&page).unwrap_err();
        assert!(error.contains("23"), "{error}");
        assert!(error.contains("PHP produced no diagnostic output"));
    }
}
