import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "installer_release", Path(__file__).parents[1] / "scripts/installer_release.py"
)
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)

REPOSITORY = "example/engine"
SHA = "a" * 40


class InstallerReleaseTests(unittest.TestCase):
    def setUp(self):
        self.run = {
            "conclusion": "success", "event": "push", "head_branch": "main",
            "head_repository": {"full_name": REPOSITORY}, "head_sha": SHA,
        }
        self.pr = {
            "merged_at": "2026-10-10T00:00:00Z", "merge_commit_sha": SHA,
            "base": {"ref": "main", "repo": {"full_name": REPOSITORY}},
            "body": '## Changes\n\nLiteral `code`, $(text), "quotes" and Māori.\n',
        }
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.installer = Path(self.temp.name) / "RusticGameEngine-Setup-0.1.exe"
        self.installer.write_bytes(b"installer fixture")

    def test_version_sequence_and_rollover(self):
        cases = [
            ([], "v0.2-Pre-Alpha"),
            (["v0.1-pre-alpha"], "v0.2-Pre-Alpha"),
            (["v0.2-Pre-Alpha", "unrelated", "v0.1-pre-alpha"], "v0.3-Pre-Alpha"),
            (["v0.9-Pre-Alpha"], "v1.0-Pre-Alpha"),
            (["v1.9-Pre-Alpha", "v0.2-Pre-Alpha"], "v2.0-Pre-Alpha"),
        ]
        for tags, expected in cases:
            with self.subTest(tags=tags):
                self.assertEqual(release.next_tag([{"tag_name": t} for t in tags]), expected)

    def publish_with_mocks(self, releases=(), pulls=None, fail_upload=False):
        calls = []

        def gh(*args):
            calls.append(args)
            if "--notes-file" in args:
                path = args[args.index("--notes-file") + 1]
                self.assertEqual(Path(path).read_text(encoding="utf-8"), self.pr["body"] or "")
            if fail_upload and args[:2] == ("release", "upload"):
                raise RuntimeError("upload failed")
            return ""

        with patch.object(release, "api_list", side_effect=[
            [self.pr] if pulls is None else pulls, list(releases)
        ]), patch.object(release, "gh", side_effect=gh), \
                patch.object(release, "announce") as announce:
            release.publish(REPOSITORY, self.run, self.temp.name)
            announce.assert_called_once_with(
                REPOSITORY, calls[0][2], self.pr, [self.installer]
            )
        return calls

    def test_new_release_uses_exact_pr_body_commit_and_installer(self):
        calls = self.publish_with_mocks()
        create, upload, publish = calls
        self.assertEqual(create[:3], ("release", "create", "v0.2-Pre-Alpha"))
        self.assertEqual(create[create.index("--target") + 1], SHA)
        self.assertIn("--draft", create)
        self.assertIn("--prerelease", create)
        self.assertIn(str(self.installer), upload)
        self.assertEqual(publish[-1], "--draft=false")

    def test_empty_description_remains_empty(self):
        self.pr["body"] = None
        self.publish_with_mocks()

    def test_retry_reuses_release_and_repairs_assets(self):
        for draft in (True, False):
            with self.subTest(draft=draft):
                calls = self.publish_with_mocks([
                    {"tag_name": "v0.3-Pre-Alpha", "target_commitish": SHA, "draft": draft},
                    {"tag_name": "v0.9-Pre-Alpha", "target_commitish": "b" * 40},
                ])
                self.assertEqual(calls[0][:3], ("release", "edit", "v0.3-Pre-Alpha"))
                self.assertIn("--clobber", calls[1])
                self.assertFalse(any(call[:2] == ("release", "create") for call in calls))
                self.assertEqual(len(calls), 3 if draft else 2)

    def test_upload_failure_leaves_draft_unpublished(self):
        with patch.object(release, "api_list", side_effect=[[self.pr], []]), \
                patch.object(release, "gh", side_effect=["", RuntimeError("upload failed")]) as gh, \
                patch.object(release, "announce") as announce:
            with self.assertRaisesRegex(RuntimeError, "upload failed"):
                release.publish(REPOSITORY, self.run, self.temp.name)
            self.assertEqual(gh.call_count, 2)
            self.assertIn("--draft", gh.call_args_list[0].args)
            announce.assert_not_called()

    def test_untrusted_or_unsuccessful_runs_cannot_publish(self):
        changes = [
            {"event": "pull_request"}, {"event": "merge_group"},
            {"conclusion": "failure"}, {"head_branch": "feature"},
            {"head_repository": {"full_name": "fork/engine"}},
        ]
        for change in changes:
            with self.subTest(change=change), patch.object(release, "gh") as gh:
                with self.assertRaises(ValueError):
                    release.publish(REPOSITORY, self.run | change, self.temp.name)
                gh.assert_not_called()

    def test_missing_or_empty_installer_cannot_create_release(self):
        self.installer.write_bytes(b"")
        for missing in (False, True):
            if missing:
                self.installer.unlink()
            with patch.object(release, "api_list") as api:
                with self.assertRaisesRegex(ValueError, "installer"):
                    release.publish(REPOSITORY, self.run, self.temp.name)
                api.assert_not_called()

    def test_requires_exact_merged_pr_on_build_branch(self):
        invalid_pulls = [[], [self.pr, self.pr],
                         [self.pr | {"merged_at": None}],
                         [self.pr | {"merge_commit_sha": "b" * 40}],
                         [self.pr | {"base": {"ref": "work", "repo": {"full_name": REPOSITORY}}}]]
        for pulls in invalid_pulls:
            with self.subTest(pulls=pulls), patch.object(release, "gh") as gh:
                with self.assertRaisesRegex(ValueError, "merged pull request"):
                    self.publish_with_mocks(pulls=pulls)
                gh.assert_not_called()

    def test_api_list_reads_all_pages(self):
        with patch.object(release, "gh", return_value='[[{"tag_name":"v0.9-Pre-Alpha"}],[]]') as gh:
            self.assertEqual(release.api_list("repos/example/engine/releases"),
                             [{"tag_name": "v0.9-Pre-Alpha"}])
            gh.assert_called_once_with("api", "repos/example/engine/releases", "--paginate", "--slurp")


