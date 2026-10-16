"""Self-test for evaluate.py's outside-workspace detection: python bench/test_evaluate.py"""
import json
import os

src = open(os.path.join(os.path.dirname(__file__), "evaluate.py"), encoding="utf-8").read()
ns = {}
head = src[: src.index("rows = []")]
head = head.replace("RUNS = sys.argv[1]", "RUNS = '.'").replace("OUT = sys.argv[2] if len(sys.argv) > 2 else None", "OUT = None")
exec(head, ns)

ws = "c:/users/dev/appdata/local/temp/ws-ab12"
inside = json.dumps({"AbsolutePath": "C:\\Users\\dev\\AppData\\Local\\Temp\\ws-ab12\\repo\\cart\\cart.go"})
outside = json.dumps({"CommandLine": "Get-Content C:\\Users\\dev\\.gemini\\antigravity-cli\\brain\\x\\transcript.jsonl"})
gocmd = json.dumps({"CommandLine": "& \"C:\\Program Files\\Go\\bin\\go.exe\" test ./..."})
assert ns["outside_paths"](inside, ws) == [], ns["outside_paths"](inside, ws)
assert len(ns["outside_paths"](outside, ws)) == 1, ns["outside_paths"](outside, ws)
assert ns["outside_paths"](gocmd, ws) == [], ns["outside_paths"](gocmd, ws)
print("ok")
