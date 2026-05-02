#!/usr/bin/env python3
"""
Manual HTTP tester for the Rust Axum app.

Examples:
  python3 scripts/manual_api_test.py --start-server list-users
  python3 scripts/manual_api_test.py get-user 1
  python3 scripts/manual_api_test.py check-all
  python3 scripts/manual_api_test.py interactive
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass
from html.parser import HTMLParser
from typing import Any


DEFAULT_BASE_URL = "http://127.0.0.1:3000"


@dataclass
class Response:
    status: int
    headers: dict[str, str]
    body: bytes
    elapsed_ms: float

    @property
    def text(self) -> str:
        return self.body.decode("utf-8", errors="replace")

    def json(self) -> Any:
        return json.loads(self.text)


class TextExtractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.parts: list[str] = []

    def handle_data(self, data: str) -> None:
        text = data.strip()
        if text:
            self.parts.append(text)

    def text(self) -> str:
        return "\n".join(self.parts)


def request(base_url: str, path: str, timeout: float = 5.0) -> Response:
    url = f"{base_url.rstrip('/')}/{path.lstrip('/')}"
    req = urllib.request.Request(url, method="GET")
    started_at = time.monotonic()

    try:
        with urllib.request.urlopen(req, timeout=timeout) as res:
            body = res.read()
            elapsed_ms = (time.monotonic() - started_at) * 1000.0
            return Response(
                status=res.status,
                headers=dict(res.headers.items()),
                body=body,
                elapsed_ms=elapsed_ms,
            )
    except urllib.error.HTTPError as err:
        body = err.read()
        elapsed_ms = (time.monotonic() - started_at) * 1000.0
        return Response(
            status=err.code,
            headers=dict(err.headers.items()),
            body=body,
            elapsed_ms=elapsed_ms,
        )


def print_response(response: Response, *, parse_html: bool = False) -> None:
    content_type = response.headers.get("Content-Type", "")
    print(f"HTTP {response.status}")
    print(f"Response-Time: {response.elapsed_ms:.2f} ms")
    if content_type:
        print(f"Content-Type: {content_type}")
    print()

    if "application/json" in content_type:
        print(json.dumps(response.json(), indent=2, sort_keys=True))
        return

    if parse_html:
        parser = TextExtractor()
        parser.feed(response.text)
        print(parser.text())
        return

    print(response.text)


def assert_status(response: Response, expected: int, label: str) -> bool:
    if response.status == expected:
        print(f"PASS {label}: HTTP {expected} ({response.elapsed_ms:.2f} ms)")
        return True

    print(
        f"FAIL {label}: expected HTTP {expected}, got HTTP {response.status} "
        f"({response.elapsed_ms:.2f} ms)"
    )
    print_response(response)
    return False


def check_all(base_url: str) -> int:
    failures = 0

    users = request(base_url, "/api/users")
    failures += not assert_status(users, 200, "list users")
    if users.status == 200:
        data = users.json()
        expected = [
            {"id": 1, "name": "Alice", "email": "alice@example.com"},
            {"id": 2, "name": "Bob", "email": "bob@example.com"},
        ]
        if data == expected:
            print("PASS list users body")
        else:
            failures += 1
            print("FAIL list users body")
            print(json.dumps(data, indent=2, sort_keys=True))

    user = request(base_url, "/api/users/1")
    failures += not assert_status(user, 200, "get user 1")
    if user.status == 200:
        data = user.json()
        if data == {"id": 1, "name": "Alice", "email": "alice@example.com"}:
            print("PASS get user 1 body")
        else:
            failures += 1
            print("FAIL get user 1 body")
            print(json.dumps(data, indent=2, sort_keys=True))

    missing = request(base_url, "/api/users/999")
    failures += not assert_status(missing, 404, "missing user")
    if missing.status == 404:
        data = missing.json()
        if data == {"error": "User with id 999 not found"}:
            print("PASS missing user body")
        else:
            failures += 1
            print("FAIL missing user body")
            print(json.dumps(data, indent=2, sort_keys=True))

    index = request(base_url, "/")
    failures += not assert_status(index, 200, "index page")
    if index.status == 200:
        html = index.text
        expected_fragments = [
            "<title>Users List</title>",
            "<strong>Alice</strong>",
            "alice@example.com",
            "<strong>Bob</strong>",
            "bob@example.com",
        ]
        missing_fragments = [frag for frag in expected_fragments if frag not in html]
        if not missing_fragments:
            print("PASS index page body")
        else:
            failures += 1
            print("FAIL index page body missing fragments:")
            for fragment in missing_fragments:
                print(f"  {fragment}")

    return 1 if failures else 0


def wait_for_server(base_url: str, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    last_error: Exception | None = None

    while time.monotonic() < deadline:
        try:
            request(base_url, "/api/users", timeout=1.0)
            return
        except Exception as err:  # noqa: BLE001 - this is a readiness probe.
            last_error = err
            time.sleep(0.2)

    raise RuntimeError(f"server did not become ready at {base_url}: {last_error}")


def start_server(base_url: str, timeout: float) -> subprocess.Popen[bytes]:
    process = subprocess.Popen(
        ["cargo", "run"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )

    try:
        wait_for_server(base_url, timeout)
    except Exception:
        process.terminate()
        try:
            output, _ = process.communicate(timeout=3)
        except subprocess.TimeoutExpired:
            process.kill()
            output, _ = process.communicate()
        if output:
            print(output.decode("utf-8", errors="replace"), file=sys.stderr)
        raise

    return process


def interactive(base_url: str) -> int:
    menu = """
