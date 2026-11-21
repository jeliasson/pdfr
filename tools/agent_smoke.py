#!/usr/bin/env python3
"""End-to-end check of agent control: talks MCP to `pdfr mcp` the way an AI
agent does, and has it open a form, read it, fill it in, take a screenshot,
save and close.

    tools/agent_smoke.py target/debug/pdfr samples/flat_form.pdf /tmp/out

Needs a display (CI runs it under Xvfb and Sway). Standard library only.
Exits non-zero, saying why, if anything is off.
"""

import base64
import json
import os
import subprocess
import sys


class Mcp:
    def __init__(self, exe, data_dir):
        self.p = subprocess.Popen(
            [exe, "mcp", "--data-dir", data_dir],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )
        self.n = 0

    def request(self, method, params=None):
        self.n += 1
        msg = {"jsonrpc": "2.0", "id": self.n, "method": method}
        if params is not None:
            msg["params"] = params
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()
        line = self.p.stdout.readline()
        if not line:
            sys.exit(f"pdfr mcp closed its output during {method}")
        answer = json.loads(line)
        assert answer["id"] == self.n, answer
        if "error" in answer:
            sys.exit(f"{method} failed: {answer['error']}")
        return answer["result"]

    def notify(self, method):
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": method}) + "\n")
        self.p.stdin.flush()

    def tool(self, name, **args):
        """Calls a tool: (its JSON answer, its image bytes or None)."""
        r = self.request("tools/call", {"name": name, "arguments": args})
        text = next(c["text"] for c in r["content"] if c["type"] == "text")
        if r.get("isError"):
            return {"error": text}, None
        image = next((c["data"] for c in r["content"] if c["type"] == "image"), None)
        return json.loads(text), base64.b64decode(image) if image else None

    def must(self, name, **args):
        answer, image = self.tool(name, **args)
        if "error" in answer:
            sys.exit(f"{name} failed: {answer['error']}")
        return answer, image


def main():
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    exe, form, out = (os.path.abspath(a) for a in sys.argv[1:])
    os.makedirs(out, exist_ok=True)
    data = os.path.join(out, "data")
    os.makedirs(data, exist_ok=True)
    saved = os.path.join(out, "filled.pdf")
    shot = os.path.join(out, "agent.png")
    for f in (saved, shot):
        if os.path.exists(f):
            os.remove(f)

    m = Mcp(exe, data)
    hello = m.request(
        "initialize",
        {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "agent_smoke", "version": "1"},
        },
    )
    assert hello["serverInfo"]["name"] == "pdfr", hello
    m.notify("notifications/initialized")
    names = [t["name"] for t in m.request("tools/list")["tools"]]
    for t in ("open", "read_form", "fill", "screenshot", "save_as", "close"):
        assert t in names, f"tool {t} is missing"

    opened, _ = m.must("open", path=form, new_window=True)
    assert opened["boxes"] > 0, opened
    assert opened["state"]["loading"] is False, opened

    boxes = m.must("read_form")[0]["boxes"]
    texts = [b for b in boxes if b["kind"] in ("text", "line") and not b.get("read_only")]
    checks = [b for b in boxes if b["kind"] == "checkbox"]
    assert texts, "the form has no text box"
    filled, _ = m.must("fill", ref=texts[0]["ref"], value="Anna Lindqvist")
    assert filled["box"]["value"] == "Anna Lindqvist", filled
    assert filled["state"]["unsaved_changes"] is True, filled
    if checks:
        ticked, _ = m.must("set_checked", ref=checks[0]["ref"], checked=True)
        assert ticked["box"]["checked"] is True, ticked

    info, png = m.must("screenshot")
    assert png and png[:8] == b"\x89PNG\r\n\x1a\n", "the screenshot is not a PNG"
    assert max(info["width"], info["height"]) <= 1568, info
    with open(shot, "wb") as f:
        f.write(png)

    again = m.must("read_form")[0]["boxes"]
    assert next(b for b in again if b["ref"] == texts[0]["ref"])["value"] == "Anna Lindqvist"

    done, _ = m.must("save_as", path=saved)
    assert done["state"]["unsaved_changes"] is False, done
    with open(saved, "rb") as f:
        assert f.read(5) == b"%PDF-", "the saved file is not a PDF"
    # A file that is there is not replaced unasked.
    refused, _ = m.tool("save_as", path=form)
    assert refused.get("error", "").startswith("exists"), refused

    m.must("close")
    m.p.stdin.close()
    m.p.wait(timeout=10)
    print(f"AGENT_SMOKE ok: {len(boxes)} boxes, screenshot {info['width']}x{info['height']}")
    print(f"AGENT_SCREENSHOT {shot}")
    print(f"AGENT_SAVED {saved}")


if __name__ == "__main__":
    main()
