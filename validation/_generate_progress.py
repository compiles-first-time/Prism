"""
One-shot generator for build/PROGRESS.md.

Scans every expanded spec in build/spec/ and extracts the SR table rows
(left-most cell = backticked SR_* ID). Produces PROGRESS.md with:

  | SR ID | Spec | Layer | Type | Usecase (excerpt) | Status |

Status starts at "Not Started" for every row. Build chat updates this file
as SRs are implemented, tested, and verified.

Run from build/ or build/validation/ — output goes to build/PROGRESS.md.

Usage:
    python validation/_generate_progress.py
"""
import pathlib
import re

BUILD_ROOT = pathlib.Path(__file__).resolve().parent.parent
SPECS_DIR = BUILD_ROOT / "spec"
OUTPUT_PATH = BUILD_ROOT / "PROGRESS.md"

# Matches the first cell of a markdown table row whose content is EXACTLY a
# backticked SR ID (same strict form used by _xverify_phase_a.py). The closing
# backtick must come immediately after the SR ID — this filters out legend and
# cross-reference rows where an SR ID is mentioned inside a description cell.
SR_ROW_LINE = re.compile(r'^\| `(SR_[A-Z][A-Z0-9]*_\d+(?:_[A-Za-z0-9-]+)?)`(.*)$', re.MULTILINE)


def split_row(row_text):
    """Split a markdown table row into trimmed cells (drop leading/trailing empty)."""
    parts = [c.strip() for c in row_text.split("|")]
    # Drop the leading and trailing empty string from leading/trailing pipes
    if parts and parts[0] == "":
        parts = parts[1:]
    if parts and parts[-1] == "":
        parts = parts[:-1]
    return parts


rows = []

VALID_TYPES = {"---", "SE", "BE"}

for spec_file in sorted(SPECS_DIR.glob("*.md")):
    spec_name = spec_file.stem  # e.g. "01-governance-layer-expanded"
    short_spec = spec_name.replace("-expanded", "")
    with open(spec_file, encoding="utf-8") as f:
        content = f.read()
    for m in SR_ROW_LINE.finditer(content):
        # Authoritative SR ID comes from the regex capture group — never
        # from the parsed cells. split_row gets confused by cross-reference
        # rows whose first cell has extra content beyond the SR ID.
        sr_id = m.group(1)
        # Get the full line text for cell extraction
        line_start = m.start()
        line_end = content.find("\n", line_start)
        if line_end < 0:
            line_end = len(content)
        row_text = content[line_start:line_end]
        cells = split_row(row_text)
        type_col = cells[1].strip() if len(cells) > 1 else ""
        # Filter: only accept rows whose Type column is a valid SR type.
        # This excludes Cross-Reference Index rows, BP Log rows, etc., whose
        # second column is a spec file name or description rather than
        # ---/SE/BE. Those rows are not SR definitions.
        if type_col not in VALID_TYPES:
            continue
        layer = cells[2] if len(cells) > 2 else ""
        usecase = cells[3] if len(cells) > 3 else ""
        # Normalize: strip backticks, collapse whitespace, truncate
        usecase = re.sub(r"\s+", " ", usecase).strip()
        if len(usecase) > 110:
            usecase = usecase[:107] + "..."
        rows.append({
            "sr_id": sr_id,
            "spec": short_spec,
            "type": type_col or "---",
            "layer": layer or "",
            "usecase": usecase or "",
        })


# Deduplicate by SR ID (some SRs may appear multiple times in tables)
seen = set()
dedup = []
for r in rows:
    if r["sr_id"] not in seen:
        seen.add(r["sr_id"])
        dedup.append(r)

# Sort by prefix then numeric component
def sort_key(r):
    # SR_GOV_01 -> ("GOV", 1, "")
    # SR_GOV_01_BE-01 -> ("GOV", 1, "BE-01")
    parts = r["sr_id"].split("_")
    prefix = parts[1] if len(parts) > 1 else ""
    try:
        num = int(parts[2]) if len(parts) > 2 else 0
    except ValueError:
        num = 0
    qual = "_".join(parts[3:]) if len(parts) > 3 else ""
    return (prefix, num, qual)


dedup.sort(key=sort_key)


# Group by prefix for sectioning
sections = {}
for r in dedup:
    prefix = r["sr_id"].split("_")[1]
    sections.setdefault(prefix, []).append(r)


prefix_to_title = {
    "GOV":   "Governance Layer (SR_GOV_*)",
    "DM":    "Data Model (SR_DM_*)",
    "CONN":  "Connection Layer (SR_CONN_*)",
    "INT":   "Intelligence Layer (SR_INT_*)",
    "LLM":   "LLM Routing (SR_LLM_*)",
    "DS":    "Decision Support (SR_DS_*)",
    "UI":    "Interface (SR_UI_*)",
    "CAT":   "Component Catalog (SR_CAT_*)",
    "SA":    "Service Account Catalog (SR_SA_*)",
    "SCALE": "Scalability Infrastructure (SR_SCALE_*)",
    "FW":    "Value Flywheel (SR_FW_*)",
    "UU":    "Unknown Unknowns (SR_UU_*)",
    "V2":    "V2 Handoff Contract (SR_V2_*)",
    "OVR":   "Overview Index (SR_OVR_*)",
}


