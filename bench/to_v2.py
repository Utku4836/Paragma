"""Converts a bench memory (the Go-era layout: knowledge/, episodes/, work/)
into a Paragma 2 project: notes with id and type properties, rules as
"- rN: text" lines, error sections named Symptom / Cause / Fix.

    python bench/to_v2.py <memory-dir> <paragma-home> [project-name]
"""
import os
import re
import sys

KIND_DIRS = {"episodes/errors": "error", "episodes/decisions": "decision"}


def split_props(text):
    if not text.startswith("---\n"):
        return {}, text
    end = text.find("\n---", 4)
    if end < 0:
        return {}, text
    props = {}
    for line in text[4:end].splitlines():
        if ":" in line:
            k, v = line.split(":", 1)
            props[k.strip()] = v.strip()
    body = text[end + 4:].lstrip("-\n")
    return props, body


def quote(v):
    return '"' + v.replace("\\", "\\\\").replace('"', '\\"') + '"' if (": " in v or v[:1] in "[{&*!|>'\"%@`#-,?:") else v


def render(props, body):
    lines = ["---"]
    for k, v in props.items():
        lines.append(f"{k}: {v}" if isinstance(v, str) and v.startswith("[") else f"{k}: {quote(str(v))}")
    lines.append("---")
    return "\n".join(lines) + "\n" + body.strip() + "\n"


def convert(mem, home, name):
    out = os.path.join(home, "projects", name)
    os.makedirs(out, exist_ok=True)
    counters = {"e": 0, "d": 0, "n": 0}

    def next_id(p):
        counters[p] += 1
        return f"{p}{counters[p]}"

    # rules: "> [!rule] text ^r001" callouts under "## Group" -> "- r1: text"
    rules = open(os.path.join(mem, "knowledge", "rules.md"), encoding="utf-8").read()
    _, body = split_props(rules)
    lines = []
    for line in body.splitlines():
        if line.startswith("## "):
            lines.append("\n" + line)
        m = re.match(r"> \[!rule\]\s*(.*?)\s*\^r0*(\d+)\s*$", line)
        if m:
            lines.append(f"- r{m.group(2)}: {m.group(1)}")
    with open(os.path.join(out, "rules.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write(render({"id": "rules", "type": "rules", "title": "Rules"}, "\n".join(lines)))

    for root, _, files in os.walk(mem):
        rel_root = os.path.relpath(root, mem).replace("\\", "/")
        if rel_root.startswith("archive") or rel_root == "." and False:
            continue
        for fn in sorted(files):
            if not fn.endswith(".md") or (rel_root == "knowledge" and fn == "rules.md"):
                continue
            src = os.path.join(root, fn)
            props, body = split_props(open(src, encoding="utf-8").read())
            kind = KIND_DIRS.get(rel_root, "note")
            new = {}
            if kind == "error":
                new["id"] = next_id("e")
                new["type"] = "error"
                new["title"] = props.get("title", fn[:-3]).strip('"')
                if props.get("sig"):
                    new["signature"] = props["sig"].strip('"')
                files = re.findall(r"^- (\S+)$", body.split("## Files", 1)[1], re.M) if "## Files" in body else []
                if files:
                    new["files"] = "[" + ", ".join(files) + "]"
                new["seen"] = props.get("seen", "1")
                new["status"] = props.get("status", "active")
                body = re.sub(r"\n## Files\n(- .*\n?)*", "\n", body)
                body = body.replace("## Root cause", "## Cause")
            elif kind == "decision":
                new["id"] = next_id("d")
                new["type"] = "decision"
                new["title"] = props.get("title", fn[:-3]).strip('"')
                new["status"] = props.get("status", "active")
            else:
                new["id"] = next_id("n")
                new["type"] = "note"
                new["title"] = props.get("title", fn[:-3]).strip('"')
            body = re.sub(r"^# .*\n", "", body, count=1)
            dst_dir = {"episodes/errors": "errors", "episodes/decisions": "decisions"}.get(rel_root, rel_root)
            dst = os.path.join(out, dst_dir, fn)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            with open(dst, "w", encoding="utf-8", newline="\n") as f:
                f.write(render(new, body))
    return out


if __name__ == "__main__":
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    print(convert(sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "shopkit"))
