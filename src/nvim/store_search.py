#!/usr/bin/env python3
"""Tuim marketplace and offline installed-plugin inventory."""
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time
import unicodedata
import urllib.parse
import urllib.request

TUIM_DIR = pathlib.Path(os.environ.get("XDG_DATA_HOME", pathlib.Path.home() / ".local/share")) / "tuim"
TUIM_CONFIG_DIR = pathlib.Path(os.environ.get("XDG_CONFIG_HOME", pathlib.Path.home() / ".config")) / "tuim"
DB_PATH = TUIM_DIR / "store_db.json"
USER_PLUGINS_PATH = TUIM_DIR / "user_plugins.json"
STATE_PATH = TUIM_DIR / "plugin_states.json"
INVENTORY_PATH = TUIM_DIR / "plugin_inventory.json"
MAX_CATALOG_BYTES = 16 * 1024 * 1024
MAX_CATALOG_ITEMS = 50_000
MAX_QUERY_CHARS = 256
SHA_RE = re.compile(r"^[0-9a-fA-F]{7,40}$")


def recipe_options_path():
    return TUIM_DIR / "recipe_options.json"


def pins_path():
    return TUIM_DIR / "plugin_pins.json"


def lockfile_path():
    return TUIM_DIR / "lazy-lock.json"


def pending_restart_path():
    return TUIM_DIR / "plugin_pending_restart.json"


def recipes_path():
    candidate = TUIM_DIR / "plugin_recipes.json"
    if candidate.is_file():
        return candidate
    return pathlib.Path(__file__).resolve().parent / "plugin_recipes.json"


def load_recipes_catalog():
    path = recipes_path()
    if not path.is_file():
        return {"version": 1, "recipes": []}
    with path.open(encoding="utf-8") as file:
        data = json.load(file)
    if not isinstance(data, dict) or not isinstance(data.get("recipes"), list):
        raise ValueError("Invalid plugin_recipes.json catalog")
    recipes = []
    for item in data["recipes"]:
        if isinstance(item, dict) and valid_repo(item.get("repo")):
            recipes.append(item)
    return {"version": data.get("version", 1), "recipes": recipes}


def recipes_by_repo():
    return {r["repo"]: r for r in load_recipes_catalog()["recipes"]}


def check_requirements(recipe):
    if not isinstance(recipe, dict):
        return []
    missing = []
    for req in recipe.get("requirements", []):
        if isinstance(req, dict):
            cmd = req.get("command", "")
            if isinstance(cmd, str) and cmd and shutil.which(cmd) is None:
                missing.append(req.get("label") or cmd)
    return missing


def resolve_installed_commit(plugin_name, plugin_dir, lockfile=None):
    if lockfile is None:
        try:
            lockfile = read_json(lockfile_path(), {})
        except ValueError:
            lockfile = {}
    lock_entry = lockfile.get(plugin_name)
    if isinstance(lock_entry, dict):
        sha = lock_entry.get("commit", "")
        if isinstance(sha, str) and SHA_RE.fullmatch(sha):
            return sha
    if plugin_dir.is_dir() and (plugin_dir / ".git").exists():
        try:
            out = subprocess.run(
                ["git", "-C", str(plugin_dir), "rev-parse", "HEAD"],
                capture_output=True,
                text=True,
                timeout=3,
                check=False,
            )
            sha = out.stdout.strip()
            if out.returncode == 0 and SHA_RE.fullmatch(sha):
                return sha
        except Exception:
            pass
    return ""


def mark_pending_restart(repo):
    pending = read_json(pending_restart_path(), [])
    if repo not in pending:
        pending.append(repo)
        write_json(pending_restart_path(), pending)


def detect_colorschemes(plugin_dir):
    colors_dir = plugin_dir / "colors"
    if not colors_dir.is_dir():
        return []
    schemes = []
    seen = set()
    for path in sorted(colors_dir.iterdir()):
        if path.is_file() and path.suffix in (".lua", ".vim"):
            if path.stem not in seen:
                seen.add(path.stem)
                schemes.append(path.stem)
    return schemes


