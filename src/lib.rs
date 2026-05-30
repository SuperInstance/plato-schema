use serde::{Deserialize, Serialize};
use std::fmt;

// ── SchemaVersion ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SchemaVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl SchemaVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if !s.starts_with('v') && !s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            return Err(format!("invalid version string: {s}"));
        }
        let digits = s.trim_start_matches('v');
        let parts: Vec<&str> = digits.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("expected major.minor.patch, got: {s}"));
        }
        let major = parts[0].parse::<u32>().map_err(|e| format!("invalid major: {e}"))?;
        let minor = parts[1].parse::<u32>().map_err(|e| format!("invalid minor: {e}"))?;
        let patch = parts[2].parse::<u32>().map_err(|e| format!("invalid patch: {e}"))?;
        Ok(Self { major, minor, patch })
    }

    pub fn is_compatible(&self, other: &Self) -> bool {
        self.major == other.major
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Ord for SchemaVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }
}

impl PartialOrd for SchemaVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ── FieldType ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Array,
    Object,
    Enum(Vec<String>),
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldType::String => write!(f, "string"),
            FieldType::Integer => write!(f, "integer"),
            FieldType::Float => write!(f, "float"),
            FieldType::Boolean => write!(f, "boolean"),
            FieldType::Array => write!(f, "array"),
            FieldType::Object => write!(f, "object"),
            FieldType::Enum(vals) => write!(f, "enum({})", vals.join("|")),
        }
    }
}

// ── SchemaField ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaField {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    pub description: String,
}

impl SchemaField {
    pub fn new(name: &str, field_type: FieldType, required: bool, description: &str) -> Self {
        Self {
            name: name.to_string(),
            field_type,
            required,
            description: description.to_string(),
        }
    }
}

// ── ValidationResult ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

impl ValidationResult {
    pub fn ok() -> Self {
        Self { valid: true, errors: vec![] }
    }

    pub fn fail(errors: Vec<String>) -> Self {
        Self { valid: false, errors }
    }
}

// ── Migration ────────────────────────────────────────────────────────────────

pub type TransformFn = fn(&str) -> String;

#[derive(Debug, Clone)]
pub struct Migration {
    pub from_version: SchemaVersion,
    pub to_version: SchemaVersion,
    pub transform: TransformFn,
}

impl Migration {
    pub fn new(from: SchemaVersion, to: SchemaVersion, transform: TransformFn) -> Self {
        Self { from_version: from, to_version: to, transform }
    }
}

// ── MessageSchema ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSchema {
    pub version: SchemaVersion,
    pub name: String,
    pub fields: Vec<SchemaField>,
    #[serde(skip)]
    #[serde(default = "Vec::new")]
    pub migrations: Vec<MigrationPlaceholder>,
}

// MigrationPlaceholder so we can Serialize; real Migration carries a fn pointer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MigrationPlaceholder {
    pub from: String,
    pub to: String,
}

impl MessageSchema {
    pub fn new(name: &str, version: SchemaVersion, fields: Vec<SchemaField>) -> Self {
        Self {
            version,
            name: name.to_string(),
            fields,
            migrations: vec![],
        }
    }

    pub fn validate(&self, json: &str) -> ValidationResult {
        let mut errors: Vec<String> = Vec::new();
        let value: serde_json::Value = match serde_json::from_str(json) {
            Ok(v) => v,
            Err(e) => {
                return ValidationResult::fail(vec![format!("invalid JSON: {e}")]);
            }
        };

        let obj = match value.as_object() {
            Some(o) => o,
            None => {
                return ValidationResult::fail(vec!["expected a JSON object".to_string()]);
            }
        };

        for field in &self.fields {
            match obj.get(&field.name) {
                None => {
                    if field.required {
                        errors.push(format!("missing required field: {}", field.name));
                    }
                }
                Some(val) => {
                    if let Err(e) = validate_type(&field.name, val, &field.field_type) {
                        errors.push(e);
                    }
                }
            }
        }

        if errors.is_empty() {
            ValidationResult::ok()
        } else {
            ValidationResult::fail(errors)
        }
    }

