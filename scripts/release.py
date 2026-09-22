#!/usr/bin/env python3
"""
Tetonic / Lokai Release Operator Tool

Orchestrates a new release by performing pre-flight sanity checks, running the
architecture gate, generating changelogs, tagging the release commit, and
triggering the GitHub Actions multi-platform build matrix.

Usage:
    python scripts/release.py <version> [--repo tetonic-labs/tetonic] [--skip-gate] [--dry-run]
Example:
    python scripts/release.py v0.1.0
"""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path


def run_cmd(cmd, cwd=None, capture=False, check=True):
    """Run shell command and return stdout or exit status."""
    if isinstance(cmd, str):
        shell = True
    else:
        shell = False
    
    result = subprocess.run(
        cmd,
        cwd=cwd,
        shell=shell,
        text=True,
        capture_output=capture,
        check=check,
    )
    return result.stdout.strip() if capture else ""


def main():
    parser = argparse.ArgumentParser(description="Tetonic release automation operator.")
    parser.add_argument("version", help="Release version (e.g., v0.1.0 or 0.1.0)")
    parser.add_argument(
        "--repo",
        default="tetonic-labs/tetonic",
        help="GitHub repository (default: tetonic-labs/tetonic)",
    )
    parser.add_argument(
        "--skip-gate",
        action="store_true",
        help="Skip local cargo architecture verification gate (not recommended)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Execute pre-flight checks without creating or pushing git tags",
    )
    parser.add_argument(
        "--allow-dirty",
        action="store_true",
        help="Allow running release pre-flight checks with a dirty working tree",
    )

    args = parser.parse_args()

    # Normalize version to start with 'v'
    version = args.version if args.version.startswith("v") else f"v{args.version}"
    if not re.match(r"^v\d+\.\d+\.\d+(-[a-zA-Z0-9.]+)?$", version):
        print(f"Error: Invalid semantic version format '{version}'. Expected vX.Y.Z", file=sys.stderr)
        sys.exit(1)

    repo_root = Path(__file__).resolve().parent.parent
    engine_dir = repo_root / "engine"

    print("==========================================================")
    print(f"  Tetonic Release Pipeline: {version}")
    print(f"  Target Repository:        https://github.com/{args.repo}")
    print("==========================================================")
    print()

    # Step 1: Verify git working tree
    print("==> Checking git working tree status...")
    status = run_cmd(["git", "status", "--porcelain"], cwd=repo_root, capture=True)
    if status:
        if args.allow_dirty:
            print("Warning: Git working tree is dirty, proceeding due to --allow-dirty.")
        else:
            print("Error: Git working tree is dirty. Please commit or stash changes before releasing.", file=sys.stderr)
            print(status, file=sys.stderr)
            sys.exit(1)
    else:
        print("Working tree is clean.")
    print()

    # Step 2: Run engineering gate
    if not args.skip_gate:
        print("==> Running engineering gate (cargo run -p lokai-arch-gate -- verify package)...")
        try:
            run_cmd(["cargo", "run", "-p", "lokai-arch-gate", "--", "verify", "package"], cwd=engine_dir)
            print("Engineering gate verification: PASSED")
        except subprocess.CalledProcessError:
            print("Error: Engineering gate failed. Fix lints/architecture errors before releasing.", file=sys.stderr)
            sys.exit(1)
        print()
    else:
        print("Warning: Skipping engineering gate (--skip-gate specified).")
        print()

    # Step 3: Check if tag already exists
    print(f"==> Verifying tag {version}...")
    existing_tags = run_cmd(["git", "tag", "-l", version], cwd=repo_root, capture=True)
    if existing_tags:
        print(f"Error: Git tag '{version}' already exists in this repository.", file=sys.stderr)
        sys.exit(1)

    # Step 4: Derive changelog
    print("==> Generating changelog from commits...")
    last_tag = run_cmd(["git", "describe", "--tags", "--abbrev=0"], cwd=repo_root, capture=True, check=False)
    if last_tag:
        print(f"Previous tag: {last_tag}")
        commits = run_cmd(["git", "log", f"{last_tag}..HEAD", "--oneline"], cwd=repo_root, capture=True)
    else:
        print("No previous tag found. Using commit history:")
        commits = run_cmd(["git", "log", "-n", "20", "--oneline"], cwd=repo_root, capture=True)

    print()
    print("Changelog summary:")
    print(commits if commits else "(No commits)")
    print()

    if args.dry_run:
        print("Dry run completed successfully. No git tags were created.")
        return

    # Step 5: Confirm and tag
    confirm = input(f"Create and push tag '{version}' to trigger release build? [y/N]: ").strip().lower()
    if confirm not in ("y", "yes"):
        print("Release aborted by user.")
        sys.exit(0)

    print(f"==> Creating annotated git tag {version}...")
    run_cmd(["git", "tag", "-a", version, "-m", f"Release {version}"], cwd=repo_root)

    print(f"==> Pushing tag {version} to origin...")
    try:
        run_cmd(["git", "push", "origin", version], cwd=repo_root)
        print()
        print("==========================================================")
        print(f"  Tag {version} pushed successfully!")
        print("  GitHub Actions will now build and publish:")
        print(f"    https://github.com/{args.repo}/actions")
        print(f"    https://github.com/{args.repo}/releases/tag/{version}")
        print("==========================================================")
    except subprocess.CalledProcessError as e:
        print(f"Warning: Failed to push tag to origin: {e}")
        print("Push manually when ready:")
        print(f"  git push origin {version}")


if __name__ == "__main__":
    main()
