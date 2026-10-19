"""Release CLI regressions; GitHub writes are mocked."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1]
ASSETS = (
    "FTO-Alg-Finder-windows-x64.exe",
    "FTO-Alg-Finder-linux-x86_64.AppImage",
    "FTO-Alg-Finder-linux-amd64.deb",
    "FTO-Alg-Finder-linux-x86_64.rpm",
)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="fto-release-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        shutil.copy2(SOURCE / "release.sh", self.root / "release.sh")
        (self.root / "release").symlink_to("release.sh")
        self.assets = self.root / "builds/release"
        self.assets.mkdir(parents=True)
        for name in ASSETS:
            (self.assets / name).write_bytes(name.encode())
        self.write_checksums()
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        fake_gh = bin_dir / "gh"
        fake_gh.write_text('''#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["CALL_LOG"], "a") as log: log.write(json.dumps(args) + "\\n")
if args[:2] == ["repo", "view"]: print("owner/repo")
elif args[:2] == ["release", "view"]:
    print("v1.0.0" if "tagName" in args else "https://github.com/owner/repo/releases/tag/v1.0.0.209")
elif args[:2] == ["release", "create"]:
    if os.environ.get("FAIL_UPLOAD"): sys.exit(1)
elif args[:2] == ["release", "edit"]: pass
else: sys.exit(2)
''')
        fake_gh.chmod(0o755)
        fake_git = bin_dir / "git"
        fake_git.write_text(f'''#!/bin/sh
case "$1" in
 check-ref-format) exec '{shutil.which("git")}' "$@" ;;
 rev-parse) echo 1111111111111111111111111111111111111111 ;;
 ls-remote) if [ -n "$EXISTING_TAG" ]; then echo "1111111 refs/tags/$EXISTING_TAG"; fi ;;
 *) exit 2 ;;
esac
''')
        fake_git.chmod(0o755)
        self.log = self.root / "calls.jsonl"
        self.env = {**os.environ, "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}", "CALL_LOG": str(self.log)}

    def write_checksums(self):
        (self.assets / "SHA256SUMS").write_text("".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
            for path in sorted(self.assets.iterdir()) if path.name != "SHA256SUMS"
        ))

    def run_script(self, *args, alias="release", **env):
        return subprocess.run([str(self.root / alias), *args], cwd="/tmp", env={**self.env, **env}, capture_output=True, text=True)

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def assert_no_writes(self):
        self.assertFalse(any(call[:2] in (["release", "create"], ["release", "edit"]) for call in self.calls()))

    def test_current_is_read_only_and_works_from_another_directory(self):
        result = self.run_script("--current")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), "v1.0.0")
        self.assert_no_writes()

    def test_publish_tag_uploads_verified_assets_before_marking_latest(self):
        result = self.run_script("v1.0.0.209", alias="release.sh")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls()
        create = next(call for call in calls if call[:2] == ["release", "create"])
        edit = next(call for call in calls if call[:2] == ["release", "edit"])
        self.assertEqual(create[2], "v1.0.0.209")
        self.assertIn("--draft", create)
        self.assertEqual(create[create.index("--target") + 1], "1" * 40)
        for name in (*ASSETS, "SHA256SUMS"):
            self.assertIn(str(self.assets / name), create)
        self.assertLess(calls.index(create), calls.index(edit))
        self.assertIn("--latest", edit)
        self.assertIn("--draft=false", edit)

    def test_corrupt_missing_or_unlisted_assets_prevent_upload(self):
        (self.assets / ASSETS[0]).write_bytes(b"corrupted")
        result = self.run_script("v1.0.0.209")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)
        self.assert_no_writes()
        (self.assets / ASSETS[0]).unlink()
        self.assertNotEqual(self.run_script("v1.0.0.209").returncode, 0)
        self.assert_no_writes()

    def test_failed_upload_is_never_published(self):
        self.assertNotEqual(self.run_script("v1.0.0.209", FAIL_UPLOAD="1").returncode, 0)
        self.assertTrue(any(call[:2] == ["release", "create"] for call in self.calls()))
        self.assertFalse(any(call[:2] == ["release", "edit"] for call in self.calls()))

    def test_existing_or_invalid_tags_are_not_overwritten(self):
        result = self.run_script("v1.0.0.209", EXISTING_TAG="v1.0.0.209")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already exists", result.stderr)
        self.assert_no_writes()
        self.assertNotEqual(self.run_script("invalid tag").returncode, 0)
        self.assertNotEqual(self.run_script("--unknown").returncode, 0)
        self.assert_no_writes()


if __name__ == "__main__":
    unittest.main()