def detect_lua_module(plugin_dir, repo):
    lua_dir = plugin_dir / "lua"
    if not lua_dir.is_dir():
        return ""
    available = {}
    lower_map = {}
    for path in sorted(lua_dir.iterdir()):
        mod = None
        if path.is_dir() and (path / "init.lua").is_file():
            mod = path.name
        elif path.is_file() and path.suffix == ".lua":
            mod = path.stem
        if mod and mod != "health":
            available[mod] = True
            lower_map[mod.lower()] = mod
    if not available:
        return ""
    repo_name = repo.rsplit("/", 1)[-1].removesuffix(".git")
    stripped = re.sub(r"(\.nvim|\.lua|-nvim)$", "", repo_name)
    stripped = re.sub(r"^(nvim-|vim-)", "", stripped)
    candidates = [
        repo_name,
        re.sub(r"(\.nvim|\.lua)$", "", repo_name),
        stripped,
        stripped.replace("-", "_"),
        stripped.replace("_", "-"),
    ]
    for cand in candidates:
        if cand in available:
            return cand
        if cand.lower() in lower_map:
            return lower_map[cand.lower()]
    if len(available) == 1:
        return next(iter(available))
    return ""


def resolve_config_path(repo):
    if not valid_repo(repo):
        return ""
    sanitized = repo.replace("/", "_")
    primary = TUIM_CONFIG_DIR / "plugins" / f"{sanitized}.lua"
    if primary.is_file():
        return str(primary)
    legacy = TUIM_DIR / "plugin_configs" / f"{sanitized}.lua"
    if legacy.is_file():
        return str(legacy)
    return ""


def valid_repo(repo):
    if not isinstance(repo, str) or repo.count("/") != 1:
        return False
    owner, name = repo.split("/")
    return (1 <= len(owner) <= 39 and owner[0].isalnum() and owner[-1].isalnum()
            and re.fullmatch(r"[A-Za-z0-9-]+", owner) is not None
            and re.fullmatch(r"[A-Za-z0-9_.-]{1,100}", name) is not None
            and name not in (".", ".."))


def display_text(value, limit):
    if not isinstance(value, str) or len(value) > limit:
        raise ValueError("Invalid catalog text")
    return "".join(" " if unicodedata.category(ch) in ("Cc", "Cf", "Cs") else ch for ch in value)


def validate_catalog(data):
    if not isinstance(data, dict) or not isinstance(data.get("items"), list):
        raise ValueError("Invalid marketplace catalog")
    if len(data["items"]) > MAX_CATALOG_ITEMS:
        raise ValueError("Marketplace catalog has too many entries")
    items, seen = [], set()
    for entry in data["items"]:
        if not isinstance(entry, dict) or not valid_repo(entry.get("full_name")):
            continue
        repo = entry["full_name"]
        if repo.lower() in seen:
            continue
        try:
            description = entry.get("description")
            description = display_text("" if description is None else description, 4096)
            tags = entry.get("tags", [])
            stars = entry.get("stars", {})
            if not isinstance(tags, list) or len(tags) > 64 or not isinstance(stars, dict):
                continue
            tags = [display_text(tag, 64) for tag in tags]
            stars = stars.get("curr", 0)
            if type(stars) is not int or not 0 <= stars <= 1_000_000_000:
                continue
        except ValueError:
            continue
        # Derive the display name from the validated identity. Ignore remote
        # URLs, code, installed flags and other untrusted catalog fields.
        items.append(dict(name=repo.split("/")[1], full_name=repo,
                          description=description, tags=tags, stars={"curr": stars}))
        seen.add(repo.lower())
    if data["items"] and not items:
        raise ValueError("Marketplace catalog has no valid entries")
    return {"items": items}


