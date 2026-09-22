#!/usr/bin/env python3
import argparse
import hashlib
import os

def hash_directory(dir_path):
    """Hashes the contents of a directory predictably."""
    sha256 = hashlib.sha256()
    
    for root, _, files in sorted(os.walk(dir_path)):
        # Optionally exclude .git or generated folders if requested
        for file in sorted(files):
            filepath = os.path.join(root, file)
            # Skip symlinks for simplicity in this baseline script
            if os.path.islink(filepath):
                continue
                
            with open(filepath, 'rb') as f:
                while chunk := f.read(8192):
                    sha256.update(chunk)
                    
    return sha256.hexdigest()

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Check integrity of an extracted corpus fixture.")
    parser.add_argument("--dir", required=True, help="Directory to hash")
    parser.add_argument("--expected", required=False, help="Expected SHA256 digest")
    
    args = parser.parse_args()
    
    if not os.path.exists(args.dir):
        print(f"Error: Directory {args.dir} does not exist.")
        exit(1)
        
    actual_digest = hash_directory(args.dir)
    print(f"Actual digest: {actual_digest}")
    
    if args.expected:
        if actual_digest != args.expected:
            print(f"INTEGRITY FAILURE: Expected {args.expected}, got {actual_digest}")
            exit(1)
        else:
            print("INTEGRITY SUCCESS")
            exit(0)
