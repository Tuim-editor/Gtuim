"""Offline marketplace lifecycle checks. Run: python -m unittest discover -s tests -p 'test_*.py'."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("gtuim_store", ROOT / "src/nvim/store_search.py")
store = importlib.util.module_from_spec(spec)
spec.loader.exec_module(store)


class PluginStoreTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gtuim-store-test-")
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        store.TUIM_DIR = root / "data"
        store.TUIM_CONFIG_DIR = root / "config"
        store.DB_PATH = store.TUIM_DIR / "store_db.json"
        store.USER_PLUGINS_PATH = store.TUIM_DIR / "user_plugins.json"
        store.STATE_PATH = store.TUIM_DIR / "plugin_states.json"
        store.INVENTORY_PATH = store.TUIM_DIR / "plugin_inventory.json"

    def inventory(self, entries):
        store.write_json(store.INVENTORY_PATH, entries)
        for entry in entries:
            (store.TUIM_DIR / "lazy" / entry["name"]).mkdir(parents=True)

    def test_add_persists_pending_plugin_and_configuration_survives_removal(self):
        repo = "fixture/smoke.nvim"
        config = store.TUIM_CONFIG_DIR / "plugins/fixture_smoke.nvim.lua"
        config.parent.mkdir(parents=True)
        config.write_text("return { opts = {} }\n")
        self.assertTrue(store.change_plugin("add", repo)["success"])
        plugin = store.inventory()[repo]
        self.assertTrue(plugin["installed"])
        self.assertIn("pending", plugin["status"].lower())
        self.assertEqual(plugin["config_path"], str(config))
        store.change_plugin("remove", repo)
        self.assertTrue(config.is_file())
        self.assertFalse(store.inventory()[repo]["installed"])

    def test_dependency_cannot_be_disabled_before_its_parent(self):
        self.inventory([
            {"name": "parent.nvim", "full_name": "fixture/parent.nvim", "source": "Bundled"},
            {"name": "dep.nvim", "full_name": "fixture/dep.nvim", "source": "Dependency",
             "required_by": ["fixture/parent.nvim"]},
        ])
        with self.assertRaisesRegex(ValueError, "Required by"):
            store.change_plugin("disable", "fixture/dep.nvim")
        store.change_plugin("disable", "fixture/parent.nvim")
        self.assertTrue(store.change_plugin("disable", "fixture/dep.nvim")["success"])
        with self.assertRaisesRegex(ValueError, "dependencies first"):
            store.change_plugin("enable", "fixture/parent.nvim")

    def test_installed_listing_never_needs_catalog_or_network(self):
        self.inventory([{"name": "smoke.nvim", "full_name": "fixture/smoke.nvim", "source": "Bundled"}])
        original = store.download_db
        def no_network():
            self.fail("installed plugin list attempted a catalog download")
        store.download_db = no_network
        try:
            plugins = store.search("", "installed")
        finally:
            store.download_db = original
        self.assertEqual([p["full_name"] for p in plugins], ["fixture/smoke.nvim"])

    def test_manager_and_invalid_repository_are_protected(self):
        for repo in ("folke/lazy.nvim", "../bad", "fixture/../../bad", "https://github.com/fixture/plugin"):
            with self.subTest(repo=repo), self.assertRaises(ValueError):
                store.change_plugin("add", repo)
        self.assertFalse(store.STATE_PATH.exists())

    def test_module_detection_and_colorschemes(self):
        plugin = store.TUIM_DIR / "lazy/smoke.nvim"
        (plugin / "lua/smoke").mkdir(parents=True)
        (plugin / "lua/smoke/init.lua").write_text("return {}\n")
        (plugin / "colors").mkdir()
        (plugin / "colors/smoke.lua").write_text("-- fixture\n")
        self.assertEqual(store.detect_lua_module(plugin, "fixture/smoke.nvim"), "smoke")
        self.assertEqual(store.detect_colorschemes(plugin), ["smoke"])

    def test_supported_search_works_offline_and_returns_curated_recipes(self):
        original = store.download_db
        def no_network():
            self.fail("supported recipe list attempted a catalog download")
        store.download_db = no_network
        try:
            items = store.search("", "supported")
        finally:
            store.download_db = original
        repos = {p["full_name"]: p for p in items}
        for expected in (
            "blazkowolf/gruber-darker.nvim",
            "lewis6991/gitsigns.nvim",
            "numToStr/Comment.nvim",
            "windwp/nvim-autopairs",
        ):
            self.assertIn(expected, repos)
            self.assertTrue(repos[expected]["supported"])
            self.assertTrue(repos[expected]["recipe_id"])
            self.assertEqual(repos[expected]["lifecycle"], "not_installed")
        self.assertEqual(repos["blazkowolf/gruber-darker.nvim"]["pin_source"], "recipe")

    def test_configure_validates_types_bounds_and_persists_atomically(self):
        repo = "blazkowolf/gruber-darker.nvim"
        self.inventory([{"name": "gruber-darker.nvim", "full_name": repo, "source": "Marketplace", "lifecycle": "verified"}])
        res = store.configure_plugin(repo, '{"bold": false, "italic.strings": false}')
        self.assertTrue(res["success"])
        entry = store.inventory()[repo]
        self.assertTrue(entry["configured"])
        self.assertEqual(entry["lifecycle"], "pending_restart")
        self.assertEqual(entry["recipe_values"]["italic.strings"], False)

        with self.assertRaisesRegex(ValueError, "Unknown option"):
            store.configure_plugin(repo, '{"not_a_real_option": true}')
        with self.assertRaisesRegex(ValueError, "must be a boolean"):
            store.configure_plugin(repo, '{"bold": "yes"}')
        with self.assertRaisesRegex(ValueError, "must be an integer"):
            store.configure_plugin("lewis6991/gitsigns.nvim", '{"update_debounce": true}')
        with self.assertRaisesRegex(ValueError, "must be >= 20"):
            store.configure_plugin("lewis6991/gitsigns.nvim", '{"update_debounce": 5}')

        reset = store.reset_plugin_options(repo)
        self.assertTrue(reset["success"])
        self.assertFalse(store.inventory()[repo]["configured"])

    def test_pin_and_unpin_resolve_lockfile_commit_and_mark_pending_restart(self):
        repo = "numToStr/Comment.nvim"
        self.inventory([{"name": "Comment.nvim", "full_name": repo, "source": "Marketplace", "lifecycle": "verified"}])
        sha = "e30b7f2008e52442154b66f7c519bfd2f1e32acb"
        store.write_json(store.lockfile_path(), {"Comment.nvim": {"branch": "master", "commit": sha}})
        res = store.pin_plugin(repo)
        self.assertTrue(res["success"])
        self.assertEqual(res["commit"], sha)
        entry = store.inventory()[repo]
        self.assertEqual(entry["pinned_commit"], sha)
        self.assertEqual(entry["pin_source"], "user")
        self.assertEqual(entry["lifecycle"], "pending_restart")

        unpin = store.unpin_plugin(repo)
        self.assertTrue(unpin["success"])
        entry = store.inventory()[repo]
        self.assertEqual(entry["pin_source"], "recipe")
        self.assertEqual(entry["pinned_commit"], "v0.8.0")

        with self.assertRaisesRegex(ValueError, "installed"):
            store.pin_plugin("windwp/nvim-autopairs")

    def test_verify_reports_missing_requirements_and_health(self):
        repo = "lewis6991/gitsigns.nvim"
        self.inventory([{"name": "gitsigns.nvim", "full_name": repo, "source": "Marketplace", "lifecycle": "verified"}])
        orig_which = store.shutil.which
        store.shutil.which = lambda cmd: None if cmd == "git" else orig_which(cmd)
        try:
            res = store.verify_plugin(repo)
        finally:
            store.shutil.which = orig_which
        self.assertTrue(res["success"])
        self.assertEqual(res["lifecycle"], "needs_attention")
        self.assertTrue(res["missing_requirements"])
        self.assertIn("Missing requirement", res["health_message"])
        self.assertEqual(res["message"], res["health_message"])

    def test_lua_pin_override_is_preserved_over_recipe_pin(self):
        repo = "numToStr/Comment.nvim"
        self.inventory([{
            "name": "Comment.nvim",
            "full_name": repo,
            "source": "Marketplace",
            "lifecycle": "verified",
            "pin_source": "lua",
            "pinned_commit": "v0.8.1-custom",
        }])
        entry = store.inventory()[repo]
        self.assertEqual(entry["pin_source"], "lua")
        self.assertEqual(entry["pinned_commit"], "v0.8.1-custom")
        supported = {p["full_name"]: p for p in store.search("", "supported")}
        self.assertEqual(supported[repo]["pin_source"], "lua")
        self.assertEqual(supported[repo]["pinned_commit"], "v0.8.1-custom")


if __name__ == "__main__":
    unittest.main()
