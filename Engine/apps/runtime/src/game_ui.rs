use resvg::{tiny_skia, usvg};
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
                "php" => html.push_str(&render_php(&path)?),
                _ => {}
            }
        }
        if html.trim().is_empty() {
            return Ok(Self { tree: None });
        }
        let svg = html_to_svg(&html, &css);
        let mut options = usvg::Options::default();
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        options.fontdb = std::sync::Arc::new(fonts);
        let tree = usvg::Tree::from_str(&svg, &options)
            .map_err(|e| format!("could not layout game UI: {e}"))?;
        Ok(Self { tree: Some(tree) })
    }

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
            .chunks_exact_mut(4)
            .zip(overlay.data().chunks_exact(4))
        {
            let a = u16::from(src[3]);
            for channel in 0..3 {
                dst[channel] = ((u16::from(src[channel]) * a + u16::from(dst[channel]) * (255 - a))
                    / 255) as u8;
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
fn render_php(path: &Path) -> Result<String, String> {
    let output = Command::new("php")
        .arg(path)
        .current_dir(path.parent().unwrap_or(Path::new(".")))
        .output()
        .map_err(|e| format!("could not render {} (PHP is required): {e}", path.display()))?;
    if !output.status.success() {
        return Err(format!(
            "PHP UI {} failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("PHP UI output is not UTF-8: {e}"))
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
            svg.push_str(&format!(r#"<text x="20" y="{}" fill="{}" font-family="sans-serif" font-size="18">{}</text>"#,y,escape(color),escape(text.trim())));
            y += 28;
        }
        if tag.starts_with("button") || tag.starts_with("input") {
            svg.push_str(&format!(r##"<rect x="16" y="{}" width="180" height="32" rx="5" fill="#334155" stroke="#94a3b8"/>"##,y-23));
        }
    }
    let tail = strip_tags(rest);
    if !tail.trim().is_empty() {
        svg.push_str(&format!(
            r#"<text x="20" y="{}" fill="{}" font-family="sans-serif" font-size="18">{}</text>"#,
            y,
            escape(color),
            escape(tail.trim())
        ));
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
