//! Parse a backend's text into the output contract (ADR-007).
//!
//! Tolerant of fenced code blocks and prose wrapping. Validates types against
//! the schema; extra keys are ignored. Failures are [`ParseError`] (mapped to
//! `ErrorKind::BadOutput` by the backend layer).

use serde_json::Value;

use crate::types::Candidate;

/// Structured output extracted from a model response.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedOutput {
    /// At least one candidate.
    pub candidates: Vec<Candidate>,
    /// Advisory notes from the model (classifier is the authority on risk).
    pub risk_notes: Vec<String>,
    /// Model-stated assumptions / uncertainty.
    pub assumptions: Vec<String>,
}

/// Why parsing rejected a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// No JSON object could be found.
    NoJson,
    /// An object was started but never closed (truncated output).
    Truncated,
    /// JSON parsed but a field had the wrong type or a required field was missing.
    Schema,
}

/// Failed parse of a backend response.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind:?}: {detail}")]
pub struct ParseError {
    /// Class of failure.
    pub kind: ParseErrorKind,
    /// Human-readable reason.
    pub detail: String,
}

impl ParseError {
    fn no_json(detail: impl Into<String>) -> Self {
        Self {
            kind: ParseErrorKind::NoJson,
            detail: detail.into(),
        }
    }

    fn truncated(detail: impl Into<String>) -> Self {
        Self {
            kind: ParseErrorKind::Truncated,
            detail: detail.into(),
        }
    }

    fn schema(detail: impl Into<String>) -> Self {
        Self {
            kind: ParseErrorKind::Schema,
            detail: detail.into(),
        }
    }
}

/// Strip fences, extract the first balanced JSON object, validate the schema.
pub fn parse_response(raw: &str) -> Result<ParsedOutput, ParseError> {
    let json_text = extract_json(raw)?;
    let value: Value = serde_json::from_str(json_text).map_err(|e| {
        if e.is_eof() {
            ParseError::truncated(e.to_string())
        } else {
            ParseError::schema(e.to_string())
        }
    })?;
    validate_output(&value)
}

fn extract_json(raw: &str) -> Result<&str, ParseError> {
    let stripped = strip_fence(raw);
    first_balanced_object(stripped)
}

/// Remove a wrapping markdown fence if present (` ```json ` … ` ``` `).
fn strip_fence(raw: &str) -> &str {
    let trimmed = raw.trim();
    let rest = match trimmed.strip_prefix("```") {
        Some(r) => r,
        None => return trimmed,
    };
    // Optional language tag on the opening fence.
    let after_tag = match rest.find('\n') {
        Some(i) => &rest[i + 1..],
        None => return trimmed,
    };
    match after_tag.rfind("```") {
        Some(i) => after_tag[..i].trim(),
        None => after_tag.trim(),
    }
}

fn first_balanced_object(s: &str) -> Result<&str, ParseError> {
    let bytes = s.as_bytes();
    let start = s
        .find('{')
        .ok_or_else(|| ParseError::no_json("no JSON object found"))?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (offset, &b) in bytes[start..].iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            match b {
                b'\\' => escape = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let end = start + offset + 1;
                    return Ok(&s[start..end]);
                }
            }
            _ => {}
        }
    }
    if depth > 0 {
        Err(ParseError::truncated("unclosed JSON object"))
    } else {
        Err(ParseError::no_json("no JSON object found"))
    }
}

