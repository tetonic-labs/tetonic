import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from general_tasks import GENERAL_TASKS


class GeneralGraderTests(unittest.TestCase):
    def grade(self, task, answer):
        with tempfile.TemporaryDirectory() as directory:
            (Path(directory) / "answer.json").write_text(json.dumps(answer))
            with patch.object(sys, "path", [directory] + sys.path), contextlib.redirect_stdout(io.StringIO()):
                exec(task["grader"], {"sys": sys})

    def test_correct_answers_pass(self):
        answers = [
            {"goods_after_discount":48, "shipping":6, "tax":4.8, "total":58.8, "policy_source":"policy.txt"},
            {"A":1, "B":2, "D":3, "C":4},
            {"west":10, "east":10, "total":20, "included_records":4},
        ]
        for task, answer in zip(GENERAL_TASKS, answers):
            self.grade(task, answer)

    def test_plausible_wrong_answers_fail(self):
        answers = [
            {"goods_after_discount":48, "shipping":0, "tax":4.8, "total":52.8, "policy_source":"policy.txt"},
            {"A":1, "B":2, "C":3, "D":4},
            {"west":22.5, "east":10, "total":32.5, "included_records":5},
        ]
        for task, answer in zip(GENERAL_TASKS, answers):
            with self.assertRaises(AssertionError):
                self.grade(task, answer)


if __name__ == "__main__":
    unittest.main()
