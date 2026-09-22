#!/usr/bin/env python3
import argparse
import tarfile
import zipfile
import os
import shutil

def setup_fixture(fixture_path, target_dir):
    if os.path.exists(target_dir):
        print(f"Cleaning existing directory at {target_dir}...")
        shutil.rmtree(target_dir)
        
    os.makedirs(target_dir, exist_ok=True)
    print(f"Extracting {fixture_path} to {target_dir}...")
    
    if fixture_path.endswith('.zip'):
        with zipfile.ZipFile(fixture_path, 'r') as zip_ref:
            zip_ref.extractall(target_dir)
    elif fixture_path.endswith('.tar.gz') or fixture_path.endswith('.tgz'):
        with tarfile.open(fixture_path, 'r:gz') as tar_ref:
            tar_ref.extractall(target_dir)
    else:
        print(f"Unsupported fixture format: {fixture_path}")
        exit(1)
        
    print("Setup complete.")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Extract an evaluation fixture.")
    parser.add_argument("--fixture", required=True, help="Path to the .zip or .tar.gz fixture")
    parser.add_argument("--target", required=True, help="Target extraction directory")
    
    args = parser.parse_args()
    setup_fixture(args.fixture, args.target)
