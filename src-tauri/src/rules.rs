use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    Notify,
    Block,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockRule {
    pub from_app_id: Option<String>,
    pub from_app_name: Option<String>,
    pub to_app_id: Option<String>,
    pub to_app_name: Option<String>,
    pub action: RuleAction,
}

fn rules_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("rules.json"))
}

pub fn default_rules() -> Vec<BlockRule> {
    let terminals = [
        ("com.apple.Terminal", "Terminal"),
        ("com.googlecode.iterm2", "iTerm2"),
        ("io.alacritty", "Alacritty"),
        ("com.github.wez.wezterm", "WezTerm"),
        ("net.kovidgoyal.kitty", "kitty"),
        ("co.zeit.hyper", "Hyper"),
        ("com.mitchellh.ghostty", "Ghostty"),
        ("com.raphaelamorim.rio", "Rio"),
    ];
    terminals
        .iter()
        .map(|(id, name)| BlockRule {
            from_app_id: None,
            from_app_name: None,
            to_app_id: Some(id.to_string()),
            to_app_name: Some(name.to_string()),
            action: RuleAction::Notify,
        })
        .collect()
}

pub fn load(app: &tauri::AppHandle) -> Vec<BlockRule> {
    let Some(path) = rules_path(app) else {
        return default_rules();
    };
    match fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_else(|_| default_rules()),
        Err(_) => default_rules(),
    }
}

pub fn save(app: &tauri::AppHandle, rules: &[BlockRule]) -> Result<(), String> {
    let Some(path) = rules_path(app) else {
        return Err("no app data dir".into());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(rules).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

/// Check if a rule is valid: at least one side must specify an app
#[allow(dead_code)]
pub fn is_valid(rule: &BlockRule) -> bool {
    rule.from_app_id.is_some() || rule.to_app_id.is_some()
}

/// Find first matching rule for a source→dest pair
pub fn matches_rule(
    rules: &[BlockRule],
    source_app_id: Option<&str>,
    dest_app_id: &str,
) -> Option<BlockRule> {
    rules
        .iter()
        .find(|r| {
            let from_matches = match &r.from_app_id {
                None => true,
                Some(id) => source_app_id
                    .map(|s| s.eq_ignore_ascii_case(id))
                    .unwrap_or(false),
            };
            let to_matches = match &r.to_app_id {
                None => true,
                Some(id) => id.eq_ignore_ascii_case(dest_app_id),
            };
            from_matches && to_matches
        })
        .cloned()
}
