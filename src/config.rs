//! Oxvelte configuration file support.
//!
//! Config file: `oxvelte.config.json` (project root)
//!
//! ```json
//! {
//!   "rules": {
//!     "svelte/no-at-html-tags": "error",
//!     "svelte/button-has-type": ["warn", { "button": false }],
//!     "svelte/no-inline-styles": "off"
//!   },
//!   "settings": {
//!     "svelte": {
//!       "kit": {
//!         "files": { "routes": "src/routes" }
//!       }
//!     }
//!   }
//! }
//! ```
//!
//! Rule severity: "off" | "warn" | "error" (or 0 | 1 | 2)
//! Rule options: ["error", { ...options }]

use crate::linter::RuleConfig;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Parsed oxvelte configuration.
#[derive(Debug, Clone, Default)]
pub struct OxvelteConfig {
    /// Rule name → (enabled, severity, options)
    pub rules: HashMap<String, RuleEntry>,
    /// Global settings (e.g. svelte.kit.files.routes)
    pub settings: Option<Value>,
    /// Glob patterns for custom JS rule files (e.g. `["./rules/*.js"]`)
    pub custom_rules: Vec<String>,
}

/// A single rule's configuration.
#[derive(Debug, Clone)]
pub struct RuleEntry {
    /// "off", "warn", or "error"
    pub severity: String,
    /// Rule-specific options (if any)
    pub options: Option<Value>,
}

impl RuleEntry {
    pub fn is_off(&self) -> bool {
        self.severity == "off" || self.severity == "0"
    }
}

impl OxvelteConfig {
    /// Load config from the nearest `oxvelte.config.json` walking up from `start_dir`.
    pub fn load(start_dir: &Path) -> Self {
        let mut dir = start_dir;
        loop {
            let config_path = dir.join("oxvelte.config.json");
            if config_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&config_path) {
                    if let Ok(config) = Self::parse(&content) {
                        return config;
                    }
                }
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
        Self::default()
    }

    /// Parse config from a JSON string.
    pub fn parse(json_str: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(json_str)
            .map_err(|e| format!("Invalid JSON: {}", e))?;

        let mut config = Self::default();

        // Parse rules
        if let Some(rules_obj) = value.get("rules").and_then(|v| v.as_object()) {
            for (name, val) in rules_obj {
                let entry = parse_rule_entry(val);
                config.rules.insert(name.clone(), entry);
            }
        }

        // Parse settings
        config.settings = value.get("settings").cloned();

        // Parse customRules
        if let Some(arr) = value.get("customRules").and_then(|v| v.as_array()) {
            config.custom_rules = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }

        Ok(config)
    }

    /// Get the RuleConfig for a specific rule name.
    pub fn rule_config(&self, rule_name: &str) -> RuleConfig {
        if let Some(entry) = self.rules.get(rule_name) {
            RuleConfig {
                options: entry.options.as_ref().map(|o| Value::Array(vec![o.clone()])),
                settings: self.settings.clone(),
            }
        } else {
            RuleConfig {
                options: None,
                settings: self.settings.clone(),
            }
        }
    }

    /// Check if a rule is explicitly disabled.
    pub fn is_rule_off(&self, rule_name: &str) -> bool {
        self.rules.get(rule_name).is_some_and(|e| e.is_off())
    }

    /// Convert from an ESLint-style config (flat or legacy).
    pub fn from_eslint(json_str: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(json_str)
            .map_err(|e| format!("Invalid JSON: {}", e))?;

        let mut config = Self::default();

        // Handle flat config array: [{ rules: {...} }, ...]
        if let Some(arr) = value.as_array() {
            for item in arr {
                merge_eslint_object(item, &mut config);
            }
        } else {
            // Legacy .eslintrc object
            merge_eslint_object(&value, &mut config);
        }

        Ok(config)
    }

    /// Serialize to oxvelte.config.json format.
    pub fn to_json(&self) -> String {
        let mut obj = serde_json::Map::new();

        if !self.rules.is_empty() {
            let mut rules = serde_json::Map::new();
            let mut sorted_rules: Vec<_> = self.rules.iter().collect();
            sorted_rules.sort_by_key(|(k, _)| k.clone());
            for (name, entry) in sorted_rules {
                let val = if let Some(opts) = &entry.options {
                    Value::Array(vec![Value::String(entry.severity.clone()), opts.clone()])
                } else {
                    Value::String(entry.severity.clone())
                };
                rules.insert(name.clone(), val);
            }
            obj.insert("rules".to_string(), Value::Object(rules));
        }

        if let Some(settings) = &self.settings {
            obj.insert("settings".to_string(), settings.clone());
        }

        serde_json::to_string_pretty(&Value::Object(obj)).unwrap_or_default()
    }
}

