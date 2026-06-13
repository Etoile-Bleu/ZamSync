use crate::{ZamError, ZamResult};

/// Controls which payloads `ZamEngine` accepts at submit and replicate time.
///
/// `None` is the default and accepts any bytes. Use `Json` or `JsonRequired`
/// for deployments (like Bhutan ePIS) where all events must carry structured data.
#[derive(Debug, Clone, Default)]
pub enum PayloadSchema {
    /// Accept any bytes (default -- backward compatible).
    #[default]
    None,
    /// Payload must be valid JSON.
    Json,
    /// Payload must be valid JSON **and** contain all listed top-level keys.
    JsonRequired(Vec<String>),
}

impl PayloadSchema {
    /// Parse a schema from a CLI flag value (`"none"`, `"json"`, `"json+key1,key2"`).
    pub fn from_str(s: &str) -> Result<Self, String> {
        if s == "none" {
            return Ok(Self::None);
        }
        if s == "json" {
            return Ok(Self::Json);
        }
        if let Some(rest) = s.strip_prefix("json+") {
            let fields = rest.split(',').map(str::to_owned).collect();
            return Ok(Self::JsonRequired(fields));
        }
        Err(format!(
            "unknown schema '{s}': use 'none', 'json', or 'json+field1,field2'"
        ))
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn validate(&self, payload: &[u8]) -> ZamResult<()> {
        match self {
            Self::None => Ok(()),
            Self::Json => json_parse(payload).map(|_| ()),
            Self::JsonRequired(fields) => {
                let v = json_parse(payload)?;
                for field in fields {
                    if v.get(field.as_str()).is_none() {
                        return Err(ZamError::Validation(format!(
                            "missing required field '{field}'"
                        )));
                    }
                }
                Ok(())
            }
        }
    }
}

fn json_parse(payload: &[u8]) -> ZamResult<serde_json::Value> {
    serde_json::from_slice(payload)
        .map_err(|e| ZamError::Validation(format!("invalid JSON: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_accepts_anything() {
        assert!(PayloadSchema::None.validate(b"not json at all").is_ok());
        assert!(PayloadSchema::None.validate(b"").is_ok());
    }

    #[test]
    fn test_json_accepts_valid() {
        assert!(PayloadSchema::Json.validate(br#"{"type":"patient_admitted"}"#).is_ok());
        assert!(PayloadSchema::Json.validate(b"42").is_ok());
        assert!(PayloadSchema::Json.validate(b"null").is_ok());
    }

    #[test]
    fn test_json_rejects_invalid() {
        let err = PayloadSchema::Json.validate(b"not json").unwrap_err();
        assert!(matches!(err, ZamError::Validation(_)));
    }

    #[test]
    fn test_json_required_accepts_all_fields() {
        let schema = PayloadSchema::JsonRequired(vec!["type".into(), "patient_id".into()]);
        let payload = br#"{"type":"discharge","patient_id":"BT-001","ward":"3A"}"#;
        assert!(schema.validate(payload).is_ok());
    }

    #[test]
    fn test_json_required_rejects_missing_field() {
        let schema = PayloadSchema::JsonRequired(vec!["type".into(), "patient_id".into()]);
        let payload = br#"{"type":"discharge"}"#;
        let err = schema.validate(payload).unwrap_err();
        assert!(matches!(&err, ZamError::Validation(msg) if msg.contains("patient_id")));
    }

    #[test]
    fn test_from_str_round_trip() {
        assert!(matches!(PayloadSchema::from_str("none").unwrap(), PayloadSchema::None));
        assert!(matches!(PayloadSchema::from_str("json").unwrap(), PayloadSchema::Json));
        let PayloadSchema::JsonRequired(fields) = PayloadSchema::from_str("json+type,patient_id").unwrap() else { panic!() };
        assert_eq!(fields, ["type", "patient_id"]);
        assert!(PayloadSchema::from_str("bad").is_err());
    }
}
