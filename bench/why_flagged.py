"""Show why evaluate.py flags tool calls as outside the workspace.

    python bench/why_flagged.py <run-dir>
"""
import json
import os
import re
import sys

run = sys.argv[1]
ws = open(os.path.join(run, "workspace.txt"), encoding="utf-8").read().strip()
if ws.startswith("/") and ws[2] == "/":
    ws = ws[1] + ":" + ws[2:]
ws = ws.replace("\\", "/").lower()
own = ws.rsplit("/", 1)[-1]
for conv in ("conv1.jsonl", "conv2.jsonl"):
    p = os.path.join(run, conv)
    if not os.path.exists(p):
        continue
    for line in open(p, encoding="utf-8", errors="replace"):
        try:
            e = json.loads(line)
        except ValueError:
            continue
        su = e.get("step_update") or {}
        if su.get("state") != "DONE" or su.get("step_type") != "tool":
            continue
        info = su.get("tool_info") or {}
        params = json.dumps(info.get("parameters", {}), ensure_ascii=False)
        text = (params + " " + (info.get("output") or "")).replace("\\\\", "/").replace("\\", "/").lower()
        reasons = []
        if "paragma/bench" in text:
            i = text.index("paragma/bench")
            reasons.append("bench: " + text[max(0, i - 40): i + 60])
        for w in set(re.findall(r"/(ws-[a-z0-9]{8})", params.replace("\\\\", "/").replace("\\", "/").lower())):
            if w != own:
                reasons.append("foreign " + w)
        if info.get("name") == "manage_task":
            continue
        gm = re.search(r"c:/users/[^/]+/\.gemini/", text)
        if gm:
            i = gm.start()
            reasons.append("gemini: " + text[max(0, i - 50): i + 60])
        if reasons:
            print(conv, su.get("step_index"), info.get("name"), "|", " || ".join(reasons)[:300])