    pub fn migrate(
        &self,
        json: &str,
        target: &SchemaVersion,
        migrations: &[Migration],
    ) -> Result<String, String> {
        if &self.version == target {
            return Ok(json.to_string());
        }

        let mut current = self.version.clone();
        let mut data = json.to_string();

        while current != *target {
            let migration = migrations.iter().find(|m| m.from_version == current)
                .ok_or_else(|| format!("no migration path from {} to {}", current, target))?;

            data = (migration.transform)(&data);
            current = migration.to_version.clone();
        }

        Ok(data)
    }
}

// ── Type validation helpers ──────────────────────────────────────────────────

fn validate_type(name: &str, value: &serde_json::Value, ft: &FieldType) -> Result<(), String> {
    match ft {
        FieldType::String => {
            if !value.is_string() {
                return Err(format!("field '{name}': expected string, got {}", json_type(value)));
            }
        }
        FieldType::Integer => {
            if !value.is_i64() && !value.is_u64() {
                return Err(format!("field '{name}': expected integer, got {}", json_type(value)));
            }
        }
        FieldType::Float => {
            if !value.is_f64() && !value.is_i64() && !value.is_u64() {
                return Err(format!("field '{name}': expected float, got {}", json_type(value)));
            }
        }
        FieldType::Boolean => {
            if !value.is_boolean() {
                return Err(format!("field '{name}': expected boolean, got {}", json_type(value)));
            }
        }
        FieldType::Array => {
            if !value.is_array() {
                return Err(format!("field '{name}': expected array, got {}", json_type(value)));
            }
        }
        FieldType::Object => {
            if !value.is_object() {
                return Err(format!("field '{name}': expected object, got {}", json_type(value)));
            }
        }
        FieldType::Enum(variants) => {
            if let Some(s) = value.as_str() {
                if !variants.contains(&s.to_string()) {
                    return Err(format!(
                        "field '{name}': value '{}' not in enum({})",
                        s,
                        variants.join("|")
                    ));
                }
            } else {
                return Err(format!("field '{name}': expected string for enum, got {}", json_type(value)));
            }
        }
    }
    Ok(())
}

