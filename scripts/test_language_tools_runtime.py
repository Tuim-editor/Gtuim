#!/usr/bin/env python3
"""Exercise the production Mason snapshot Lua offline, without user configuration."""
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE = (ROOT / "src/tui/widgets/mason.rs").read_text()
QUERY = re.search(r'let lua_code = r#"(.*?)"#;', SOURCE, re.S).group(1)

CASES = {
    "unavailable": """
        package.preload['mason-registry'] = function() error('fixture missing plugin') end
    """,
    "disabled": """
        vim.g.tuim_mason_enabled = false
        package.preload['mason-registry'] = function() error('fixture disabled plugin') end
    """,
    "populated": """
        package.preload['mason-registry'] = function() return {
            get_all_package_specs = function() return {
                {name='rust-analyzer', description='Rust language server', categories={'LSP'}},
                {name='stylua', description='Lua formatter', categories={'Formatter'}},
            } end,
            is_installed = function(name) return name == 'rust-analyzer' end,
        } end
    """,
    "legacy_api": """
        package.preload['mason-registry'] = function() return {
            get_all_packages = function() return {{
                name='lua-language-server',
                spec={description='Lua server', categories={'LSP'}},
                is_installed=function() return true end,
            }} end,
        } end
    """,
    "empty_registry": """
        package.preload['mason-registry'] = function() return {
            get_all_package_specs = function() return {} end,
        } end
    """,
}

CHECKS = {
    "unavailable": "assert(not p.available and p.health:find('failed to initialize', 1, true), p.health)",
    "disabled": "assert(not p.available and p.health:find('disabled', 1, true), p.health)",
    "populated": """
        assert(p.available and #p.packages == 2, vim.inspect(p))
        assert(p.packages[1].name == 'rust-analyzer' and p.packages[1].installed)
        assert(p.packages[1].categories[1] == 'LSP')
        assert(not p.packages[2].installed)
    """,
    "legacy_api": "assert(p.available and #p.packages == 1 and p.packages[1].installed)",
    "empty_registry": "assert(p.available and #p.packages == 0 and p.health:find('registry is empty', 1, true), p.health)",
}


def main():
    for name, setup in CASES.items():
        with tempfile.TemporaryDirectory(prefix="gtuim-language-tools-") as directory:
            base = Path(directory)
            env = os.environ.copy()
            env.update(HOME=str(base / "home"), NVIM_APPNAME="gtuim-test")
            for kind in ("CONFIG", "DATA", "CACHE", "STATE"):
                path = base / kind.lower()
                path.mkdir()
                env[f"XDG_{kind}_HOME"] = str(path)
            (base / "home").mkdir()
            script = base / "snapshot.lua"
            script.write_text(setup + "\nlocal p = vim.json.decode((function()\n" + QUERY
                              + "\nend)())\n" + CHECKS[name] + "\nvim.cmd('qa!')\n")
            result = subprocess.run(["nvim", "--clean", "--headless", "-l", str(script)],
                                    env=env, capture_output=True, text=True, timeout=15)
            if result.returncode:
                raise AssertionError(f"{name}: {result.stdout}\n{result.stderr}")
            print(f"PASS: language-tools snapshot ({name})")


if __name__ == "__main__":
    main()
