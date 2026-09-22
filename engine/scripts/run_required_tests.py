"""Run a focused Cargo gate; successful execution of zero tests is an error."""
import argparse
import re
import subprocess
import sys


def passed_count(output):
    return sum(int(n) for n in re.findall(r"^test result: ok\. (\d+) passed; 0 failed;", output, re.MULTILINE))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package")
    parser.add_argument("filter")
    parser.add_argument("--exact", action="store_true")
    args = parser.parse_args()
    command = ["cargo", "test", "--locked", "-p", args.package, args.filter]
    if args.exact:
        command.extend(["--", "--exact"])
    result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, encoding="utf-8", errors="replace", check=False)
    print(result.stdout, end="")
    if result.returncode:
        return result.returncode
    if passed_count(result.stdout) == 0:
        print("Required test gate executed zero passing tests", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
