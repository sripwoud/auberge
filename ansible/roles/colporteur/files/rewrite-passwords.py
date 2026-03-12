#!/usr/bin/env python3
"""Rewrite colporteur config passwords to use sidecar secret files.

Reads a colporteur config.toml, finds [accounts.*] sections, replaces
password values with '!cat <secrets_dir>/<account_name>', and outputs
discovered account names to stdout (one per line).
"""

import re
import sys

SAFE_NAME = re.compile(r"^[A-Za-z0-9_-]+$")


def main():
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <config.toml> <secrets_dir>", file=sys.stderr)
        sys.exit(1)

    config_path = sys.argv[1]
    secrets_dir = sys.argv[2]

    with open(config_path) as f:
        lines = f.readlines()

    current_account = None
    result = []
    accounts = []

    for line in lines:
        section = re.match(r"\s*\[accounts\.([^\]]+)\]", line)
        if section:
            current_account = section.group(1).strip('"').strip("'")
            if not SAFE_NAME.match(current_account):
                print(
                    f"Unsafe account name '{current_account}': "
                    "only alphanumeric, hyphen, and underscore allowed",
                    file=sys.stderr,
                )
                sys.exit(1)
            accounts.append(current_account)
            result.append(line)
        elif re.match(r"\s*\[", line):
            current_account = None
            result.append(line)
        elif current_account and (m_pw := re.match(r"(\s*)password\s*=\s*\"[^\"]*\"(.*)", line)):
            indent = m_pw.group(1)
            trailing = m_pw.group(2)
            result.append(f'{indent}password = "!cat {secrets_dir}/{current_account}"{trailing}\n')
        else:
            result.append(line)

    with open(config_path, "w") as f:
        f.writelines(result)

    for name in accounts:
        print(name)


if __name__ == "__main__":
    main()