def load_catalog(stream):
    raw = stream.read(MAX_CATALOG_BYTES + 1)
    if len(raw) > MAX_CATALOG_BYTES:
        raise ValueError("Marketplace catalog exceeds the size limit")
    return validate_catalog(json.loads(raw))


class HTTPSRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urllib.parse.urlsplit(newurl).scheme != "https":
            raise ValueError("Catalog redirects must use HTTPS")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def read_json(path, default):
    if not path.exists():
        return default
    with path.open(encoding="utf-8") as file:
        value = json.load(file)
    if not isinstance(value, type(default)):
        raise ValueError(f"Invalid data in {path.name}; repair this file first")
    return value


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(dir=path.parent, prefix=path.name, suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as file:
            json.dump(value, file, indent=2)
            file.write("\n")
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def download_db():
    url = "https://github.com/alex-popov-tech/store.nvim.crawler/releases/latest/download/db_minified.json"
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "Tuim"})
        opener = urllib.request.build_opener(HTTPSRedirectHandler())
        with opener.open(request, timeout=10) as response:
            data = load_catalog(response)
        # Normalization/JSON escaping must not create a cache that exceeds the
        # same limit enforced when it is read on subsequent searches.
        if len((json.dumps(data, indent=2) + "\n").encode("utf-8")) > MAX_CATALOG_BYTES:
            raise ValueError("Normalized marketplace catalog exceeds the size limit")
        write_json(DB_PATH, data)
        return True
    except Exception as error:
        sys.stderr.write(f"Catalog refresh failed: {error}\n")
        return False


