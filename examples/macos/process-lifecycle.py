#!/usr/bin/env python3
"""Inspect, identify, and terminate only this example's own temporary child."""
import argparse
import os
import re
import shutil
import signal
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wmic", default=os.environ.get("WMIC_BIN", "wmic"))
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("this example requires macOS")
    binary = shutil.which(args.wmic)
    if binary is None:
        parser.error("cannot find wmic; use --wmic or set WMIC_BIN")

    def invoke(arguments, method=False):
        result = subprocess.run(
            [binary] + arguments,
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=20,
            check=True,
        )
        print(result.stdout, end="")
        if method:
            codes = re.findall(r"\bReturnValue\s*=\s*(\d+)\s*;", result.stdout)
            if codes != ["0"]:
                raise ValueError("expected exactly one successful method ReturnValue")
        return result.stdout

    child = None
    try:
        child = subprocess.Popen(
            ["/bin/sleep", "120"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        selector = "ProcessId=" + str(child.pid)
        print("Example-owned child PID:", child.pid)
        output = invoke(
            ["process", "where", selector, "get", "Name,ProcessId", "/value"]
        )
        if "ProcessId=" + str(child.pid) not in output.splitlines():
            raise ValueError("owned child was not returned by the query")
        invoke(["process", "where", selector, "call", "GetOwner"], method=True)
        if child.poll() is not None:
            raise ValueError("owned child exited before the termination demonstration")
        invoke(["process", "where", selector, "call", "Terminate"], method=True)
        child.wait(timeout=5)
        if child.returncode != -signal.SIGKILL:
            raise ValueError("expected Terminate to end the owned child by SIGKILL")
        print("Owned child terminated and reaped.")
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        diagnostic = getattr(error, "stderr", None) or str(error)
        parser.exit(1, "Process example failed: " + diagnostic.rstrip() + "\n")
    finally:
        if child is not None:
            if child.poll() is None:
                child.kill()
            child.wait()


if __name__ == "__main__":
    main()
