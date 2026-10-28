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
    let at = match path {
        "" => name.to_owned(),
        _ => format!("{path}.{name}"),
    };
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

pub fn text(
    fields: &Map<String, Value>,
    name: &str,
    path: &str,
    faults: &mut Vec<String>,
) -> Option<String> {
    match fields.get(name) {
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => {
            faults.push(format!(
                "{path}.{name}: takes a string, not {}",
                shape(other)
            ));
            None
        }
        None => {
            faults.push(format!("{path}.{name}: missing"));
            None
        }
    }
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
