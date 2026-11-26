"""Regression tests for fail-closed, redacted pre-push protection (no network)."""
import os
from pathlib import Path
import secrets
import shutil
import string
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("gitleaks"), "Gitleaks must be installed")
class SecretChecks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gtuim-secret-checks-")
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        (self.repo / "scripts").mkdir()
        (self.repo / ".githooks").mkdir()
        for relative in ("scripts/check-secrets.sh", ".githooks/pre-push"):
            shutil.copyfile(ROOT / relative, self.repo / relative)
        self.run_git("init", "-q")
        self.run_git("config", "user.name", "Scanner Test")
        self.run_git("config", "user.email", "scanner@example.invalid")
        self.run_git("add", ".")
        self.run_git("commit", "-qm", "Clean fixture")

    def run_git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, text=True).strip()

    def scan(self, *, hook_input=None, env=None):
        script = ".githooks/pre-push" if hook_input is not None else "scripts/check-secrets.sh"
        return subprocess.run(
            ["bash", script], cwd=self.repo, input=hook_input,
            env=env, text=True, capture_output=True,
        )

    def synthetic_secret_commit(self, allow=False):
        # Deliberately invalid, randomly generated test credential; never sent anywhere.
        token = "gh" + "p_" + "".join(
            secrets.choice(string.ascii_letters + string.digits) for _ in range(36)
        )
        suffix = " # gitleaks:allow" if allow else ""
        (self.repo / "fixture.txt").write_text('github_token = "' + token + '"' + suffix + "\n")
        self.run_git("add", "fixture.txt")
        self.run_git("commit", "-qm", "Synthetic fixture")
        return self.run_git("rev-parse", "HEAD"), token

    def test_clean_history_passes(self):
        self.assertEqual(self.scan().returncode, 0)

    def test_missing_scanner_blocks(self):
        result = self.scan(env={**os.environ, "GITLEAKS_BIN": "/nonexistent/gitleaks"})
        self.assertNotEqual(result.returncode, 0)

    def test_inline_allow_does_not_bypass_redacted_scan(self):
        _, token = self.synthetic_secret_commit(allow=True)
        result = self.scan()
        self.assertEqual(result.returncode, 1)
        self.assertTrue(token not in result.stdout + result.stderr, "Secret output was not redacted")

    def test_hook_scans_unreferenced_outgoing_commit(self):
        oid, token = self.synthetic_secret_commit()
        self.run_git("reset", "--hard", "HEAD^")
        self.assertEqual(self.scan().returncode, 0)
        result = self.scan(hook_input=f"{oid} {oid} refs/heads/test {'0' * 40}\n")
        self.assertEqual(result.returncode, 1)
        self.assertTrue(token not in result.stdout + result.stderr, "Secret output was not redacted")

    def test_ref_deletion_passes(self):
        result = self.scan(hook_input=f"(delete) {'0' * 40} refs/heads/test {'a' * 40}\n")
        self.assertEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