def inventory():
    states = read_json(STATE_PATH, {})
    saved_recipe_opts = read_json(recipe_options_path(), {})
    pins = read_json(pins_path(), {})
    lockfile = read_json(lockfile_path(), {})
    pending_repos = set(read_json(pending_restart_path(), []))
    recipes = recipes_by_repo()

    entries = {p["full_name"]: dict(p) for p in read_json(INVENTORY_PATH, [])}
    for repo in read_json(USER_PLUGINS_PATH, []):
        entries.setdefault(repo, {"name": repo.rsplit("/", 1)[-1], "full_name": repo, "source": "Marketplace"})
    known_names = {p["name"] for p in entries.values()}
    root = TUIM_DIR / "lazy"
    if root.is_dir():
        for path in sorted(root.iterdir()):
            if path.is_dir() and path.name not in known_names and not path.name.startswith("."):
                # Preserve visibility of local/orphaned plugins without inventing a repository.
                entries["local:" + path.name] = {"name": path.name, "full_name": "local:" + path.name,
                    "source": "Unmanaged", "description": "Local plugin; manage its source with Lazy."}
    for repo, entry in entries.items():
        state = states.get(repo, "enabled")
        recipe = recipes.get(repo)
        plugin_dir = root / entry["name"]
        on_disk = plugin_dir.is_dir()
        runtime_enabled = entry.get("enabled", True)
        config_path = entry.get("config_path") or resolve_config_path(repo)
        colorschemes = entry.get("colorschemes") or (detect_colorschemes(plugin_dir) if on_disk else [])
        lua_module = entry.get("lua_module") or (recipe.get("module", "") if recipe else "") or (detect_lua_module(plugin_dir, repo) if on_disk else "")
        auto_init = entry.get("auto_init") or ("custom" if config_path else ("colorscheme" if colorschemes else ""))
        pending_removal = state == "removed" and on_disk
        installed = state != "removed" and (on_disk or repo in states or entry.get("source") in ("Marketplace", "User config"))
        enabled = state == "enabled"

        repo_opts = saved_recipe_opts.get(repo, {}) if isinstance(saved_recipe_opts.get(repo), dict) else {}
        configured = bool(config_path) or bool(repo_opts)
        user_pin = pins.get(repo, "")
        recipe_pin = (recipe.get("pin") or "") if recipe else ""
        runtime_pin_source = entry.get("pin_source", "")
        runtime_pinned = entry.get("pinned_commit", "")
        if isinstance(user_pin, str) and SHA_RE.fullmatch(user_pin):
            pinned_commit = user_pin
            pin_source = "user"
        elif runtime_pin_source == "lua":
            pinned_commit = runtime_pinned or "Lua-managed"
            pin_source = "lua"
        elif recipe_pin:
            pinned_commit = recipe_pin
            pin_source = "recipe"
        else:
            pinned_commit = ""
            pin_source = ""

        installed_commit = entry.get("installed_commit") or (resolve_installed_commit(entry["name"], plugin_dir, lockfile) if on_disk else "")
        missing_reqs = check_requirements(recipe) if recipe else list(entry.get("missing_requirements") or [])

        if state == "removed":
            status = "Removal pending restart" if on_disk else "Not installed"
        elif state == "disabled":
            status = "Disabled"
        elif not on_disk:
            status = "Install pending restart"
        else:
            status = "Enabled"
        if state != "removed" and enabled != runtime_enabled:
            status += " (restart)"
        elif repo in pending_repos and installed and on_disk and state != "removed":
            status += " (restart)"

        runtime_lifecycle = entry.get("lifecycle", "")
        runtime_health = entry.get("health_message", "")
        if state == "removed":
            lifecycle = "pending_restart" if on_disk else "not_installed"
            health_message = "Removal pending restart" if on_disk else "Not installed"
        elif not installed:
            lifecycle = "not_installed"
            health_message = "Not installed"
        elif not on_disk or enabled != runtime_enabled or repo in pending_repos:
            lifecycle = "pending_restart"
            health_message = "Restart Neovim to apply saved plugin changes"
        elif missing_reqs:
            lifecycle = "needs_attention"
            health_message = "Missing requirement(s): " + ", ".join(missing_reqs)
        elif runtime_lifecycle in ("verified", "initialized", "needs_attention", "dependencies_ready", "downloaded"):
            lifecycle = runtime_lifecycle
            health_message = runtime_health or status
        elif not enabled:
            lifecycle = "downloaded"
            health_message = "Disabled"
        elif entry.get("loaded"):
            lifecycle = "initialized"
            health_message = runtime_health or "Initialized"
        else:
            lifecycle = "dependencies_ready"
            health_message = runtime_health or "Ready (lazy-loaded)"

        entry.update(
            installed=installed,
            enabled=enabled,
            stars=0,
            protected=repo == "folke/lazy.nvim" or repo.startswith("local:"),
            pending_removal=pending_removal,
            config_path=config_path,
            has_config=bool(config_path),
            colorschemes=colorschemes,
            lua_module=lua_module,
            auto_init=auto_init,
            status=status,
            supported=recipe is not None,
            recipe_id=recipe["id"] if recipe else "",
            lifecycle=lifecycle,
            health_message=health_message,
            installed_commit=installed_commit,
            pinned_commit=pinned_commit,
            pin_source=pin_source,
            configured=configured,
            recipe_values=repo_opts,
            missing_requirements=missing_reqs,
        )
        required = entry.get("required_by", [])
        details = [entry.get("source", "Plugin"), status]
        if recipe is not None:
            details.append("Supported")
        if configured:
            details.append("Configured")
        elif auto_init and auto_init not in ("bundled", "pending"):
            details.append("Init: " + auto_init)
        if colorschemes:
            details.append("Theme: " + ", ".join(colorschemes))
        if required:
            details.append("Required by: " + ", ".join(required))
        entry["description"] = " | ".join(details)
    return entries


