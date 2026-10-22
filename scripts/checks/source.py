# Purpose: Enforce authored source size, required headers, and collaborator references.
# Pattern: Repository Check.
# Usage: python3 scripts/checks/source.py from any directory.
# Related: AGENTS.md, docs/development.md.

from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
SOURCES = [
    ROOT.joinpath("install.sh"),
    *ROOT.joinpath("crates").rglob("*.rs"),
    *ROOT.joinpath("crates").rglob("*.sql"),
    *ROOT.joinpath("scripts").rglob("*.py"),
]


def inspect(path):
    lines = path.read_text(encoding="utf-8").splitlines()
    failures = []
    if len(lines) >= 350:
        failures.append(f"{len(lines)} physical lines; maximum is 349")
    start = 1 if lines and lines[0].startswith("#!") else 0
    header = lines[start:start + 8]
    for field in ("Purpose", "Pattern", "Usage", "Related"):
        entries = []
        prefixes = tuple(prefix + field + ":" for prefix in ("// ", "# ", "-- "))
        for line in header:
            if line.lstrip().startswith(prefixes):
                entries.append(line.split(field + ":", 1)[1].strip())
        if len(entries) != 1 or not entries[0]:
            failures.append(f"missing or repeated {field} header field")
        elif field == "Related" and entries[0].rstrip(".") != "None":
            for reference in entries[0].rstrip(".").split(","):
                relative = Path(reference.strip().strip("`"))
                if (relative.is_absolute() or ".." in relative.parts
                        or not ROOT.joinpath(relative).exists()):
                    failures.append(f"invalid collaborator reference: {reference.strip()}")
    return failures


def main():
    failures = []
    for path in sorted(set(SOURCES)):
        failures.extend((path, failure) for failure in inspect(path))
    for path, failure in failures:
        print(f"{path.relative_to(ROOT)}: {failure}", file=sys.stderr)
    if failures:
        return 1
    print(f"Checked {len(set(SOURCES))} authored source and automation files: "
          "all under 350 lines with valid headers.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
