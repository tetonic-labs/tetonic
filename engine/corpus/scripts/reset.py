#!/usr/bin/env python3
import argparse
import os
import subprocess

def reset_fixture(target_dir):
    if not os.path.exists(target_dir):
        print(f"Directory {target_dir} does not exist.")
        exit(1)
        
    print(f"Resetting {target_dir} to clean state...")
    try:
        # Assuming the fixture contains a git repository
        subprocess.run(["git", "clean", "-fdx"], cwd=target_dir, check=True)
        subprocess.run(["git", "reset", "--hard", "HEAD"], cwd=target_dir, check=True)
        print("Reset complete.")
    except subprocess.CalledProcessError as e:
        print(f"Failed to reset: {e}")
        exit(1)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Reset an evaluation fixture working tree.")
    parser.add_argument("--target", required=True, help="Target extraction directory to reset")
    
    args = parser.parse_args()
    reset_fixture(args.target)
