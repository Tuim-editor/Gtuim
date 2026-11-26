#!/usr/bin/env python3
"""Opt-in network smoke test of the actual pinned plugins, entirely in a temp HOME.

Run: python3 scripts/test_plugin_recipes_live.py --online
This downloads and executes third-party plugin code from the curated catalog.
It never installs into the user's runtime or changes the user's Git configuration.
"""
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
MAX_ARCHIVE_BYTES = 16 * 1024 * 1024


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--online", action="store_true", help="allow download and execution of pinned upstream plugins")
    args = parser.parse_args()
    if not args.online:
        parser.error("Explicit --online is required; use test_plugin_runtime.py for offline checks")
    recipes = json.loads((ROOT / "src/nvim/plugin_recipes.json").read_text())["recipes"]
    with tempfile.TemporaryDirectory(prefix="gtuim-recipes-live-") as temp:
        base = Path(temp)
        for recipe in recipes:
            pin = recipe.get("pin")
            if not pin:
                raise ValueError(f"Unpinned recipe: {recipe['repo']}")
            url = "https://codeload.github.com/" + recipe["repo"] + "/tar.gz/" + urllib.parse.quote(pin, safe="")
            print(f"Downloading {recipe['repo']} @ {pin}", flush=True)
            with urllib.request.urlopen(url, timeout=25) as response:
                raw = response.read(MAX_ARCHIVE_BYTES + 1)
            if len(raw) > MAX_ARCHIVE_BYTES:
                raise ValueError("Recipe archive exceeded size limit")
            dest = base / "plugins" / recipe["id"]
            dest.mkdir(parents=True)
            with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
                archive.extractall(dest, filter="data")
            roots = [p for p in dest.iterdir() if p.is_dir()]
            if len(roots) != 1:
                raise ValueError("Unexpected plugin archive structure")
            recipe["test_dir"] = str(roots[0])
        (base / "recipes.json").write_text(json.dumps(recipes))
        env = os.environ.copy()
        env.update(HOME=temp, XDG_CONFIG_HOME=f"{temp}/config", XDG_DATA_HOME=f"{temp}/data",
                   XDG_STATE_HOME=f"{temp}/state", XDG_CACHE_HOME=f"{temp}/cache",
                   NVIM_APPNAME="gtuim-recipe-test", GTUIM_RECIPE_TEST_DIR=temp)
        repo = base / "fixture"
        repo.mkdir()
        (repo / "example.lua").write_text("local original = 1\n")
        for command in (["init", "-q"], ["add", "example.lua"],
                        ["-c", "user.name=Recipe Test", "-c", "user.email=test@example.invalid",
                         "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]):
            subprocess.run(["git", "-C", str(repo), *command], env=env, check=True, timeout=10)
        subprocess.run(["nvim", "--headless", "-u", "NONE", "-i", "NONE", "-l",
                        str(ROOT / "tests/plugin_recipes_live.lua")], env=env, check=True, timeout=30)


if __name__ == "__main__":
    main()
