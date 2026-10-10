"""Publish an already-built installer; never execute downloaded artifacts."""

import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


VERSION = re.compile(r"v(\d+)\.(\d)-pre-alpha", re.IGNORECASE)


def gh(*args):
    return subprocess.run(
        ["gh", *args], check=True, text=True, capture_output=True
    ).stdout


def api_list(endpoint):
    pages = json.loads(gh("api", endpoint, "--paginate", "--slurp"))
    return [item for page in pages for item in page]


def next_tag(releases):
    versions = []
    for release in releases:
        match = VERSION.fullmatch(release["tag_name"])
        if match:
            versions.append(int(match[1]) * 10 + int(match[2]))
    version = max([1, *versions]) + 1
    return f"v{version // 10}.{version % 10}-Pre-Alpha"


def publish(repository, run, directory):
    if (
        run["conclusion"] != "success"
        or run["event"] not in {"push", "workflow_dispatch"}
        or run["head_branch"] not in {"main", "work"}
        or run["head_repository"]["full_name"] != repository
    ):
        raise ValueError("Only successful trusted branch builds can publish releases.")

    installers = sorted(Path(directory).rglob("RusticGameEngine-Setup-*.exe"))
    if not installers or any(not path.is_file() or path.stat().st_size == 0 for path in installers):
        raise ValueError("No nonempty Windows installer was downloaded.")

    sha = run["head_sha"]
    pulls = api_list(f"repos/{repository}/commits/{sha}/pulls?per_page=100")
    pulls = [
        pr for pr in pulls
        if pr.get("merged_at") and pr["merge_commit_sha"] == sha
        and pr["base"]["ref"] == run["head_branch"]
        and pr["base"]["repo"]["full_name"] == repository
    ]
    if len(pulls) != 1:
        raise ValueError("Build commit must match exactly one merged pull request.")
    body = pulls[0].get("body") or ""

    releases = api_list(f"repos/{repository}/releases?per_page=100")
    existing = [
        release for release in releases
        if VERSION.fullmatch(release["tag_name"])
        and release["target_commitish"] == sha
    ]
    if len(existing) > 1:
        raise ValueError("Multiple installer releases already target this commit.")
    tag = existing[0]["tag_name"] if existing else next_tag(releases)

    # Pass PR text as a file, preserving Markdown without shell interpolation.
    with tempfile.TemporaryDirectory() as temp:
        notes = Path(temp) / "release-notes.md"
        notes.write_text(body, encoding="utf-8")
        if existing:
            gh("release", "edit", tag, "--repo", repository, "--title", tag,
               "--notes-file", str(notes), "--prerelease")
        else:
            gh("release", "create", tag, "--repo", repository, "--target", sha,
               "--title", tag, "--notes-file", str(notes), "--prerelease", "--draft")
        # Retrying after a partial upload repairs the same release and version.
        gh("release", "upload", tag, *(str(path) for path in installers),
           "--repo", repository, "--clobber")
        if not existing or existing[0]["draft"]:
            gh("release", "edit", tag, "--repo", repository, "--draft=false")
    print(f"Published {tag}: https://github.com/{repository}/releases/tag/{tag}")


if __name__ == "__main__":
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    publish(os.environ["GITHUB_REPOSITORY"], event["workflow_run"],
            os.environ["INSTALLER_DIRECTORY"])
