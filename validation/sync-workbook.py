"""
One-command workbook sync: copies the workbook from working/ into build/validation/,
then runs _xverify_phase_a.py to confirm 484/484 cross-alignment.

Usage (from build/ directory):
    python validation/sync-workbook.py

Steps:
1. Removes read-only attribute from the existing workbook (Windows attrib -R)
2. Copies D:\\Projects\\IDEA\\working\\Platform_Requirements_and_Exceptions.xlsx
   to D:\\Projects\\IDEA\\build\\validation\\Platform_Requirements_and_Exceptions.xlsx
3. Restores read-only attribute (attrib +R)
4. Runs _xverify_phase_a.py from the build/ directory
5. Reports result (exit 0 = clean, exit 1 = drift detected)

If drift is detected, the old workbook is NOT restored automatically.
The operator must investigate the drift (usually means the workbook was updated
with new BRs or SR references that the specs do not yet define, or vice versa).
"""
import shutil
import subprocess
import sys
import pathlib

BUILD_ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKING_DIR = BUILD_ROOT.parent / "working"
SOURCE_WORKBOOK = WORKING_DIR / "Platform_Requirements_and_Exceptions.xlsx"
TARGET_WORKBOOK = BUILD_ROOT / "validation" / "Platform_Requirements_and_Exceptions.xlsx"
XVERIFY_SCRIPT = BUILD_ROOT / "validation" / "_xverify_phase_a.py"

print("=" * 60)
print("WORKBOOK SYNC")
print("=" * 60)
print(f"  Source: {SOURCE_WORKBOOK}")
print(f"  Target: {TARGET_WORKBOOK}")
print()

# Step 1: verify source exists
if not SOURCE_WORKBOOK.exists():
    print(f"ERROR: Source workbook not found: {SOURCE_WORKBOOK}")
    sys.exit(1)

# Step 2: remove read-only on target (Windows-specific)
if TARGET_WORKBOOK.exists():
    try:
        subprocess.run(["attrib", "-R", str(TARGET_WORKBOOK)], check=False)
        print("  Removed read-only from target workbook")
    except FileNotFoundError:
        # attrib not available (non-Windows) — try chmod
        TARGET_WORKBOOK.chmod(0o644)
        print("  Set target workbook writable (chmod 644)")

# Step 3: copy
shutil.copy2(SOURCE_WORKBOOK, TARGET_WORKBOOK)
print(f"  Copied {SOURCE_WORKBOOK.name} to build/validation/")

# Step 4: restore read-only
try:
    subprocess.run(["attrib", "+R", str(TARGET_WORKBOOK)], check=False)
    print("  Restored read-only attribute")
except FileNotFoundError:
    TARGET_WORKBOOK.chmod(0o444)
    print("  Set target workbook read-only (chmod 444)")

# Step 5: run cross-verification
print()
print("Running cross-verification...")
print("-" * 60)
result = subprocess.run(
    [sys.executable, str(XVERIFY_SCRIPT)],
    cwd=str(BUILD_ROOT),
)
print("-" * 60)
print()

if result.returncode == 0:
    print("SYNC COMPLETE: workbook and specs are in perfect cross-alignment.")
    sys.exit(0)
else:
    print("SYNC WARNING: drift detected between workbook and specs.")
    print("The workbook has been copied but cross-verification FAILED.")
    print("Investigate the drift before continuing any implementation work.")
    print("See validation/_xverify_phase_a_report.txt for details.")
    sys.exit(1)
