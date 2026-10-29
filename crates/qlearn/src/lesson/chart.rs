//! The chart block: a bar or a line chart, in a small JSON format of qlearn.
//!
//! The server checks the whole format, so the page can draw every chart
//! that it gets, in the style of the page. See section 4.4 of the design,
//! and the entry "Holes found by the chart" of `stack.md` for the limits.
//!
//! Every fault comes at once, at the line of the fence, with the JSON path
//! of the field, such as `series[2].values`.

use std::collections::HashSet;

use serde_json::{Map, Value};

use super::Fault;
use crate::json::{self, array, object, shape, text};
use crate::model::{Chart, ChartKind, ChartSeries, ChartX, ChartY};
use crate::quiz::count_of;

const KIND: &str = "chart";

/// Where a text shows, as a fault names it.
const PLACE: &str = "the chart";

/// The most series of a chart. More colors stop being told apart.
pub const SERIES: usize = 8;

/// The most x values of a chart. A line of more points is a trend that a
/// sentence says better, and the labels no longer fit a narrow screen.
pub const X_VALUES: usize = 50;

/// The most bars of a bar chart, x values times series. More bars get too
/// thin to read on a narrow screen.
pub const BARS: usize = 60;

/// The most characters of each text, so that it holds on its line of the
/// chart on a narrow screen.
pub const TITLE: usize = 100;
pub const LABEL: usize = 60;
pub const X_VALUE: usize = 24;
pub const NAME: usize = 40;
pub const UNIT: usize = 12;

/// The largest size of a value. The page computes in floats and writes a
/// tick in full when its step is fine, so a larger value loses its digits.
pub const LARGEST: f64 = 1e15;

/// The narrowest y axis, as the page draws it. The page writes a tick with
/// 6 decimals at most, so a narrower axis shows the same text at each tick.
pub const NARROWEST: f64 = 1e-4;

/// How many times the y axis may go into the size of its values, so that
/// the floats of the page still tell the ticks apart.
pub const RESOLUTION: f64 = 1e9;

/// Read the JSON of a fence. `fence` is the line of its opening.
pub fn parse(body: &str, fence: usize) -> Result<Chart, Vec<Fault>> {
    let fault = |line: usize, message: String| Fault {
        line,
        kind: KIND,
        message,
    };
    let value = super::fence_json(
        body,
        fence,
        KIND,
        "the fence holds no chart: write its JSON inside it",
    )?;

    let mut faults = Vec::new();
    let chart = read(&value, &mut faults);
    match (chart, faults.is_empty()) {
        (Some(chart), true) => Ok(chart),
        _ => Err(faults
            .into_iter()
            .map(|message| fault(fence, message))
            .collect()),
    }
}

fn read(value: &Value, faults: &mut Vec<String>) -> Option<Chart> {
    let fields = object(value, "the chart", faults)?;
    for name in fields.keys() {
        if !["kind", "title", "x", "y", "series"].contains(&name.as_str()) {
            faults.push(format!(
                "unknown field {name:?}. A chart takes kind, title, x, y and series"
            ));
        }
    }

    let kind = read_kind(fields.get("kind"), faults);
    let title = filled(fields, "title", "", TITLE, faults);
    let x = read_x(fields.get("x"), faults);
    let y = read_y(fields.get("y"), faults);
    let series = read_series(fields, kind, x.as_ref(), faults);

    Some(Chart {
        kind: kind?,
        title: title?,
        x: x?,
        y: y?,
        series: series?,
    })
}

fn read_kind(value: Option<&Value>, faults: &mut Vec<String>) -> Option<ChartKind> {
    match value {
        Some(Value::String(kind)) if kind == "bar" => Some(ChartKind::Bar),
        Some(Value::String(kind)) if kind == "line" => Some(ChartKind::Line),
        Some(other) => {
            faults.push(format!("kind: takes \"bar\" or \"line\", not {other}"));
            None
        }
        None => {
            faults.push("kind: missing. Write \"bar\" or \"line\"".to_owned());
            None
        }
    }
}

/// An object field, such as `x`.
fn field<'a>(
    value: Option<&'a Value>,
    path: &str,
    known: &[&str],
    hint: &str,
    faults: &mut Vec<String>,
) -> Option<&'a Map<String, Value>> {
    let Some(value) = value else {
        faults.push(format!("{path}: missing"));
        return None;
    };
    let fields = object(value, path, faults)?;
    for name in fields.keys().filter(|name| !known.contains(&name.as_str())) {
        faults.push(format!("{path}: unknown field {name:?}. {hint}"));
    }
    Some(fields)
}