fn json_type(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

// ── Built-in schemas ─────────────────────────────────────────────────────────

pub fn tile_schema() -> MessageSchema {
    MessageSchema::new(
        "tile",
        SchemaVersion::new(1, 0, 0),
        vec![
            SchemaField::new("id", FieldType::String, true, "Unique tile identifier"),
            SchemaField::new("label", FieldType::String, true, "Display label"),
            SchemaField::new("position", FieldType::Object, true, "Tile position {x, y}"),
            SchemaField::new("state", FieldType::Enum(vec!["active".into(), "inactive".into(), "pending".into()]), true, "Tile state"),
            SchemaField::new("priority", FieldType::Integer, false, "Priority level"),
        ],
    )
}

pub fn alert_schema() -> MessageSchema {
    MessageSchema::new(
        "alert",
        SchemaVersion::new(1, 0, 0),
        vec![
            SchemaField::new("id", FieldType::String, true, "Alert ID"),
            SchemaField::new("severity", FieldType::Enum(vec!["info".into(), "warning".into(), "error".into(), "critical".into()]), true, "Alert severity"),
            SchemaField::new("message", FieldType::String, true, "Alert message text"),
            SchemaField::new("timestamp", FieldType::Float, true, "Unix timestamp"),
            SchemaField::new("resolved", FieldType::Boolean, false, "Whether the alert is resolved"),
        ],
    )
}

pub fn state_schema() -> MessageSchema {
    MessageSchema::new(
        "state",
        SchemaVersion::new(1, 0, 0),
        vec![
            SchemaField::new("component", FieldType::String, true, "Component name"),
            SchemaField::new("status", FieldType::Enum(vec!["running".into(), "stopped".into(), "error".into()]), true, "Component status"),
            SchemaField::new("uptime", FieldType::Float, false, "Seconds since start"),
            SchemaField::new("metadata", FieldType::Object, false, "Extra metadata"),
        ],
    )
}

pub fn event_schema() -> MessageSchema {
    MessageSchema::new(
        "event",
        SchemaVersion::new(1, 0, 0),
        vec![
            SchemaField::new("event_type", FieldType::String, true, "Event type name"),
            SchemaField::new("source", FieldType::String, true, "Event source"),
            SchemaField::new("data", FieldType::Object, true, "Event payload"),
            SchemaField::new("tags", FieldType::Array, false, "Event tags"),
        ],
    )
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Schema version parsing
    #[test]
    fn parse_version_standard() {
        let v = SchemaVersion::parse("1.2.3").unwrap();
        assert_eq!(v, SchemaVersion::new(1, 2, 3));
    }

    // 2. Schema version parsing with 'v' prefix
    #[test]
    fn parse_version_v_prefix() {
        let v = SchemaVersion::parse("v2.0.1").unwrap();
        assert_eq!(v, SchemaVersion::new(2, 0, 1));
    }

    // 3. Schema version parse failure
    #[test]
    fn parse_version_invalid() {
        assert!(SchemaVersion::parse("abc").is_err());
        assert!(SchemaVersion::parse("1.2").is_err());
        assert!(SchemaVersion::parse("1.2.3.4").is_err());
    }

    // 4. Version comparison/ordering
    #[test]
    fn version_ordering() {
        let v1 = SchemaVersion::new(1, 0, 0);
        let v2 = SchemaVersion::new(1, 1, 0);
        let v3 = SchemaVersion::new(2, 0, 0);
        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v1 < v3);
    }

    // 5. Version compatibility (same major)
    #[test]
    fn version_compatible_same_major() {
        let v1 = SchemaVersion::new(1, 0, 0);
        let v2 = SchemaVersion::new(1, 5, 3);
        assert!(v1.is_compatible(&v2));
    }

    // 6. Version incompatibility (different major)
    #[test]
    fn version_incompatible_different_major() {
        let v1 = SchemaVersion::new(1, 0, 0);
        let v2 = SchemaVersion::new(2, 0, 0);
        assert!(!v1.is_compatible(&v2));
    }

    // 7. Required field missing
    #[test]
    fn validate_missing_required_field() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("name", FieldType::String, true, "Name")],
        );
        let result = schema.validate("{}");
        assert!(!result.valid);
        assert!(result.errors[0].contains("missing required field: name"));
    }

    // 8. Wrong type
    #[test]
    fn validate_wrong_type() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("age", FieldType::Integer, true, "Age")],
        );
        let result = schema.validate(r#"{"age": "not a number"}"#);
        assert!(!result.valid);
        assert!(result.errors[0].contains("expected integer"));
    }

    // 9. Valid input passes
    #[test]
    fn validate_valid_input() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![
                SchemaField::new("name", FieldType::String, true, "Name"),
                SchemaField::new("count", FieldType::Integer, false, "Count"),
            ],
        );
        let result = schema.validate(r#"{"name": "hello", "count": 42}"#);
        assert!(result.valid);
    }

    // 10. Extra fields are allowed
    #[test]
    fn validate_extra_fields_allowed() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("name", FieldType::String, true, "Name")],
        );
        let result = schema.validate(r#"{"name": "hi", "extra": "ignored"}"#);
        assert!(result.valid);
    }

    // 11. Enum constraint valid
    #[test]
    fn validate_enum_valid() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("status", FieldType::Enum(vec!["on".into(), "off".into()]), true, "Status")],
        );
        let result = schema.validate(r#"{"status": "on"}"#);
        assert!(result.valid);
    }

    // 12. Enum constraint invalid
    #[test]
    fn validate_enum_invalid() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("status", FieldType::Enum(vec!["on".into(), "off".into()]), true, "Status")],
        );
        let result = schema.validate(r#"{"status": "maybe"}"#);
        assert!(!result.valid);
        assert!(result.errors[0].contains("not in enum"));
    }

    // 13. Nested object validation (object type)
    #[test]
    fn validate_nested_object() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("config", FieldType::Object, true, "Config")],
        );
        let result = schema.validate(r#"{"config": {"nested": true}}"#);
        assert!(result.valid);
    }

    // 14. Array type validation
    #[test]
    fn validate_array_type() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("items", FieldType::Array, true, "Items")],
        );
        let result = schema.validate(r#"{"items": [1, 2, 3]}"#);
        assert!(result.valid);
    }

    // 15. Empty JSON string
    #[test]
    fn validate_empty_json() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("name", FieldType::String, true, "Name")],
        );
        let result = schema.validate("");
        assert!(!result.valid);
        assert!(result.errors[0].contains("invalid JSON"));
    }

    // 16. Non-object JSON (array)
    #[test]
    fn validate_non_object_json() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("name", FieldType::String, true, "Name")],
        );
        let result = schema.validate(r#"[1,2,3]"#);
        assert!(!result.valid);
        assert!(result.errors[0].contains("expected a JSON object"));
    }

    // 17. Float accepts integer
    #[test]
    fn validate_float_accepts_integer() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("val", FieldType::Float, true, "Val")],
        );
        let result = schema.validate(r#"{"val": 42}"#);
        assert!(result.valid);
    }

    // 18. Migration
    #[test]
    fn migrate_simple() {
        let v1 = SchemaVersion::new(1, 0, 0);
        let v2 = SchemaVersion::new(1, 1, 0);
        let migrations = vec![Migration::new(
            v1.clone(),
            v2.clone(),
            |json| {
                let mut val: serde_json::Value = serde_json::from_str(json).unwrap();
                if let Some(obj) = val.as_object_mut() {
                    obj.insert("new_field".to_string(), serde_json::Value::String("default".to_string()));
                }
                serde_json::to_string(&val).unwrap()
            },
        )];
        let schema = MessageSchema::new("test", v1, vec![]);
        let result = schema.migrate(r#"{"old_field": 1}"#, &v2, &migrations).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["new_field"], "default");
        assert_eq!(parsed["old_field"], 1);
    }

    // 19. Migration no-op when same version
    #[test]
    fn migrate_same_version() {
        let v = SchemaVersion::new(1, 0, 0);
        let schema = MessageSchema::new("test", v.clone(), vec![]);
        let result = schema.migrate(r#"{"x":1}"#, &v, &[]).unwrap();
        assert_eq!(result, r#"{"x":1}"#);
    }

    // 20. Migration failure when no path
    #[test]
    fn migrate_no_path() {
        let v1 = SchemaVersion::new(1, 0, 0);
        let v3 = SchemaVersion::new(3, 0, 0);
        let schema = MessageSchema::new("test", v1, vec![]);
        let result = schema.migrate(r#"{"x":1}"#, &v3, &[]);
        assert!(result.is_err());
    }

    // 21. Built-in tile schema validates example
    #[test]
    fn tile_schema_validates_example() {
        let schema = tile_schema();
        let result = schema.validate(r#"{
            "id": "tile-1",
            "label": "My Tile",
            "position": {"x": 0, "y": 0},
            "state": "active",
            "priority": 5
        }"#);
        assert!(result.valid);
    }

    // 22. Built-in alert schema validates example
    #[test]
    fn alert_schema_validates_example() {
        let schema = alert_schema();
        let result = schema.validate(r#"{
            "id": "alert-1",
            "severity": "warning",
            "message": "Something happened",
            "timestamp": 1700000000.0,
            "resolved": false
        }"#);
        assert!(result.valid);
    }

    // 23. Built-in state schema validates example
    #[test]
    fn state_schema_validates_example() {
        let schema = state_schema();
        let result = schema.validate(r#"{
            "component": "engine",
            "status": "running",
            "uptime": 123.45
        }"#);
        assert!(result.valid);
    }

    // 24. Built-in event schema validates example
    #[test]
    fn event_schema_validates_example() {
        let schema = event_schema();
        let result = schema.validate(r#"{
            "event_type": "click",
            "source": "ui",
            "data": {"target": "button-1"},
            "tags": ["user", "interaction"]
        }"#);
        assert!(result.valid);
    }

    // 25. Boolean type validation
    #[test]
    fn validate_boolean_wrong_type() {
        let schema = MessageSchema::new(
            "test",
            SchemaVersion::new(1, 0, 0),
            vec![SchemaField::new("flag", FieldType::Boolean, true, "Flag")],
        );
        let result = schema.validate(r#"{"flag": "true"}"#);
        assert!(!result.valid);
        assert!(result.errors[0].contains("expected boolean"));
    }

    // 26. Version display
    #[test]
    fn version_display() {
        let v = SchemaVersion::new(1, 2, 3);
        assert_eq!(format!("{v}"), "1.2.3");
    }

    // 27. FieldType display
    #[test]
    fn field_type_display() {
        assert_eq!(format!("{}", FieldType::String), "string");
        assert_eq!(format!("{}", FieldType::Enum(vec!["a".into(), "b".into()])), "enum(a|b)");
    }
}