fn parse_rule_entry(val: &Value) -> RuleEntry {
    match val {
        // "off" | "warn" | "error"
        Value::String(s) => RuleEntry {
            severity: normalize_severity(s),
            options: None,
        },
        // 0 | 1 | 2
        Value::Number(n) => RuleEntry {
            severity: match n.as_u64() {
                Some(0) => "off".to_string(),
                Some(1) => "warn".to_string(),
                _ => "error".to_string(),
            },
            options: None,
        },
        // ["error", { ...options }]
        Value::Array(arr) => {
            let severity = arr.first()
                .map(|v| match v {
                    Value::String(s) => normalize_severity(s),
                    Value::Number(n) => match n.as_u64() {
                        Some(0) => "off".to_string(),
                        Some(1) => "warn".to_string(),
                        _ => "error".to_string(),
                    },
                    _ => "error".to_string(),
                })
                .unwrap_or_else(|| "error".to_string());
            let options = arr.get(1).cloned();
            RuleEntry { severity, options }
        }
        _ => RuleEntry { severity: "error".to_string(), options: None },
    }
}

fn normalize_severity(s: &str) -> String {
    match s {
        "0" | "off" => "off".to_string(),
        "1" | "warn" => "warn".to_string(),
        _ => "error".to_string(),
    }
}

fn merge_eslint_object(obj: &Value, config: &mut OxvelteConfig) {
    // Extract rules — only keep svelte/* rules
    if let Some(rules) = obj.get("rules").and_then(|v| v.as_object()) {
        for (name, val) in rules {
            if name.starts_with("svelte/") {
                config.rules.insert(name.clone(), parse_rule_entry(val));
            }
        }
    }

    // Extract settings
    if let Some(settings) = obj.get("settings") {
        config.settings = Some(settings.clone());
    }

    // Handle overrides array (legacy ESLint)
    if let Some(overrides) = obj.get("overrides").and_then(|v| v.as_array()) {
        for override_obj in overrides {
            // Check if this override applies to *.svelte files
            let applies_to_svelte = override_obj.get("files")
                .and_then(|f| f.as_array())
                .is_some_and(|files| files.iter().any(|f| {
                    f.as_str().is_some_and(|s| s.contains(".svelte"))
                }));
            if applies_to_svelte {
                merge_eslint_object(override_obj, config);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let json = r#"{ "rules": { "svelte/no-at-html-tags": "error" } }"#;
        let config = OxvelteConfig::parse(json).unwrap();
        assert_eq!(config.rules["svelte/no-at-html-tags"].severity, "error");
        assert!(!config.is_rule_off("svelte/no-at-html-tags"));
    }

    #[test]
    fn test_parse_with_options() {
        let json = r#"{ "rules": { "svelte/button-has-type": ["warn", { "button": false }] } }"#;
        let config = OxvelteConfig::parse(json).unwrap();
        let entry = &config.rules["svelte/button-has-type"];
        assert_eq!(entry.severity, "warn");
        assert!(entry.options.is_some());
    }

    #[test]
    fn test_parse_off() {
        let json = r#"{ "rules": { "svelte/no-inline-styles": "off" } }"#;
        let config = OxvelteConfig::parse(json).unwrap();
        assert!(config.is_rule_off("svelte/no-inline-styles"));
    }

    #[test]
    fn test_from_eslint_legacy() {
        let json = r#"{
            "plugins": ["svelte"],
            "rules": {
                "svelte/no-at-html-tags": "warn",
                "no-console": "error",
                "svelte/button-has-type": ["error", { "button": false }]
            },
            "settings": {
                "svelte": { "kit": { "files": { "routes": "src/routes" } } }
            }
        }"#;
        let config = OxvelteConfig::from_eslint(json).unwrap();
        // Only svelte rules kept
        assert_eq!(config.rules.len(), 2);
        assert!(config.rules.contains_key("svelte/no-at-html-tags"));
        assert!(config.rules.contains_key("svelte/button-has-type"));
        assert!(!config.rules.contains_key("no-console"));
        // Settings preserved
        assert!(config.settings.is_some());
    }

    #[test]
    fn test_roundtrip() {
        let json = r#"{ "rules": { "svelte/no-at-html-tags": "error", "svelte/button-has-type": ["warn", { "button": false }] } }"#;
        let config = OxvelteConfig::parse(json).unwrap();
        let output = config.to_json();
        let reparsed = OxvelteConfig::parse(&output).unwrap();
        assert_eq!(reparsed.rules.len(), 2);
    }
}
