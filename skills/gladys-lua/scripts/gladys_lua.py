#!/usr/bin/env python3
import argparse
import json
import os
import sys
import urllib.error
import urllib.request


def _env():
    url = os.environ.get("GLADYS_DAEMON_URL", "http://127.0.0.1:3923").rstrip("/")
    token = os.environ.get("GLADYS_DAEMON_TOKEN")
    if not token:
        sys.exit("set GLADYS_DAEMON_TOKEN")
    return url, token


def _call(method, path, body, extra):
    url, token = _env()
    headers = {
        "Authorization": f"Bearer {token}",
        "Content-Type": "text/plain",
        **extra,
    }
    req = urllib.request.Request(url + path, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()


def _check(path):
    body = open(path, "rb").read()
    _, raw = _call("POST", "/v1/scripts/check", body, {})
    text = raw.decode()
    print(text)
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        sys.exit(1)


def main():
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("check")
    c.add_argument("file")
    u = sub.add_parser("upload")
    u.add_argument("type", choices=["command", "handler"])
    u.add_argument("name")
    u.add_argument("file")
    u.add_argument("--level", default="user")
    u.add_argument("--docs", default="")
    u.add_argument("--event", default="message.inbound")
    u.add_argument("--account", default="")
    u.add_argument("--kind", dest="conv_kind", default="")
    u.add_argument("--peer", default="")
    args = p.parse_args()
    if args.cmd == "check":
        data = _check(args.file)
        sys.exit(0 if data.get("ok") else 1)
    data = _check(args.file)
    if not data.get("ok"):
        sys.exit(1)
    headers = {
        "X-Gladys-Type": args.type,
        "X-Gladys-Docs": args.docs,
    }
    if args.type == "command":
        headers["X-Gladys-Level"] = args.level
    else:
        headers["X-Gladys-Event"] = args.event
        if args.account:
            headers["X-Gladys-Account"] = args.account
        if args.conv_kind:
            headers["X-Gladys-Kind"] = args.conv_kind
        if args.peer:
            headers["X-Gladys-Peer"] = args.peer
    body = open(args.file, "rb").read()
    _, raw = _call("PUT", f"/v1/scripts/{args.name}", body, headers)
    print(raw.decode())


if __name__ == "__main__":
    main()
