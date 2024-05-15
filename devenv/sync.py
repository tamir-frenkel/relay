from __future__ import annotations

from devenv.lib import config, venv
import os
import subprocess


def main(context: dict[str, str]) -> int:
    os.environ['RELAY_DEBUG'] = '1'

    reporoot = context["reporoot"]

    # configure versions for tools in devenv/config.ini

    name = "relay"

    venv_dir, python_version, requirements, editable_paths, bins = venv.get(
        reporoot, name
    )
    url, sha256 = config.get_python(reporoot, python_version)
    print(f"ensuring {name} venv at {venv_dir}...")
    venv.ensure(venv_dir, python_version, url, sha256)

    print(f"syncing {name} with {requirements}...")
    venv.sync(reporoot, venv_dir, requirements, editable_paths, bins)

    return 0
