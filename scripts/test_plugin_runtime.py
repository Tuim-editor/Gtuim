#!/usr/bin/env python3
"""Run plugin runtime integration checks without network or user-config changes."""
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run_scenario(scenario):
    with tempfile.TemporaryDirectory(prefix="gtuim-plugin-test-") as temp:
        env = os.environ.copy()
        env.update({
            "HOME": temp,
            "XDG_CONFIG_HOME": f"{temp}/config",
            "XDG_DATA_HOME": f"{temp}/data",
            "XDG_STATE_HOME": f"{temp}/state",
            "XDG_CACHE_HOME": f"{temp}/cache",
            "NVIM_APPNAME": "tuim",
            "GTUIM_TEST_ROOT": str(ROOT),
            "GTUIM_TEST_SCENARIO": scenario,
            "TUIM_SKIP_ONBOARDING": "1",
        })
        env.pop("TUIM_DISABLE_PLUGINS", None)
        if scenario == "safe_mode":
            env["TUIM_DISABLE_PLUGINS"] = "1"
        subprocess.run(
            ["nvim", "--headless", "-u", "NONE", "-i", "NONE", "-l",
             str(ROOT / "tests/plugin_runtime_smoke.lua")],
            env=env, cwd=ROOT, check=True, timeout=20,
        )


if __name__ == "__main__":
    for scenario in (
        "automatic",
        "override",
        "bad_config",
        "setup_error",
        "require_error",
        "safe_mode",
        "bootstrap_timeout",
        "recipe_options",
        "recipe_override_precedence",
        "recipe_verify_fail",
        "unmatched_module_and_theme_sync",
    ):
        run_scenario(scenario)
