#!/usr/bin/env python3
"""Exercise the documented MCP stdio round trip against a storyboard binary."""

import json
import subprocess
import sys
from pathlib import Path


def call(process, request_id, method, params):
    request = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
    process.stdin.write(json.dumps(request) + "\n")
    process.stdin.flush()
    while True:
        response = json.loads(process.stdout.readline())
        if response.get("id") == request_id:
            if "error" in response:
                raise RuntimeError(response["error"])
            return response["result"]


def tool(process, request_id, name, arguments):
    result = call(process, request_id, "tools/call", {"name": name, "arguments": arguments})
    if result.get("isError"):
        raise RuntimeError(result)
    return json.loads(result["content"][-1]["text"]), result["content"]


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: mcp_round_trip.py /absolute/path/to/storyboard workspace")
    binary, workspace = map(Path, sys.argv[1:])
    workspace.mkdir(parents=True, exist_ok=True)
    process = subprocess.Popen(
        [binary, "mcp", workspace], text=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE
    )
    try:
        call(process, 1, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "walkthrough", "version": "1"}})
        process.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}) + "\n")
        process.stdin.flush()
        project, _ = tool(process, 2, "new", {"project": "mcp-story", "title": "MCP round trip"})
        scene, _ = tool(process, 3, "add_scene", {"project": "mcp-story", "base_version": project["version"], "title": "Opening"})
        shot, _ = tool(process, 4, "add_shot", {"project": "mcp-story", "base_version": scene["version"], "scene": "scene-1", "title": "Arrival"})
        edited, _ = tool(process, 5, "add_text", {"project": "mcp-story", "base_version": shot["version"], "at": "1", "text": "A train enters."})
        rendered, content = tool(process, 6, "panel_image", {"project": "mcp-story", "at": 1, "width": 64, "mode": "stage"})
        if content[0].get("type") != "image":
            raise RuntimeError("panel_image did not return an image block")
        print(f"version={edited['version']} path={rendered['path']} image_block=true")
    finally:
        process.terminate()
        process.wait(timeout=5)


if __name__ == "__main__":
    main()