fn validate_output(value: &Value) -> Result<ParsedOutput, ParseError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ParseError::schema("root must be a JSON object"))?;

    let commands = obj
        .get("commands")
        .ok_or_else(|| ParseError::schema("missing field `commands`"))?;
    let arr = commands
        .as_array()
        .ok_or_else(|| ParseError::schema("`commands` must be an array"))?;
    if arr.is_empty() {
        return Err(ParseError::schema(
            "`commands` must contain at least one entry",
        ));
    }

    let mut candidates = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let c = item
            .as_object()
            .ok_or_else(|| ParseError::schema(format!("commands[{i}] must be an object")))?;
        let command = match c.get("command") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(Value::String(_)) => {
                return Err(ParseError::schema(format!(
                    "commands[{i}].command is empty"
                )));
            }
            Some(_) => {
                return Err(ParseError::schema(format!(
                    "commands[{i}].command must be a string"
                )));
            }
            None => {
                return Err(ParseError::schema(format!(
                    "commands[{i}] missing field `command`"
                )));
            }
        };
        let explanation = match c.get("explanation") {
            None => String::new(),
            Some(Value::String(s)) => s.clone(),
            Some(_) => {
                return Err(ParseError::schema(format!(
                    "commands[{i}].explanation must be a string"
                )));
            }
        };
        let confidence = match c.get("confidence") {
            None => 0.0,
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) as f32,
            Some(_) => {
                return Err(ParseError::schema(format!(
                    "commands[{i}].confidence must be a number"
                )));
            }
        };
        candidates.push(Candidate {
            command,
            explanation,
            confidence,
        });
    }

    let risk_notes = string_array(obj.get("risk_notes"), "risk_notes")?;
    let assumptions = string_array(obj.get("assumptions"), "assumptions")?;

    Ok(ParsedOutput {
        candidates,
        risk_notes,
        assumptions,
    })
}

fn string_array(value: Option<&Value>, field: &str) -> Result<Vec<String>, ParseError> {
    match value {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::String(s) => out.push(s.clone()),
                    _ => {
                        return Err(ParseError::schema(format!("{field}[{i}] must be a string")));
                    }
                }
            }
            Ok(out)
        }
        Some(_) => Err(ParseError::schema(format!("`{field}` must be an array"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_json() -> &'static str {
        r#"{
            "commands": [
                {
                    "command": "docker run --name pg -p 7000:5432 -d postgres:16",
                    "explanation": "start postgres",
                    "confidence": 0.92
                }
            ],
            "risk_notes": ["password is inline"],
            "assumptions": ["host port 7000 is free"]
        }"#
    }

    #[test]
    fn valid_json() {
        let parsed = parse_response(ok_json()).expect("valid");
        assert_eq!(parsed.candidates.len(), 1);
        assert_eq!(
            parsed.candidates[0].command,
            "docker run --name pg -p 7000:5432 -d postgres:16"
        );
        assert!((parsed.candidates[0].confidence - 0.92).abs() < f32::EPSILON);
        assert_eq!(parsed.risk_notes.len(), 1);
        assert_eq!(parsed.assumptions.len(), 1);
    }

    #[test]
    fn fenced_json() {
        let raw = format!("```json\n{}\n```", ok_json());
        let parsed = parse_response(&raw).expect("fenced");
        assert_eq!(parsed.candidates.len(), 1);
    }

    #[test]
    fn prose_wrapped_json() {
        let raw = format!("Sure, here you go:\n{}\nHope that helps!", ok_json());
        let parsed = parse_response(&raw).expect("prose");
        assert_eq!(parsed.candidates.len(), 1);
    }

    #[test]
    fn extra_keys_tolerated() {
        let raw = r#"{
            "commands": [{"command": "ls", "explanation": "list", "confidence": 1.0}],
            "bonus": true,
            "model_comment": "ignored"
        }"#;
        let parsed = parse_response(raw).expect("extra keys");
        assert_eq!(parsed.candidates[0].command, "ls");
    }

    #[test]
    fn extra_keys_on_candidate_tolerated() {
        let raw = r#"{
            "commands": [{
                "command": "ls",
                "explanation": "list",
                "confidence": 0.5,
                "dialect": "zsh"
            }]
        }"#;
        let parsed = parse_response(raw).expect("extra candidate keys");
        assert_eq!(parsed.candidates[0].command, "ls");
    }

    #[test]
    fn truncated_json() {
        let err = parse_response(r#"{ "commands": [ {"command": "ls" }"#).unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::Truncated);
    }

    #[test]
    fn wrong_types() {
        let err = parse_response(r#"{ "commands": "ls" }"#).unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::Schema);
        let err = parse_response(r#"{ "commands": [{ "command": 1 }] }"#).unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::Schema);
        let err = parse_response(r#"{ "commands": [{ "command": "ls", "confidence": "high" }] }"#)
            .unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::Schema);
    }

    #[test]
    fn missing_commands() {
        let err = parse_response(r#"{ "assumptions": [] }"#).unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::Schema);
    }

    #[test]
    fn no_json() {
        let err = parse_response("sorry I cannot help with that").unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::NoJson);
    }
}