fn read_x(value: Option<&Value>, faults: &mut Vec<String>) -> Option<ChartX> {
    let fields = field(
        value,
        "x",
        &["label", "values"],
        "x takes label and values",
        faults,
    )?;
    let label = filled(fields, "label", "x", LABEL, faults);

    let mut values = Some(Vec::new());
    if let Some(items) = array(fields, "values", "x", faults) {
        if items.is_empty() {
            faults.push("x.values: empty. Give the value of each bar or point".to_owned());
            values = None;
        }
        if items.len() > X_VALUES {
            faults.push(format!(
                "x.values: holds {}, and a chart takes {X_VALUES} at most",
                count_of(items.len(), "value")
            ));
        }
        let mut seen = HashSet::new();
        for (n, item) in items.iter().enumerate() {
            let path = format!("x.values[{n}]");
            let value = match item {
                Value::String(text) => Some(text.trim().to_owned()),
                Value::Number(number) => Some(number.to_string()),
                other => {
                    faults.push(format!(
                        "{path}: takes a string or a number, not {}",
                        shape(other)
                    ));
                    None
                }
            }
            .filter(|value| match value.is_empty() {
                true => {
                    faults.push(format!("{path}: empty"));
                    false
                }
                false => short(value, &path, X_VALUE, faults),
            });
            // Two bars with one name would read as one category.
            let value = value.filter(|value| {
                let first = seen.insert(value.clone());
                if !first {
                    faults.push(format!("{path}: {value:?} comes twice in x"));
                }
                first
            });
            match (value, values.as_mut()) {
                (Some(value), Some(values)) => values.push(value),
                _ => values = None,
            }
        }
    } else {
        values = None;
    }

    Some(ChartX {
        label: label?,
        values: values?,
    })
}

fn read_y(value: Option<&Value>, faults: &mut Vec<String>) -> Option<ChartY> {
    let fields = field(
        value,
        "y",
        &["label", "unit"],
        "y takes label and unit",
        faults,
    )?;
    let label = filled(fields, "label", "y", LABEL, faults);
    // A count has no unit.
    let unit = match fields.get("unit") {
        None | Some(Value::Null) => Some(String::new()),
        Some(_) => text(fields, "unit", "y", faults)
            .map(|unit| unit.trim().to_owned())
            .filter(|unit| short(unit, "y.unit", UNIT, faults)),
    };

    Some(ChartY {
        label: label?,
        unit: unit?,
    })
}

fn read_series(
    fields: &Map<String, Value>,
    kind: Option<ChartKind>,
    x: Option<&ChartX>,
    faults: &mut Vec<String>,
) -> Option<Vec<ChartSeries>> {
    let items = array(fields, "series", "", faults)?;
    if items.is_empty() {
        faults.push("series: empty. Give at least one series".to_owned());
        return None;
    }
    if items.len() > SERIES {
        faults.push(format!(
            "series: holds {} series, and a chart draws {SERIES} at most. Split it into two \
             charts",
            items.len()
        ));
    }
    if let (Some(ChartKind::Bar), Some(x)) = (kind, x) {
        let bars = items.len() * x.values.len();
        if bars > BARS {
            faults.push(format!(
                "series: {} series of {} make {bars} bars, and a bar chart draws {BARS} at \
                 most. Take fewer series or fewer x values",
                items.len(),
                count_of(x.values.len(), "value")
            ));
        }
    }

    let mut names = HashSet::new();
    let mut series = Some(Vec::new());
    for (n, item) in items.iter().enumerate() {
        let path = format!("series[{n}]");
        let read = read_one(item, &path, kind, x, faults).filter(|one| {
            // The legend tells the series apart by their names.
            let first = names.insert(one.name.clone());
            if !first {
                faults.push(format!("{path}.name: {:?} comes twice in series", one.name));
            }
            first
        });
        match (read, series.as_mut()) {
            (Some(one), Some(series)) => series.push(one),
            _ => series = None,
        }
    }
    if let (Some(kind), Some(series)) = (kind, &series) {
        span(kind, series, faults);
    }
    series
}

