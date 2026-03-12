#!/usr/bin/env python3
"""Rewrite colporteur config passwords to use sidecar secret files.

Reads a colporteur config.toml, finds [accounts.*] sections, replaces
password values with '!cat <secrets_dir>/<account_name>', and outputs
a JSON object mapping account names to their original passwords.
"""

import json
import re
import subprocess
import sys

SAFE_NAME = re.compile(r"^[A-Za-z0-9_.@-]+$")


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
    accounts = {}

    for line in lines:
        section = re.match(r"\s*\[accounts\.([^\]]+)\]", line)
        if section:
            current_account = section.group(1).strip('"').strip("'")
            if not SAFE_NAME.match(current_account):
                print(
                    f"Unsafe account name '{current_account}': "
                    "only alphanumeric, dot, at-sign, hyphen, and underscore allowed",
                    file=sys.stderr,
                )
                sys.exit(1)
            result.append(line)
        elif re.match(r"\s*\[", line):
            current_account = None
            result.append(line)
        elif current_account and (m_pw := re.match(r'(\s*)password\s*=\s*"([^"]*)"(.*)', line)):
            indent = m_pw.group(1)
            password = m_pw.group(2)
            trailing = m_pw.group(3)
            if password.startswith("!"):
                cmd = password[1:]
                try:
                    proc = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=30)
                except subprocess.TimeoutExpired:
                    print(
                        f"Password command for account '{current_account}' timed out after 30s",
                        file=sys.stderr,
                    )
                    sys.exit(1)
                if proc.returncode != 0:
                    error_output = proc.stderr.strip() or proc.stdout.strip() or "no output from command"
                    print(
                        f"Failed to resolve password command for account '{current_account}' "
                        f"(exit code {proc.returncode}): {error_output}",
                        file=sys.stderr,
                    )
                    sys.exit(1)
                stdout = proc.stdout
                if stdout.endswith("\r\n"):
                    stdout = stdout[:-2]
                elif stdout.endswith("\n"):
                    stdout = stdout[:-1]
                password = stdout
            accounts[current_account] = password
            result.append(f'{indent}password = "!cat {secrets_dir}/{current_account}"{trailing}\n')
        else:
            result.append(line)

    with open(config_path, "w") as f:
        f.writelines(result)

    json.dump(accounts, sys.stdout)


if __name__ == "__main__":
    main()
