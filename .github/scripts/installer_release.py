"""Publish an already-built installer; never execute downloaded artifacts."""

import hashlib
import html
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.parse import quote


VERSION = re.compile(r"v(\d+)\.(\d)-pre-alpha", re.IGNORECASE)
DOCS_ROOT = "https://rustic-game-engine.pages.dev/docs/"


def documentation_links(pr):
    """Use declared published pages without blocking installer announcements."""
    body = pr.get("body") or ""
    entries = re.findall(r"^Documentation:[ \t]*(.*)$", body, re.MULTILINE)
    if not entries:
        # Older PRs use ordinary Markdown links rather than release metadata.
        entries = [body]
    if len(entries) == 1 and entries[0].strip().lower() == "none":
        return []
    urls = []
    for entry in entries:
        found = re.findall(re.escape(DOCS_ROOT) + r"[A-Za-z0-9_./%~-]+(?:#[A-Za-z0-9_.%~-]+)?", entry)
        urls.extend(found)
    if not urls:
        print("::warning::No published documentation page was specified; announcing the installer "
              "without a documentation link. Add Documentation: <published docs URL> to the "
              "merged PR description and rerun publication to update the announcement.")
    return list(dict.fromkeys(urls))


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


def graphql(query, **variables):
    with tempfile.TemporaryDirectory() as temp:
        payload = Path(temp) / "graphql.json"
        payload.write_text(json.dumps({"query": query, "variables": variables}), encoding="utf-8")
        result = json.loads(gh("api", "graphql", "--input", str(payload)))
    if result.get("errors"):
        raise RuntimeError(f"Discussion API failed: {result['errors']}")
    return result["data"]


def discussion_body(repository, tag, pr, installers, files):
    root = f"https://github.com/{repository}"
    compare_url = f"{root}/compare/{pr['base']['sha']}...{pr['merge_commit_sha']}"
    # A short plain-text overview from the merged PR; no copied Markdown body.
    summary = html.escape(" ".join(pr["title"].split())[:240])
    summary = re.sub(r"([\\`*_{}\[\]<>#|~])", r"\\\1", summary)
    lines = [f"<!-- installer-release:{tag} -->", summary, ""]
    for installer in installers:
        url = f"{root}/releases/download/{quote(tag, safe='')}/{quote(installer.name, safe='')}"
        lines.append(f"[Download {installer.name}]({url})")
    for url in documentation_links(pr):
        lines.append(f"[Updated documentation]({url})")
    lines += ["", f"[Release]({root}/releases/tag/{quote(tag, safe='')}) · "
              f"[All changed files and full diff]({compare_url}) · "
              f"[Download patch]({compare_url}.patch)", "", "## Files changed", ""]
    # Keep well below GitHub's body limit, even for very large PRs.
    for file in files:
        name = file["filename"]
        anchor = hashlib.sha256(name.encode("utf-8")).hexdigest()
        label = html.escape(name).replace("`", "&#96;")
        entry = [f"- [`{label}`]({compare_url}#diff-{anchor}) — "
                 f"{file['status']}, +{file['additions']} / -{file['deletions']}"]
        if file.get("previous_filename"):
            entry.append("  Renamed from: " + html.escape(file["previous_filename"]))
        patch = file.get("patch")
        if patch:
            preview = patch[:1200]
            # An HTML pre block prevents source content from closing a Markdown fence.
            entry += ["", "<details><summary>Diff preview</summary>", "",
                      "<pre>" + html.escape(preview) + "</pre>", "",
                      "See the file link for the complete diff.", "</details>", ""]
        else:
            entry += ["  Diff preview unavailable (binary or omitted by GitHub); see the file link.", ""]
        if len(("\n".join(lines + entry)).encode("utf-8")) > 50000:
            lines.append("More files are included in the full diff linked above.")
            break
        lines.extend(entry)
    return "\n".join(lines)


def announce(repository, tag, pr, installers):
    owner, name = repository.split("/")
    data = graphql("""query($owner:String!, $name:String!) {
      repository(owner:$owner, name:$name) {
        id discussionCategories(first:100) { nodes { id slug } }
      }
    }""", owner=owner, name=name)["repository"]
    categories = [c for c in data["discussionCategories"]["nodes"] if c["slug"] == "change-log"]
    if len(categories) != 1:
        raise ValueError("The change-log Discussion category is missing or inaccessible.")
    category = categories[0]["id"]
    marker = f"<!-- installer-release:{tag} -->"
    cursor = ""
    discussion_id = None
    while True:
        page = graphql("""query($owner:String!, $name:String!, $category:ID!, $cursor:String) {
          repository(owner:$owner, name:$name) {
            discussions(first:100, categoryId:$category, after:$cursor) {
              nodes { id body labels(first:100) { nodes { id } } }
              pageInfo { hasNextPage endCursor }
            }
          }
        }""", owner=owner, name=name, category=category,
            **({"cursor": cursor} if cursor else {}))["repository"]["discussions"]
        matches = [d for d in page["nodes"] if d["body"].startswith(marker + "\n")]
        if matches:
            discussion_id = matches[0]["id"]
            existing_labels = {label["id"] for label in matches[0]["labels"]["nodes"]}
            break
        if not page["pageInfo"]["hasNextPage"]:
            break
        cursor = page["pageInfo"]["endCursor"]
    files = api_list(f"repos/{repository}/pulls/{pr['number']}/files?per_page=100")
    body = discussion_body(repository, tag, pr, installers, files)
    title = f"{tag} — Windows installer"
    if discussion_id:
        result = graphql("""mutation($id:ID!, $title:String!, $body:String!) {
          updateDiscussion(input:{discussionId:$id, title:$title, body:$body}) {
            discussion { id url }
          }
        }""", id=discussion_id, title=title, body=body)["updateDiscussion"]
    else:
        result = graphql("""mutation($repo:ID!, $category:ID!, $title:String!, $body:String!) {
          createDiscussion(input:{repositoryId:$repo, categoryId:$category, title:$title, body:$body}) {
            discussion { id url }
          }
        }""", repo=data["id"], category=category, title=title, body=body)["createDiscussion"]
    discussion = result["discussion"]
    wanted_labels = {label["node_id"] for label in pr.get("labels", [])}
    if not discussion_id:
        existing_labels = set()
    # Apply additions first so a failed retry retains labels already attached.
    for operation, labels in (("addLabelsToLabelable", wanted_labels - existing_labels),
                              ("removeLabelsFromLabelable", existing_labels - wanted_labels)):
        if labels:
            graphql(f"""mutation($id:ID!, $labels:[ID!]!) {{
              {operation}(input:{{labelableId:$id, labelIds:$labels}}) {{ clientMutationId }}
            }}""", id=discussion["id"], labels=sorted(labels))
    print(f"Announced {tag}: {discussion['url']}")


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
    announce(repository, tag, pulls[0], installers)


if __name__ == "__main__":
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    publish(os.environ["GITHUB_REPOSITORY"], event["workflow_run"],
            os.environ["INSTALLER_DIRECTORY"])