Choose a request:
  1. List users
  2. Get user by id
  3. Render index page
  4. Run all checks
  q. Quit
"""

    while True:
        print(menu)
        choice = input("> ").strip().lower()

        if choice == "1":
            print_response(request(base_url, "/api/users"))
        elif choice == "2":
            user_id = input("User id: ").strip()
            print_response(request(base_url, f"/api/users/{user_id}"))
        elif choice == "3":
            print_response(request(base_url, "/"), parse_html=True)
        elif choice == "4":
            check_all(base_url)
        elif choice in {"q", "quit", "exit"}:
            return 0
        else:
            print("Unknown choice.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Manually test the Rust Axum app routes.")
    parser.add_argument(
        "--base-url",
        default=DEFAULT_BASE_URL,
        help=f"Base URL for the running app. Default: {DEFAULT_BASE_URL}",
    )
    parser.add_argument(
        "--start-server",
        action="store_true",
        help="Start the app with `cargo run` before sending requests.",
    )
    parser.add_argument(
        "--startup-timeout",
        type=float,
        default=15.0,
        help="Seconds to wait when --start-server is used. Default: 15",
    )

    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("list-users", help="GET /api/users")

    get_user = subcommands.add_parser("get-user", help="GET /api/users/{id}")
    get_user.add_argument("id", help="User id to fetch.")

    subcommands.add_parser("index", help="GET / and print extracted page text")
    subcommands.add_parser("check-all", help="Run basic manual checks for every route")
    subcommands.add_parser("interactive", help="Open an interactive prompt")

    return parser.parse_args()


def main() -> int:
    args = parse_args()
    process: subprocess.Popen[bytes] | None = None

    try:
        if args.start_server:
            process = start_server(args.base_url, args.startup_timeout)

        if args.command == "list-users":
            print_response(request(args.base_url, "/api/users"))
            return 0
        if args.command == "get-user":
            print_response(request(args.base_url, f"/api/users/{args.id}"))
            return 0
        if args.command == "index":
            print_response(request(args.base_url, "/"), parse_html=True)
            return 0
        if args.command == "check-all":
            return check_all(args.base_url)
        if args.command == "interactive":
            return interactive(args.base_url)

        raise ValueError(f"Unhandled command: {args.command}")
    finally:
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


if __name__ == "__main__":
    raise SystemExit(main())