def _merge_plugin_center_fields(repo, local, recipes, saved_recipe_opts, pins):
    recipe = recipes.get(repo)
    repo_opts = saved_recipe_opts.get(repo, {}) if isinstance(saved_recipe_opts.get(repo), dict) else {}
    user_pin = pins.get(repo, "")
    recipe_pin = (recipe.get("pin") or "") if recipe else ""
    if isinstance(user_pin, str) and SHA_RE.fullmatch(user_pin):
        pinned_commit, pin_source = user_pin, "user"
    elif local.get("pin_source") == "lua":
        pinned_commit, pin_source = (local.get("pinned_commit") or "Lua-managed"), "lua"
    elif recipe_pin:
        pinned_commit, pin_source = recipe_pin, "recipe"
    else:
        pinned_commit, pin_source = "", ""
    missing_reqs = local.get("missing_requirements")
    if missing_reqs is None:
        missing_reqs = check_requirements(recipe) if recipe else []
    return dict(
        supported=recipe is not None,
        recipe_id=recipe["id"] if recipe else "",
        lifecycle=local.get("lifecycle", "not_installed"),
        health_message=local.get("health_message", "Missing requirement(s): " + ", ".join(missing_reqs) if missing_reqs else "Not installed"),
        installed_commit=local.get("installed_commit", ""),
        pinned_commit=local.get("pinned_commit", pinned_commit),
        pin_source=local.get("pin_source", pin_source),
        configured=local.get("configured", bool(repo_opts) or bool(local.get("config_path"))),
        recipe_values=local.get("recipe_values", repo_opts),
        missing_requirements=missing_reqs,
    )


def search(query, category="all"):
    if not isinstance(query, str) or len(query) > MAX_QUERY_CHARS:
        raise ValueError("Search query is too long")
    installed = inventory()
    recipes = recipes_by_repo()
    saved_recipe_opts = read_json(recipe_options_path(), {})
    pins = read_json(pins_path(), {})

    if category == "installed":
        # No catalog or network required, and no 50-item cap.
        items = [p for p in installed.values() if p["installed"] or p["status"] == "Removal pending restart"]
        results = sorted(items, key=lambda p: p["name"].lower())
    elif category == "supported":
        # Curated recipes work completely offline with no catalog download.
        results = []
        for recipe in load_recipes_catalog()["recipes"]:
            repo = recipe["repo"]
            local = installed.get(repo, {})
            entry = dict(
                name=recipe.get("name") or repo.split("/", 1)[1],
                full_name=repo,
                stars=0,
                description=recipe.get("description") or "",
                installed=local.get("installed", False),
                enabled=local.get("enabled", True),
                protected=local.get("protected", False),
                status=local.get("status", "Not installed"),
                pending_removal=local.get("pending_removal", False),
                source=local.get("source", "Supported"),
                config_path=local.get("config_path", resolve_config_path(repo)),
                has_config=local.get("has_config", bool(resolve_config_path(repo))),
                colorschemes=local.get("colorschemes", []),
                lua_module=local.get("lua_module", recipe.get("module", "")),
                auto_init=local.get("auto_init", ""),
            )
            entry.update(_merge_plugin_center_fields(repo, local, recipes, saved_recipe_opts, pins))
            results.append(entry)
    else:
        if not DB_PATH.exists() or time.time() - DB_PATH.stat().st_mtime > 86400:
            download_db()  # A stale cache remains usable offline.
        if DB_PATH.exists():
            with DB_PATH.open("rb") as stream:
                items = load_catalog(stream)["items"]
        else:
            items = []
        tags = {"colorscheme": ["colorscheme", "theme", "color-scheme"], "ai": ["ai", "llm"],
                "treesitter": ["treesitter", "tree-sitter"], "telescope": ["telescope", "telescope-extension"]}
        if category != "all":
            items = [p for p in items if any(t.lower() in tags.get(category, [category]) for t in p.get("tags", []))]
        results = []
        seen_repos = set()
        for item in sorted(items, key=lambda p: p.get("stars", {}).get("curr", 0), reverse=True):
            repo = item.get("full_name", "")
            seen_repos.add(repo.lower())
            local = installed.get(repo, {})
            entry = dict(
                name=item.get("name", ""),
                full_name=repo,
                stars=item.get("stars", {}).get("curr", 0),
                description=item.get("description") or "",
                installed=local.get("installed", False),
                enabled=local.get("enabled", True),
                protected=local.get("protected", False),
                status=local.get("status", "Not installed"),
                pending_removal=local.get("pending_removal", False),
                source=local.get("source", "Marketplace"),
                config_path=local.get("config_path", ""),
                has_config=local.get("has_config", False),
                colorschemes=local.get("colorschemes", []),
                lua_module=local.get("lua_module", ""),
                auto_init=local.get("auto_init", ""),
            )
            entry.update(_merge_plugin_center_fields(repo, local, recipes, saved_recipe_opts, pins))
            results.append(entry)
        direct = query.strip()
        if valid_repo(direct) and direct.lower() not in seen_repos:
            local = installed.get(direct, {})
            entry = dict(
                name=direct.split("/", 1)[1],
                full_name=direct,
                stars=0,
                description="Install directly from GitHub (https://github.com/" + direct + ")",
                installed=local.get("installed", False),
                enabled=local.get("enabled", True),
                protected=local.get("protected", False),
                status=local.get("status", "Not installed"),
                pending_removal=local.get("pending_removal", False),
                source=local.get("source", "GitHub"),
                config_path=local.get("config_path", ""),
                has_config=local.get("has_config", False),
                colorschemes=local.get("colorschemes", []),
                lua_module=local.get("lua_module", ""),
                auto_init=local.get("auto_init", ""),
            )
            entry.update(_merge_plugin_center_fields(direct, local, recipes, saved_recipe_opts, pins))
            results.insert(0, entry)
    query_cf = query.casefold()
    results = [p for p in results if query_cf in (p["name"] + " " + p["full_name"] + " " + p["description"]).casefold()]
    return results if category in ("installed", "supported") else results[:50]


