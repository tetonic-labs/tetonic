import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from run_suite import grade, write_verifier


class VerifierOverlayTests(unittest.TestCase):
    def test_overlay_is_verified_without_accepting_undelivered_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workspace, overlay = root / "workspace", root / "overlay"
            workspace.mkdir()
            overlay.mkdir()
            (workspace / "answer.py").write_text("value = 0\n")
            (overlay / "answer.py").write_text("value = 42\n")
            grader = "import answer\nassert answer.value == 42\nprint('GRADE: PASS')\n"
            write_verifier(workspace, grader)
            shutil.copyfile(workspace / "_lokai_verify.py", overlay / "_lokai_verify.py")
            result = subprocess.run(
                [sys.executable, str(overlay / "_lokai_verify.py")],
                cwd=workspace, capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(grade(workspace, grader)[0])
            self.assertTrue(grade(overlay, grader)[0])


if __name__ == "__main__":
    unittest.main()
