#!/usr/bin/env python3
"""Project four scalar OS fields into JSON; not a general /value parser."""
import argparse
import json
import os
import shutil
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
    fields = ("Caption", "Version", "BuildNumber", "OSArchitecture")
    try:
        result = subprocess.run(
            [binary, "os", "get", ",".join(fields), "/value"],
            capture_output=True,
            text=True,
            encoding="utf-8",
            timeout=20,
            check=True,
        )
        values = {}
        for line in result.stdout.splitlines():
            name, separator, value = line.partition("=")
            if separator and name in fields:
                if name in values:
                    raise ValueError("duplicate OS property: " + name)
                values[name] = value
        if set(values) != set(fields) or any(not value for value in values.values()):
            raise ValueError("expected all four nonempty scalar OS properties")
    except (OSError, subprocess.SubprocessError, ValueError) as error:
        diagnostic = getattr(error, "stderr", None) or str(error)
        parser.exit(1, "OS query failed: " + diagnostic.rstrip() + "\n")
    print(json.dumps(values, indent=2, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