def validate_recipe_options(recipe, raw_values):
    if not isinstance(raw_values, dict):
        raise ValueError("Recipe options must be a JSON object")
    fields = {f["key"]: f for f in recipe.get("fields", []) if isinstance(f, dict) and "key" in f}
    cleaned = {}
    for key, val in raw_values.items():
        if key not in fields:
            raise ValueError(f"Unknown option '{key}' for recipe {recipe['id']}")
        field = fields[key]
        ftype = field.get("type")
        if ftype == "boolean":
            if type(val) is not bool:
                raise ValueError(f"Option '{key}' must be a boolean")
        elif ftype == "integer":
            if type(val) is not int:
                raise ValueError(f"Option '{key}' must be an integer")
            lo, hi = field.get("min"), field.get("max")
            if lo is not None and val < lo:
                raise ValueError(f"Option '{key}' must be >= {lo}")
            if hi is not None and val > hi:
                raise ValueError(f"Option '{key}' must be <= {hi}")
        elif ftype == "string":
            if type(val) is not str or len(val) > 512:
                raise ValueError(f"Option '{key}' must be a string (<= 512 chars)")
            if any(unicodedata.category(ch) in ("Cc", "Cf", "Cs") for ch in val):
                raise ValueError(f"Option '{key}' contains invalid control characters")
        elif ftype == "enum":
            choices = field.get("choices") or []
            if type(val) is not str or val not in choices:
                raise ValueError(f"Option '{key}' must be one of: {', '.join(choices)}")
        else:
            raise ValueError(f"Unsupported field type '{ftype}'")
        cleaned[key] = val
    return cleaned


def configure_plugin(repo, raw_json):
    if not valid_repo(repo):
        raise ValueError("Invalid GitHub repository identity")
    recipe = recipes_by_repo().get(repo)
    if not recipe:
        raise ValueError(f"No curated recipe found for {repo}")
    try:
        parsed = json.loads(raw_json) if isinstance(raw_json, str) else raw_json
    except json.JSONDecodeError as err:
        raise ValueError(f"Invalid options JSON: {err}") from err
    validated = validate_recipe_options(recipe, parsed)
    all_opts = read_json(recipe_options_path(), {})
    all_opts[repo] = validated
    write_json(recipe_options_path(), all_opts)
    entries = inventory()
    if repo in entries and entries[repo]["installed"]:
        mark_pending_restart(repo)
    return {
        "success": True,
        "message": "Recipe options saved. Restart Neovim to apply.",
        "values": validated,
    }


