//! Walk a JSON body by hand.
//!
//! serde stops at the first fault. A body of the agent, such as a syllabus
//! or a quiz, gets every fault at once, each with its place in the body,
//! so the agent fixes them in one pass. A helper pushes the fault of a
//! field and answers None, and the walk goes on.

use serde_json::{Map, Value};

pub fn object<'a>(
    value: &'a Value,
    path: &str,
    faults: &mut Vec<String>,
) -> Option<&'a Map<String, Value>> {
    let object = value.as_object();
    if object.is_none() {
        faults.push(format!("{path}: takes an object, not {}", shape(value)));
    }
    object
}

/// An array field. `path` is "" for the top of the body.
pub fn array<'a>(
    fields: &'a Map<String, Value>,
    name: &str,
    path: &str,
    faults: &mut Vec<String>,
) -> Option<&'a Vec<Value>> {
    let at = at(path, name);
    match fields.get(name) {
        Some(Value::Array(items)) => Some(items),
        Some(other) => {
            faults.push(format!("{at}: takes an array, not {}", shape(other)));
            None
        }
        None => {
            faults.push(format!("{at}: missing"));
            None
        }
    }
}

/// A string field. `path` is "" for the top of the body.
pub fn text(
    fields: &Map<String, Value>,
    name: &str,
    path: &str,
    faults: &mut Vec<String>,
) -> Option<String> {
    let at = at(path, name);
    match fields.get(name) {
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => {
            faults.push(format!("{at}: takes a string, not {}", shape(other)));
            None
        }
        None => {
            faults.push(format!("{at}: missing"));
            None
        }
    }
}

/// The path of a field, as a fault names it. `path` is "" for the top of
/// the body.
pub fn at(path: &str, name: &str) -> String {
    match path {
        "" => name.to_owned(),
        _ => format!("{path}.{name}"),
    }
}

/// A string field that holds one short line, trimmed. `place` names where
/// the line shows, such as "the chart".
pub fn line(
    fields: &Map<String, Value>,
    name: &str,
    path: &str,
    most: usize,
    place: &str,
    faults: &mut Vec<String>,
) -> Option<String> {
    let at = at(path, name);
    let value = text(fields, name, path, faults)?;
    let value = value.trim();
    if value.is_empty() {
        faults.push(format!("{at}: empty"));
        return None;
    }
    short(value, &at, most, place, faults).then(|| value.to_owned())
}

/// Whether a text fits on its line of `place`. A fault says why not.
pub fn short(value: &str, path: &str, most: usize, place: &str, faults: &mut Vec<String>) -> bool {
    if value.contains(['\n', '\r']) {
        faults.push(format!("{path}: holds a line break. Keep it on one line"));
        return false;
    }
    let count = value.chars().count();
    if count > most {
        faults.push(format!(
            "{path}: holds {count} characters, and {most} at most fit on {place}"
        ));
        return false;
    }
    true
}

pub fn unknown(
    fields: &Map<String, Value>,
    known: &[&str],
    path: &str,
    hint: &str,
    faults: &mut Vec<String>,
) {
    for name in fields.keys().filter(|name| !known.contains(&name.as_str())) {
        faults.push(format!("{path}: unknown field {name:?}. {hint}"));
    }
}

/// The JSON type of a value, as a fault names it.
pub fn shape(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}
