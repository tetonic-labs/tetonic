# Corpus Contribution Guide

To maintain a highly reproducible, objective evaluation harness for Lokai, all contributions to the V2 evaluation corpus must adhere to strict guidelines.

## Adding a New Scenario
1. **Create the Snapshot:** Produce a static snapshot (`.tar.gz` or `.zip`) of a repository that triggers a specific software engineering task, security failure, or context puzzle. Do **not** use floating branches, latest releases from package managers, or live APIs.
2. **Hash the Snapshot:** Run `python scripts/integrity.py --dir <snapshot>` to calculate the deterministic digest of the directory.
3. **Define the Manifest:** Create a new JSON file in `manifests/` following the `EvaluationManifest` schema (which includes `PatchCorrectness` fields, `expected_redactions`, etc.).
4. **Synthetic Secrets Only:** NEVER commit genuine AWS keys, passwords, or personal access tokens into a fixture. Any sensitive scenarios must use purely synthetic credentials.
5. **Run Baselines:** Ensure you run the V1 engine against your new scenario and record the baseline in `baseline_results.json` before opening a pull request.
