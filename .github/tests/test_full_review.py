import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT_ROOT = Path(__file__).parents[1] / "scripts"
sys.path.insert(0, str(SCRIPT_ROOT))
spec = importlib.util.spec_from_file_location("full_review", SCRIPT_ROOT / "full_review.py")
full = importlib.util.module_from_spec(spec)
spec.loader.exec_module(full)


def evidence(state="success"):
    return {"state": state, "jobs": [{"name": "Windows installer", "conclusion": "success", "status": "completed", "failed_steps": []}]}


class FullReviewTests(unittest.TestCase):
    def test_complete_source_line_coverage_survives_batch_splitting(self):
        text = "\n".join("x" * 100 for _ in range(200))
        with patch.object(full, "MAX_BATCH_BYTES", 2000):
            batches = full.partition({"Engine/unchanged.rs": text})
        actual = []
        for batch in batches:
            for item in batch:
                actual.extend(int(line.split(":", 1)[0]) for line in item["source"].splitlines())
        self.assertEqual(actual, list(range(1, 201)))
        self.assertGreater(len(batches), 1)

    def test_sweep_does_not_filter_out_unchanged_source(self):
        self.assertFalse(full.generated("Engine/crates/engine-play/src/lib.rs"))
        self.assertTrue(full.generated("Engine/target-codex-check/debug/app.exe"))
        self.assertTrue(full.generated("Website/.next/types/routes.d.ts"))

    def test_ci_failures_and_pending_evidence_cannot_become_green(self):
        clean = {"findings": []}
        for state in ("success", "failure", "pending"):
            self.assertEqual(full.final_state(clean, evidence(state)), state)
        self.assertEqual(full.final_state({"findings": [{"severity": "high"}]}, evidence("success")), "failure")

    @patch.object(full, "github")
    def test_ci_evidence_requires_exact_commit(self, github):
        github.return_value = {"head_sha": "b" * 40}
        with self.assertRaisesRegex(RuntimeError, "different commit"):
            full.ci_evidence("a" * 40, 1)

    @patch.object(full, "github")
    def test_actual_installer_result_is_preserved_in_ci_evidence(self, github):
        github.side_effect = [
            {"id": 1, "head_sha": "a" * 40, "status": "completed", "conclusion": "success", "html_url": "https://github.com/run/1"},
            {"total_count": 1, "jobs": [{"name": "Windows installer", "status": "completed", "conclusion": "success", "html_url": "https://github.com/job/1", "steps": []}]},
        ]
        data = full.ci_evidence("a" * 40, 1)
        self.assertEqual(data["state"], "success")
        self.assertEqual(data["jobs"][0]["conclusion"], "success")

    @patch.dict(os.environ, {"GH_TOKEN": "test-token"})
    @patch.object(full.ai, "git")
    def test_inventory_reads_blobs_without_checking_out_or_executing_sources(self, git):
        git.side_effect = [
            b"100644 blob " + b"a" * 40 + b"\tEngine/source.rs\0" +
            b"100644 blob " + b"b" * 40 + b"\tEngine/target/debug/app\0" +
            b"100644 blob " + b"c" * 40 + b"\timage.png\0",
            b"", b"content\n",
        ]
        sources, coverage = full.collect_sources("d" * 40)
        self.assertEqual(sources, {"Engine/source.rs": "content\n"})
        self.assertEqual(coverage["generated_files_excluded"], 1)
        self.assertEqual(coverage["omitted_files"], [{"file": "image.png", "reason": "binary asset"}])
        self.assertEqual([call.args[0] for call in git.call_args_list], ["ls-tree", "fetch", "cat-file"])
        self.assertNotIn("-l", git.call_args_list[0].args)
        self.assertNotIn("b" * 40, git.call_args_list[1].args)
        self.assertNotIn("b" * 40, git.call_args_list[1].kwargs["input_bytes"].decode())
        self.assertIn("a" * 40, git.call_args_list[1].kwargs["input_bytes"].decode())

    @patch.object(full.ai, "review")
    def test_sweep_uses_high_effort_full_sources_and_allows_existing_findings(self, review):
        review.return_value = {"summary": "Concern", "findings": [{"file": "unchanged.rs", "line": 1, "severity": "medium", "title": "Existing bug", "detail": "Impact", "recommendation": "Fix"}]}
        shared = {"shared_complete_files": [], "exact_commit_ci": evidence()}
        with tempfile.TemporaryDirectory() as directory, patch.object(full, "Path") as path:
            coverage = {}
            result = full.sweep({"unchanged.rs": "let x = 1;"}, shared, coverage)
        self.assertEqual(result["findings"][0]["file"], "unchanged.rs")
        self.assertEqual(review.call_args.kwargs["effort"], "high")
        payload = json.loads(review.call_args.args[0])
        self.assertIn("source_segments", payload)
        self.assertIn("exact_commit_ci", payload["context"])
        self.assertIn("unchanged code", review.call_args.kwargs["instructions"])

    @patch.object(full.ai, "review")
    def test_incomplete_batch_cannot_be_recorded_as_complete(self, review):
        review.side_effect = RuntimeError("Incomplete API response")
        with patch.object(full, "Path"), self.assertRaisesRegex(RuntimeError, "Incomplete"):
            coverage = {}
            full.sweep({"source.rs": "content"}, {"shared_complete_files": []}, coverage)
        self.assertEqual(coverage["batches_completed"], 0)

    @patch.object(full.ai, "review")
    def test_finding_line_must_exist_in_supplied_file(self, review):
        review.return_value = {"findings": [{"file": "source.rs", "line": 99}]}
        with patch.object(full, "Path"), self.assertRaisesRegex(RuntimeError, "outside"):
            full.sweep({"source.rs": "content"}, {"shared_complete_files": []}, {})

    def test_sensitive_files_are_not_sent_to_model(self):
        for path in (".env", "Website/.env.local", "key.pem"):
            self.assertTrue(full.sensitive(path))
        self.assertFalse(full.sensitive(".env.example"))

    @patch.object(full, "github")
    def test_manual_pr_sweep_targets_actual_pr_head(self, github):
        github.return_value = {"number": 3, "base": {"sha": "a" * 40}, "head": {"sha": "b" * 40}}
        base, head, pr_event, run_id = full.resolve_target({"inputs": {"pr_number": "3"}}, "workflow_dispatch")
        self.assertEqual((base, head), ("a" * 40, "b" * 40))
        self.assertEqual(pr_event["pull_request"]["number"], 3)
        self.assertIsNone(run_id)


if __name__ == "__main__":
    unittest.main()