class DiscussionTests(unittest.TestCase):
    def setUp(self):
        self.pr = {"number": 42, "title": "Fix editor startup"}
        self.installers = [Path("RusticGameEngine-Setup-0.1.exe")]
        self.file = {
            "filename": "Engine/main.rs", "status": "modified",
            "additions": 2, "deletions": 1, "patch": "@@ -1 +1 @@\n-old\n+new",
        }
        self.repository = {"repository": {
            "id": "repo", "discussionCategories": {
                "nodes": [{"id": "category", "slug": "change-log"}]
            },
        }}

    def page(self, nodes=(), more=False, cursor=None):
        return {"repository": {"discussions": {
            "nodes": list(nodes), "pageInfo": {"hasNextPage": more, "endCursor": cursor}
        }}}

    def test_body_includes_download_overview_files_and_diffs(self):
        body = release.discussion_body(REPOSITORY, "v0.2-Pre-Alpha", self.pr,
                                       self.installers, [self.file])
        for text in ["Fix editor startup", "/releases/download/v0.2-Pre-Alpha/",
                     self.installers[0].name, "Engine/main.rs", "+2 / -1", "+new",
                     "/pull/42/files#diff-", "/pull/42.patch"]:
            self.assertIn(text, body)

    def test_large_and_binary_diffs_have_full_diff_links(self):
        files = [self.file | {"filename": f"file-{i}", "patch": "x" * 10000}
                 for i in range(100)]
        files.insert(0, self.file | {"filename": "image.png", "patch": None})
        body = release.discussion_body(REPOSITORY, "tag", self.pr, self.installers, files)
        self.assertLess(len(body.encode("utf-8")), 51000)
        self.assertIn("binary or omitted", body)
        self.assertIn("More files", body)
        self.assertIn("/pull/42/files", body)

    def test_source_cannot_close_preview_or_inject_html(self):
        file = self.file | {"patch": "+</pre><script>alert(1)</script>\n+```"}
        body = release.discussion_body(REPOSITORY, "tag", self.pr, self.installers, [file])
        self.assertNotIn("<script>", body)
        self.assertIn("&lt;/pre&gt;", body)

    def test_create_uses_category_and_retry_updates_existing_on_later_page(self):
        for existing in (False, True):
            with self.subTest(existing=existing):
                pages = [self.page()]
                operation = "createDiscussion"
                if existing:
                    pages = [self.page(more=True, cursor="next"), self.page([{
                        "id": "discussion", "body": "<!-- installer-release:tag -->\nold"
                    }])]
                    operation = "updateDiscussion"
                responses = [self.repository, *pages, {operation: {"discussion": {"url": "url"}}}]
                with patch.object(release, "graphql", side_effect=responses) as graphql, \
                        patch.object(release, "api_list", return_value=[self.file]) as api:
                    release.announce(REPOSITORY, "tag", self.pr, self.installers)
                api.assert_called_once_with(f"repos/{REPOSITORY}/pulls/42/files?per_page=100")
                mutation = graphql.call_args
                self.assertIn(operation, mutation.args[0])
                self.assertIn("Fix editor startup", mutation.kwargs["body"])
                if existing:
                    self.assertEqual(mutation.kwargs["id"], "discussion")
                    self.assertEqual(graphql.call_args_list[2].kwargs["cursor"], "next")
                else:
                    self.assertEqual(mutation.kwargs["category"], "category")

    def test_missing_category_fails_clearly(self):
        self.repository["repository"]["discussionCategories"]["nodes"] = []
        with patch.object(release, "graphql", return_value=self.repository):
            with self.assertRaisesRegex(ValueError, "change-log"):
                release.announce(REPOSITORY, "tag", self.pr, self.installers)

    def test_graphql_reports_api_errors(self):
        with patch.object(release, "gh", return_value='{"errors":[{"message":"denied"}]}'):
            with self.assertRaisesRegex(RuntimeError, "denied"):
                release.graphql("query {}")

    def test_announcement_failure_keeps_release_published_for_retry(self):
        fixture = InstallerReleaseTests()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        with patch.object(release, "api_list", side_effect=[[fixture.pr], []]), \
                patch.object(release, "gh", return_value="") as gh, \
                patch.object(release, "announce", side_effect=RuntimeError("denied")):
            with self.assertRaisesRegex(RuntimeError, "denied"):
                release.publish(REPOSITORY, fixture.run, fixture.temp.name)
        self.assertEqual(gh.call_args.args[-1], "--draft=false")


if __name__ == "__main__":
    unittest.main()
