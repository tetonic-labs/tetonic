import unittest
from unittest.mock import patch
from subprocess import CompletedProcess
import run_required_tests as gate


class RequiredTests(unittest.TestCase):
    def test_gate_outcomes(self):
        for output, process_code, expected in [
            ("test result: ok. 0 passed; 0 failed; 9 ignored; 0 measured; 2 filtered out;\n", 0, 1),
            ("test result: ok. 1 passed; 0 failed; 0 ignored;\n", 0, 0),
            ("test result: ok. 1 passed; 0 failed;\n", 101, 101),
            ("compilation succeeded but no test summary", 0, 1),
        ]:
            with self.subTest(output=output), patch("sys.argv", ["gate", "package", "tests::case", "--exact"]), patch.object(gate.subprocess, "run", return_value=CompletedProcess([], process_code, output)) as runner, patch("builtins.print"):
                self.assertEqual(gate.main(), expected)
                self.assertEqual(runner.call_args.args[0][-2:], ["--", "--exact"])


if __name__ == "__main__":
    unittest.main()