def reset_plugin_options(repo):
    if not valid_repo(repo):
        raise ValueError("Invalid GitHub repository identity")
    all_opts = read_json(recipe_options_path(), {})
    all_opts.pop(repo, None)
    write_json(recipe_options_path(), all_opts)
    entries = inventory()
    if repo in entries and entries[repo]["installed"]:
        mark_pending_restart(repo)
    return {"success": True, "message": "Recipe options reset to defaults. Restart Neovim to apply."}


def pin_plugin(repo):
    if not valid_repo(repo):
        raise ValueError("Invalid GitHub repository identity")
    entries = inventory()
    entry = entries.get(repo)
    if not entry or not entry["installed"]:
        raise ValueError("Plugin must be installed before pinning its commit")
    plugin_dir = TUIM_DIR / "lazy" / entry["name"]
    commit = resolve_installed_commit(entry["name"], plugin_dir)
    if not commit:
        raise ValueError("Unable to resolve an installed git commit for " + repo)
    pins = read_json(pins_path(), {})
    pins[repo] = commit
    write_json(pins_path(), pins)
    mark_pending_restart(repo)
    return {
        "success": True,
        "commit": commit,
        "message": f"Pinned {repo} to {commit[:12]}. Restart Neovim to enforce pin.",
    }


def unpin_plugin(repo):
    if not valid_repo(repo):
        raise ValueError("Invalid GitHub repository identity")
    pins = read_json(pins_path(), {})
    if repo in pins:
        del pins[repo]
        write_json(pins_path(), pins)
        entries = inventory()
        if repo in entries and entries[repo]["installed"]:
            mark_pending_restart(repo)
    return {"success": True, "message": f"Removed custom pin for {repo}. Restart Neovim to apply."}


def verify_plugin(repo):
    if not valid_repo(repo):
        raise ValueError("Invalid GitHub repository identity")
    entries = inventory()
    recipes = recipes_by_repo()
    recipe = recipes.get(repo)
    entry = entries.get(repo)
    missing_reqs = check_requirements(recipe) if recipe else []

    if not entry or not entry["installed"]:
        msg = "Missing requirement(s): " + ", ".join(missing_reqs) if missing_reqs else "Not installed"
        return {
            "success": True,
            "repo": repo,
            "lifecycle": "not_installed",
            "health_message": msg,
            "message": msg,
            "missing_requirements": missing_reqs,
            "installed_commit": "",
            "pinned_commit": (recipe.get("pin") or "") if recipe else "",
            "pin_source": "recipe" if (recipe and recipe.get("pin")) else "",
            "configured": False,
        }

    plugin_dir = TUIM_DIR / "lazy" / entry["name"]
    on_disk = plugin_dir.is_dir()
    missing_deps = []
    if recipe:
        for dep_repo in recipe.get("dependencies", []):
            dep = entries.get(dep_repo)
            if not dep or not dep["installed"] or not dep["enabled"]:
                missing_deps.append(dep_repo)

    lifecycle = entry["lifecycle"]
    health_message = entry["health_message"]
    if not on_disk:
        lifecycle = "pending_restart"
        health_message = "Install pending restart"
    elif missing_reqs:
        lifecycle = "needs_attention"
        health_message = "Missing requirement(s): " + ", ".join(missing_reqs)
    elif missing_deps:
        lifecycle = "needs_attention"
        health_message = "Missing or disabled dependencies: " + ", ".join(missing_deps)
    elif recipe and lifecycle not in ("verified", "needs_attention", "pending_restart"):
        ver = recipe.get("verification", "")
        if ver.startswith("colorscheme:"):
            scheme = ver.split(":", 1)[1]
            if (plugin_dir / "colors" / f"{scheme}.lua").is_file() or (plugin_dir / "colors" / f"{scheme}.vim").is_file():
                lifecycle = "dependencies_ready" if lifecycle == "downloaded" else lifecycle
                health_message = f"Colorscheme '{scheme}' present on disk ({lifecycle})"
            else:
                lifecycle = "needs_attention"
                health_message = f"Colorscheme '{scheme}' file missing on disk"

    return {
        "success": True,
        "repo": repo,
        "lifecycle": lifecycle,
        "health_message": health_message,
        "message": health_message,
        "missing_requirements": missing_reqs,
        "installed_commit": entry.get("installed_commit", ""),
        "pinned_commit": entry.get("pinned_commit", ""),
        "pin_source": entry.get("pin_source", ""),
        "configured": entry.get("configured", False),
    }