with open(OUTPUT_PATH, "w", encoding="utf-8") as f:
    f.write("# PROGRESS — Spec Requirement Implementation Tracker\n\n")
    f.write("**Purpose:** Tracks every SR from the 14 expanded specs with its implementation status. Build chat updates this file as SRs move through the pipeline.\n\n")
    f.write("**Created:** 2026-04-11 (Phase 3 Build Environment Setup)\n")
    f.write("**Generated by:** `validation/_generate_progress.py`\n\n")
    # Compute main-flow vs exception counts
    main_flow = [r for r in dedup if r["type"] == "---"]
    se_rows = [r for r in dedup if r["type"] == "SE"]
    be_rows = [r for r in dedup if r["type"] == "BE"]
    f.write(f"**Total SRs:** {len(dedup)}\n\n")
    f.write("### Row Type Breakdown\n\n")
    f.write("| Type | Count | What It Means |\n")
    f.write("|------|------:|---------------|\n")
    f.write(f"| Main flow (`---`) | {len(main_flow)} | Features and behaviors. Each is an implementable unit. |\n")
    f.write(f"| System Exception (`SE`) | {len(se_rows)} | Infrastructure failure handlers (network, auth, timeout, resource exhaustion). |\n")
    f.write(f"| Business Exception (`BE`) | {len(be_rows)} | Logic/validation failure handlers (invalid input, policy violation, permission denied). |\n")
    f.write(f"| **TOTAL** | **{len(dedup)}** | Main flows drive implementation scope; exception rows drive resilience coverage. |\n\n")
    f.write("**Reading this table:** When tracking implementation velocity, the main-flow count ")
    f.write(f"({len(main_flow)}) represents the number of distinct features and behaviors. ")
    f.write("Completing a main-flow SR means the happy path works. Completing its associated SE and BE rows ")
    f.write("means the error paths are also covered. \"20 SRs implemented\" typically means ~8-12 features ")
    f.write("with their exception handlers.\n\n")
    f.write("---\n\n")
    f.write("## Status Legend\n\n")
    f.write("| Status | Meaning |\n")
    f.write("|--------|---------|\n")
    f.write("| Not Started | No code exists for this SR. Default starting state. |\n")
    f.write("| In Progress | Code is being written for this SR. Not yet complete. |\n")
    f.write("| Implemented | Code exists and covers the main flow and all relevant SE/BE rows. No tests yet. |\n")
    f.write("| Tested | Tests exist per `generate-test-from-spec` and all pass. |\n")
    f.write("| Verified | `implementation-reviewer` agent has approved the implementation against the spec. Ready to commit. |\n")
    f.write("| Blocked | Halted pending clarification from Nick. See LOG.md for context. |\n\n")
    f.write("**Transition rule:** Not Started -> In Progress -> Implemented -> Tested -> Verified. Blocked can come from any state and returns to the state that was active before the block.\n\n")
    f.write("---\n\n")
    f.write("## Summary by Layer\n\n")
    f.write("| Layer | Main | SE | BE | Total | Not Started | In Progress | Implemented | Tested | Verified | Blocked |\n")
    f.write("|-------|-----:|---:|---:|------:|------------:|------------:|------------:|-------:|---------:|--------:|\n")
    total_main = 0
    total_se = 0
    total_be = 0
    for prefix in sorted(sections.keys()):
        srs = sections[prefix]
        count = len(srs)
        n_main = sum(1 for r in srs if r["type"] == "---")
        n_se = sum(1 for r in srs if r["type"] == "SE")
        n_be = sum(1 for r in srs if r["type"] == "BE")
        total_main += n_main
        total_se += n_se
        total_be += n_be
        title = prefix_to_title.get(prefix, f"SR_{prefix}_*")
        f.write(f"| {title} | {n_main} | {n_se} | {n_be} | {count} | {count} | 0 | 0 | 0 | 0 | 0 |\n")
    f.write(f"| **TOTAL** | **{total_main}** | **{total_se}** | **{total_be}** | **{len(dedup)}** | **{len(dedup)}** | **0** | **0** | **0** | **0** | **0** |\n\n")
    f.write("---\n\n")

    for prefix in sorted(sections.keys()):
        title = prefix_to_title.get(prefix, f"SR_{prefix}_*")
        srs = sections[prefix]
        f.write(f"## {title}\n\n")
        f.write(f"**Count:** {len(srs)}\n\n")
        f.write("| SR ID | Spec | Type | Layer | Usecase (excerpt) | Status |\n")
        f.write("|-------|------|------|-------|-------------------|--------|\n")
        for r in srs:
            sr_id = r["sr_id"]
            spec = r["spec"]
            type_col = r["type"]
            layer = r["layer"]
            usecase = r["usecase"].replace("|", "\\|")
            f.write(f"| `{sr_id}` | {spec} | {type_col} | {layer} | {usecase} | Not Started |\n")
        f.write("\n")

print(f"Wrote {OUTPUT_PATH}")
print(f"Total SRs: {len(dedup)}")
print("Sections:")
for prefix in sorted(sections.keys()):
    print(f"  {prefix}: {len(sections[prefix])}")
