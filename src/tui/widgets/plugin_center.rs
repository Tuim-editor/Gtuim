//! Typed recipe forms. Lazy remains the installer; this layer owns user-facing options.
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Clone, Debug, Deserialize)]
pub struct RecipeCatalog {
    pub version: u32,
    pub recipes: Vec<PluginRecipe>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PluginRecipe {
    pub id: String,
    pub repo: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub module: String,
    #[serde(default)]
    pub fields: Vec<RecipeField>,
    #[serde(default)]
    pub requirements: Vec<Requirement>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub verification: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Requirement {
    pub command: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RecipeField {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub default: Value,
    #[serde(default)]
    pub choices: Vec<String>,
    pub min: Option<i64>,
    pub max: Option<i64>,
}

pub fn catalog() -> Result<RecipeCatalog, String> {
    let catalog: RecipeCatalog = serde_json::from_str(include_str!("../../nvim/plugin_recipes.json"))
        .map_err(|e| format!("Invalid bundled recipe catalog: {e}"))?;
    if catalog.version != 1 {
        return Err("Unsupported recipe catalog version".into());
    }
    Ok(catalog)
}

pub fn recipe_for(repo: &str) -> Option<PluginRecipe> {
    catalog().ok()?.recipes.into_iter().find(|r| r.repo == repo)
}

#[derive(Clone, Debug)]
pub struct RecipeForm {
    pub recipe: PluginRecipe,
    pub values: Map<String, Value>,
    pub selected: usize,
    pub dirty: bool,
    pub error: String,
    pub discard_confirm: bool,
    pub text_replace: bool,
}

impl RecipeForm {
    pub fn new(recipe: PluginRecipe, saved: Map<String, Value>) -> Self {
        let values = recipe.fields.iter().map(|field| {
            let value = saved.get(&field.key).filter(|v| field.validate(v).is_ok())
                .cloned().unwrap_or_else(|| field.default.clone());
            (field.key.clone(), value)
        }).collect();
        Self { recipe, values, selected: 0, dirty: false, error: String::new(),
            discard_confirm: false, text_replace: true }
    }

    pub fn validate(&self) -> Result<(), String> {
        for field in &self.recipe.fields {
            field.validate(self.values.get(&field.key).unwrap_or(&Value::Null))?;
        }
        Ok(())
    }

    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.recipe.fields.len().saturating_sub(1));
        self.text_replace = true;
    }

    pub fn adjust(&mut self, index: usize, direction: i64) {
        self.select(index);
        let Some(field) = self.recipe.fields.get(index) else { return; };
        let value = self.values.get(&field.key).unwrap_or(&field.default);
        let new_value = match field.kind.as_str() {
            "boolean" => Value::Bool(!value.as_bool().unwrap_or(false)),
            "integer" => Value::from(value.as_i64().unwrap_or(0).saturating_add(direction.saturating_mul(field.step()))
                .clamp(field.min.unwrap_or(i64::MIN), field.max.unwrap_or(i64::MAX))),
            "enum" if !field.choices.is_empty() => {
                let current = field.choices.iter().position(|s| Some(s.as_str()) == value.as_str()).unwrap_or(0);
                let next = (current as i64 + direction).rem_euclid(field.choices.len() as i64) as usize;
                Value::String(field.choices[next].clone())
            }
            _ => return,
        };
        self.values.insert(field.key.clone(), new_value);
        self.dirty = true;
        self.error.clear();
    }

    pub fn edit_text(&mut self, text: &str, backspace: bool) {
        let Some(field) = self.recipe.fields.get(self.selected) else { return; };
        if field.kind != "string" { return; }
        let mut current = if self.text_replace && !backspace { String::new() } else {
            self.values.get(&field.key).and_then(Value::as_str).unwrap_or("").to_string()
        };
        if backspace { current.pop(); }
        else {
            let remaining = 256usize.saturating_sub(current.chars().count());
            current.extend(text.chars().filter(|c| !c.is_control()).take(remaining));
        }
        self.text_replace = false;
        self.values.insert(field.key.clone(), Value::String(current));
        self.dirty = true;
    }

    pub fn reset_defaults(&mut self) {
        self.values = self.recipe.fields.iter().map(|f| (f.key.clone(), f.default.clone())).collect();
        self.dirty = true;
        self.error.clear();
    }
}

impl RecipeField {
    /// Coarse enough for large bounds (e.g. 500,000 lines), precise for small ranges.
    pub fn step(&self) -> i64 {
        let range = self.max.unwrap_or(0).saturating_sub(self.min.unwrap_or(0));
        if range >= 100_000 { 1000 } else if range >= 10_000 { 100 }
        else if range >= 1000 { 10 } else { 1 }
    }

