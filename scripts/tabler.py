#!/usr/bin/env python3
"""Tabler's outline icons as the one file the app carries.

    curl -sSL -o tabler.tgz https://registry.npmjs.org/@tabler/icons/-/icons-<version>.tgz
    tar -xzf tabler.tgz
    scripts/tabler.py package

writes crates/onyx-app/assets/tabler/outline.tsv, a line an icon: its name,
its category, its tags and its drawing, tab between (`folder_look.rs` reads
it), and copies Tabler's license beside it.
"""

import html
import json
import shutil
import sys
from pathlib import Path


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    package = Path(sys.argv[1])
    out = Path(__file__).resolve().parent.parent / "crates/onyx-app/assets/tabler"
    out.mkdir(parents=True, exist_ok=True)
    nodes = json.loads((package / "tabler-nodes-outline.json").read_text())
    meta = json.loads((package / "icons.json").read_text())
    lines = []
    for name in sorted(nodes):
        about = meta.get(name, {})
        tags = " ".join(str(t).replace("\t", " ").replace("\n", " ") for t in about.get("tags", []))
        category = about.get("category", "") or ""
        body = "".join("<%s %s/>" % (tag, " ".join('%s="%s"' % (k, html.escape(str(v), quote=True)) for k, v in attrs.items())) for tag, attrs in nodes[name])
        lines.append(f"{name}\t{category}\t{tags}\t{body}")
    (out / "outline.tsv").write_text("\n".join(lines) + "\n")
    shutil.copy(package / "LICENSE", out / "LICENSE")
    version = json.loads((package / "package.json").read_text()).get("version", "?")
    print(f"{len(lines)} icons from Tabler {version}")


if __name__ == "__main__":
    main()