def change_plugin(action, repo):
    if not valid_repo(repo):
        raise ValueError("Only repository plugins can be managed here")
    if repo == "folke/lazy.nvim":
        raise ValueError("Tuim needs its plugin manager")
    entries = inventory()
    entry = entries.get(repo)
    if action != "add" and (not entry or not entry["installed"]):
        raise ValueError("Plugin is not installed")
    if action in ("disable", "remove"):
        required = [r for r in entry.get("required_by", []) if entries.get(r, {}).get("installed") and entries[r]["enabled"]]
        if required:
            raise ValueError("Required by: " + ", ".join(required) + ". Disable these plugins first.")
    if action in ("add", "enable"):
        disabled_deps = [r for r, p in entries.items() if repo in p.get("required_by", []) and (not p["enabled"] or not p["installed"])]
        if disabled_deps:
            raise ValueError("Enable or reinstall dependencies first: " + ", ".join(disabled_deps))
    plugins = read_json(USER_PLUGINS_PATH, [])
    if action == "add" and (not entry or entry.get("source") in ("Marketplace", "Supported")) and repo not in plugins:
        plugins.append(repo)
        write_json(USER_PLUGINS_PATH, plugins)
    states = read_json(STATE_PATH, {})
    states[repo] = {"add": "enabled", "enable": "enabled", "disable": "disabled", "remove": "removed"}[action]
    write_json(STATE_PATH, states)
    mark_pending_restart(repo)
    if action == "add":
        return {"success": True, "message": "Saved. Restart Neovim to install & initialize; configure options anytime."}
    return {"success": True, "message": "Saved. Restart Neovim to apply; plugin configuration is kept."}


def main():
    try:
        command = sys.argv[1] if len(sys.argv) > 1 else ""
        if command == "search":
            result = search(sys.argv[2] if len(sys.argv) > 2 else "", sys.argv[3] if len(sys.argv) > 3 else "all")
        elif command == "recipes" and len(sys.argv) == 2:
            result = load_recipes_catalog()
        elif command == "configure" and len(sys.argv) == 4:
            result = configure_plugin(sys.argv[2], sys.argv[3])
        elif command == "reset_options" and len(sys.argv) == 3:
            result = reset_plugin_options(sys.argv[2])
        elif command == "verify" and len(sys.argv) == 3:
            result = verify_plugin(sys.argv[2])
        elif command == "pin" and len(sys.argv) == 3:
            result = pin_plugin(sys.argv[2])
        elif command == "unpin" and len(sys.argv) == 3:
            result = unpin_plugin(sys.argv[2])
        elif command in ("add", "remove", "enable", "disable") and len(sys.argv) == 3:
            result = change_plugin(command, sys.argv[2])
        elif command == "download":
            result = {"success": download_db()}
            if not result["success"]:
                raise ValueError("Catalog download failed")
        else:
            raise ValueError("Usage: store_search.py search|recipes|configure|reset_options|verify|pin|unpin|add|remove|enable|disable|download [args]")
        print(json.dumps(result))
    except Exception as error:
        print(json.dumps({"success": False, "message": str(error)}))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