    pub fn validate(&self, value: &Value) -> Result<(), String> {
        let valid = match self.kind.as_str() {
            "boolean" => value.is_boolean(),
            "integer" => value.as_i64().is_some_and(|v| v >= self.min.unwrap_or(i64::MIN) && v <= self.max.unwrap_or(i64::MAX)),
            "enum" => value.as_str().is_some_and(|v| self.choices.iter().any(|s| s == v)),
            "string" => value.as_str().is_some_and(|v| v.chars().count() <= 256 && !v.chars().any(char::is_control)),
            _ => false,
        };
        if valid { Ok(()) } else { Err(format!("Invalid value for {}", self.label)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> RecipeForm {
        RecipeForm::new(serde_json::from_value(serde_json::json!({
            "id":"fixture", "repo":"fixture/plugin", "name":"Fixture",
            "fields":[
                {"key":"enabled", "label":"Enabled", "type":"boolean", "default":true},
                {"key":"count", "label":"Count", "type":"integer", "default":2, "min":1,"max":3},
                {"key":"style", "label":"Style", "type":"enum", "default":"a", "choices":["a","b"]},
                {"key":"text", "label":"Text", "type":"string", "default":"old"}
            ]
        })).unwrap(), Map::new())
    }

    #[test]
    fn forms_toggle_cycle_clamp_and_validate() {
        let mut f = form();
        f.adjust(0, 1);
        f.adjust(1, 100);
        f.adjust(2, -1);
        assert_eq!(f.values["enabled"], false);
        assert_eq!(f.values["count"], 3);
        assert_eq!(f.values["style"], "b");
        assert!(f.validate().is_ok());
        f.values.insert("count".into(), Value::from(4));
        assert!(f.validate().is_err());
        f.reset_defaults();
        assert_eq!(f.values["count"], 2);
    }

    #[test]
    fn text_input_replaces_then_appends_and_limits_paste() {
        let mut f = form();
        f.select(3);
        f.edit_text("new", false);
        f.edit_text("!", false);
        assert_eq!(f.values["text"], "new!");
        f.edit_text("", true);
        assert_eq!(f.values["text"], "new");
        f.edit_text(&"界".repeat(300), false);
        assert_eq!(f.values["text"].as_str().unwrap().chars().count(), 256);
    }

    #[test]
    fn large_integer_fields_use_useful_steps_without_losing_bounds() {
        let mut f = form();
        f.recipe.fields[1].min = Some(1000);
        f.recipe.fields[1].max = Some(500000);
        f.values.insert("count".into(), Value::from(40000));
        f.adjust(1, 1);
        assert_eq!(f.values["count"], 41000);
        f.adjust(1, -1000);
        assert_eq!(f.values["count"], 1000);
    }

    #[test]
    fn dirty_forms_require_explicit_discard_and_capture_paste() {
        use crate::tui::{input::InputEvent, widgets::extension_shop::ExtensionShop};
        let mut shop = ExtensionShop::with_config_dir("/unused", "/unused");
        let mut draft = form();
        draft.select(3);
        shop.recipe_form = Some(draft);
        shop.handle_key(&InputEvent::Paste("changed".into()));
        assert!(shop.search_query.is_empty());
        shop.close_recipe_form();
        shop.close_recipe_form(); // repeated backdrop clicks must not discard
        assert!(shop.recipe_form.as_ref().unwrap().discard_confirm);
        shop.handle_key(&InputEvent::Key("n".into()));
        assert_eq!(shop.recipe_form.as_ref().unwrap().values["text"], "changed");
        shop.select_nav(1);
        assert!(shop.recipe_form.is_some());
        shop.close_recipe_form();
        shop.handle_key(&InputEvent::Key("y".into()));
        assert!(shop.recipe_form.is_none());
    }

    struct TestDir(std::path::PathBuf);
    impl Drop for TestDir {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    fn shop_fixture(installed: bool, success: bool) -> (crate::tui::widgets::extension_shop::ExtensionShop, TestDir) {
        use crate::tui::widgets::extension_shop::{ExtensionShop, PluginInfo};
        static ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("gtuim-center-ui-{}-{}", std::process::id(),
            ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("store_search.py"), format!(
            "import json, sys\nprint(json.dumps({{'success': {}, 'message': 'fixture result'}}) if sys.argv[1] == 'configure' else '[]')\n",
            if success { "True" } else { "False" })).unwrap();
        let path = dir.to_str().unwrap();
        let mut shop = ExtensionShop::with_config_dir(path, path);
        shop.plugins.push(PluginInfo { full_name: "fixture/plugin".into(), installed, supported: true, ..Default::default() });
        shop.recipe_form = Some(form());
        (shop, TestDir(dir))
    }

    #[test]
    fn preconfiguration_does_not_request_restart_but_installed_configuration_does() {
        use crate::tui::widgets::extension_shop::ShopConfirm;
        for installed in [false, true] {
            let (mut shop, _dir) = shop_fixture(installed, true);
            shop.save_recipe_form();
            assert!(shop.recipe_form.is_none());
            assert_eq!(shop.needs_restart, installed);
            assert_eq!(shop.confirm_modal, if installed { ShopConfirm::Restart } else { ShopConfirm::None });
            assert!(!shop.restart_requested, "saving must never restart without confirmation");
        }
    }

    #[test]
    fn failed_save_keeps_draft_and_reports_error() {
        let (mut shop, _dir) = shop_fixture(true, false);
        shop.recipe_form.as_mut().unwrap().adjust(0, 1);
        shop.save_recipe_form();
        let draft = shop.recipe_form.as_ref().unwrap();
        assert!(draft.dirty);
        assert_eq!(draft.values["enabled"], false);
        assert_eq!(draft.error, "fixture result");
        assert!(!shop.needs_restart);
    }

    #[test]
    fn bundled_catalog_defaults_are_valid_and_identities_unique() {
        let c = catalog().unwrap();
        assert!(!c.recipes.is_empty());
        let mut repos = std::collections::HashSet::new();
        for recipe in c.recipes {
            assert!(repos.insert(recipe.repo.clone()), "duplicate recipe");
            assert!(RecipeForm::new(recipe, Map::new()).validate().is_ok());
        }
    }
}