/// Whether the page can tell the values apart on its y axis. The axis is
/// the one of `extent` in `web/src/chart.ts`.
fn span(kind: ChartKind, series: &[ChartSeries], faults: &mut Vec<String>) {
    let values = series
        .iter()
        .flat_map(|one| &one.values)
        .filter_map(|value| value.as_ref().and_then(serde_json::Number::as_f64));
    let (mut lo, mut hi) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    if lo > hi {
        return;
    }
    if kind == ChartKind::Bar {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    let size = lo.abs().max(hi.abs());
    let axis = match kind {
        _ if lo < hi => hi - lo,
        ChartKind::Bar => 1.0,
        // One value in a line: the page pads the axis by its size.
        ChartKind::Line => 2.0 * if lo == 0.0 { 1.0 } else { lo.abs() },
    };
    let need = NARROWEST.max(size / RESOLUTION);
    if axis < need {
        let hint = if need > NARROWEST {
            ", or draw each value minus a base value"
        } else {
            ""
        };
        faults.push(format!(
            "series: the y axis spans {}, and needs {} at least to tell the values apart. \
             Change the unit of y{hint}",
            number(axis),
            number(need)
        ));
    }
}

/// A number as the agent writes it: 1e-7, not 0.0000001, and 4 digits at
/// most, so that the noise of a float stays out of the fault.
fn number(value: f64) -> String {
    let value: f64 = format!("{value:.3e}").parse().unwrap_or(value);
    if value == 0.0 || (1e-4..1e15).contains(&value.abs()) {
        format!("{value}")
    } else {
        format!("{value:e}")
    }
}

fn read_one(
    value: &Value,
    path: &str,
    kind: Option<ChartKind>,
    x: Option<&ChartX>,
    faults: &mut Vec<String>,
) -> Option<ChartSeries> {
    let fields = field(
        Some(value),
        path,
        &["name", "values"],
        "A series takes name and values",
        faults,
    )?;
    let name = filled(fields, "name", path, NAME, faults);

    let items = array(fields, "values", path, faults)?;
    if let Some(x) = x
        && items.len() != x.values.len()
    {
        faults.push(format!(
            "{path}.values: holds {}, and x holds {}. Give one value for each x value",
            count_of(items.len(), "value"),
            x.values.len()
        ));
    }
    let mut values = Some(Vec::new());
    for (n, item) in items.iter().enumerate() {
        let value = match item {
            Value::Number(found) if found.as_f64().is_some_and(|v| v.abs() > LARGEST) => {
                faults.push(format!(
                    "{path}.values[{n}]: {} is beyond {}, the largest size that a chart \
                     draws. Change the unit of y",
                    number(found.as_f64().unwrap_or_default()),
                    number(LARGEST)
                ));
                None
            }
            Value::Number(number) => Some(Some(number.clone())),
            // A gap in the line: no measure at this x value.
            Value::Null if kind != Some(ChartKind::Bar) => Some(None),
            Value::Null => {
                faults.push(format!(
                    "{path}.values[{n}]: a bar chart takes a number, not null. Write 0, or \
                     take the x value out"
                ));
                None
            }
            other => {
                faults.push(format!(
                    "{path}.values[{n}]: takes a number, not {}",
                    shape(other)
                ));
                None
            }
        };
        match (value, values.as_mut()) {
            (Some(value), Some(values)) => values.push(value),
            _ => values = None,
        }
    }
    if let Some(found) = &values
        && !found.is_empty()
        && found.iter().all(Option::is_none)
    {
        faults.push(format!(
            "{path}.values: holds null only. Give a number for at least one x value"
        ));
        values = None;
    }

    Some(ChartSeries {
        name: name?,
        values: values?,
    })
}

/// A string field that holds one short line of the chart, trimmed.
fn filled(
    fields: &Map<String, Value>,
    name: &str,
    path: &str,
    most: usize,
    faults: &mut Vec<String>,
) -> Option<String> {
    json::line(fields, name, path, most, PLACE, faults)
}

/// Whether a text fits on its line of the chart. A fault says why not.
fn short(value: &str, path: &str, most: usize, faults: &mut Vec<String>) -> bool {
    json::short(value, path, most, PLACE, faults)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn faults(body: &str) -> Vec<String> {
        parse(body, 10)
            .unwrap_err()
            .into_iter()
            .map(|fault| fault.display("lesson.md"))
            .collect()
    }

    const GOOD: &str = r#"{
  "kind": "bar",
  "title": "Election time",
  "x": { "label": "Timeout range (ms)", "values": ["150-300", 150] },
  "y": { "label": "Median election time", "unit": "ms" },
  "series": [{ "name": "5 servers", "values": [283, 15.5] }]
}"#;

    #[test]
    fn a_number_of_x_comes_as_its_text_and_a_value_stays_a_number() {
        let chart = parse(GOOD, 10).unwrap();

        assert_eq!(chart.x.values, ["150-300", "150"]);
        let values: Vec<_> = chart.series[0]
            .values
            .iter()
            .map(|value| value.as_ref().and_then(serde_json::Number::as_f64))
            .collect();
        assert_eq!(values, [Some(283.0), Some(15.5)]);
        assert_eq!(
            serde_json::to_value(&chart.series[0]).unwrap(),
            serde_json::json!({ "name": "5 servers", "values": [283, 15.5] })
        );
    }

    /// The agent copies the example of the procedure, so it must pass.
    #[test]
    fn the_example_of_the_procedure_passes() {
        let (_, rest) = crate::agent::SKILL.split_once("```chart\n").unwrap();
        let (body, _) = rest.split_once("```").unwrap();

        let chart = parse(body, 1).unwrap();
        assert_eq!(chart.series.len(), 2);
    }

    #[test]
    fn a_syntax_error_names_its_line_in_the_page() {
        assert_eq!(
            faults("{\n  \"kind\": \"bar\",\n}\n"),
            ["lesson.md:13: chart: not valid JSON: trailing comma, at column 1"]
        );
        assert_eq!(
            faults("\n  \n"),
            ["lesson.md:10: chart: the fence holds no chart: write its JSON inside it"]
        );
    }

    #[test]
    fn the_largest_chart_passes_and_one_more_value_does_not() {
        let line = |count: usize| {
            let xs: Vec<String> = (0..count).map(|n| format!("\"{n}\"")).collect();
            let ys: Vec<String> = (0..count).map(|n| n.to_string()).collect();
            format!(
                r#"{{ "kind": "line", "title": "T", "x": {{ "label": "X", "values": [{}] }},
                "y": {{ "label": "Y" }}, "series": [{{ "name": "S", "values": [{}] }}] }}"#,
                xs.join(","),
                ys.join(",")
            )
        };

        assert!(parse(&line(X_VALUES), 1).is_ok());
        assert_eq!(
            faults(&line(X_VALUES + 1)),
            ["lesson.md:10: chart: x.values: holds 51 values, and a chart takes 50 at most"]
        );
    }

    fn line_of(values: &str) -> String {
        format!(
            r#"{{ "kind": "line", "title": "T", "x": {{ "label": "X", "values": ["a", "b"] }},
            "y": {{ "label": "Y" }}, "series": [{{ "name": "S", "values": [{values}] }}] }}"#
        )
    }

    /// The page labels its ticks with 6 decimals at most, and computes in
    /// floats: a value it cannot place is refused here, not drawn wrong.
    #[test]
    fn a_value_too_large_or_a_range_too_narrow_is_refused() {
        assert!(parse(&line_of("1500, 1520"), 1).is_ok());
        assert!(parse(&line_of("0.0002, 0.0004"), 1).is_ok());
        assert!(parse(&line_of("-1e15, 1e15"), 1).is_ok());

        assert_eq!(
            faults(&line_of("-1.7e308, 1.7e308")),
            [
                "lesson.md:10: chart: series[0].values[0]: -1.7e308 is beyond 1e15, the \
                 largest size that a chart draws. Change the unit of y",
                "lesson.md:10: chart: series[0].values[1]: 1.7e308 is beyond 1e15, the \
                 largest size that a chart draws. Change the unit of y",
            ]
        );
        assert_eq!(
            faults(&line_of("1e-7, 2e-7")),
            [
                "lesson.md:10: chart: series: the y axis spans 1e-7, and needs 0.0001 at least \
                 to tell the values apart. Change the unit of y"
            ]
        );
        // One value alone spans twice its size on the axis of a line.
        assert!(parse(&line_of("1e-7, null"), 1).is_err());
        // A bar grows from zero, so a small bar next to a large one is fine.
        let bar = line_of("1e-7, 1").replace("\"line\"", "\"bar\"");
        assert!(parse(&bar, 1).is_ok());
        // Next to its size, a range is too narrow for the floats of the page.
        assert_eq!(
            faults(&line_of("99999999999999, 1e14")),
            [
                "lesson.md:10: chart: series: the y axis spans 1, and needs 100000 at least to \
                 tell the values apart. Change the unit of y, or draw each value minus a \
                 base value"
            ]
        );
    }
}
