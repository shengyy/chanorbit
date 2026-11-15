//! JSON request/response contract of `chanorbit geometry`, `stages` and
//! `levels`. Pure functions: text in, text out; the binary owns stdin/stdout.
use crate::analyzer::{Analyzer, Config};
use crate::bar::RawBar;
use crate::geometry::{Geometry, geometry};
use crate::levels_child::{LevelOut, levels};
use crate::observe::{StageFact, observed_stages};
use chrono::NaiveDateTime;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Output contract version of `geometry`, carried by each of its responses.
pub const SCHEMA: &str = "chanorbit.geometry/1";
/// Output contract version of `stages`.
pub const STAGES_SCHEMA: &str = "chanorbit.stages/1";
/// Output contract version of `levels`.
pub const LEVELS_SCHEMA: &str = "chanorbit.levels/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Geometry,
    Stages,
    Levels,
}

impl Command {
    pub fn schema(self) -> &'static str {
        match self {
            Command::Geometry => SCHEMA,
            Command::Stages => STAGES_SCHEMA,
            Command::Levels => LEVELS_SCHEMA,
        }
    }
}

/// Columnar bars: one array per field, equal lengths, in input order.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    #[serde(default)]
    pub id: Value,
    pub dt: Vec<String>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub vol: Vec<f64>,
    /// `stages` only: first input time to observe. When present it must be a
    /// string; `null` is a wrong type, not an absent field.
    #[serde(default, deserialize_with = "present_string")]
    pub observe_from: Option<String>,
}

/// One instrument, several period series, coarsest first.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelsRequest {
    #[serde(default)]
    pub id: Value,
    pub levels: Vec<LevelInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelInput {
    pub name: String,
    pub dt: Vec<String>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub vol: Vec<f64>,
}

fn present_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    String::deserialize(d).map(Some)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub schema: String,
    pub id: Value,
    #[serde(flatten)]
    pub geometry: Geometry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagesResponse {
    pub schema: String,
    pub id: Value,
    pub stages: Vec<StageFact>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LevelsResponse {
    pub schema: String,
    pub id: Value,
    pub levels: Vec<LevelOut>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub schema: String,
    pub id: Value,
    pub error: ErrorBody,
}

fn error(command: Command, id: Value, kind: &str, message: impl Into<String>) -> ErrorResponse {
    ErrorResponse {
        schema: command.schema().into(),
        id,
        error: ErrorBody {
            kind: kind.into(),
            message: message.into(),
        },
    }
}

/// Most bars accepted in one request.
pub const MAX_BARS: usize = 200_000;
/// Prices must lie in `(0, MAX_PRICE]`.
pub const MAX_PRICE: f64 = 1e9;
/// Volumes must lie in `[0, MAX_VOLUME]`.
pub const MAX_VOLUME: f64 = 1e15;

/// Parse an exchange-local wall-clock time written exactly as
/// `YYYY-MM-DDTHH:MM:SS` (no fraction, no zone, no padding variants).
pub fn parse_dt(text: &str) -> Option<NaiveDateTime> {
    let b = text.as_bytes();
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    let shape = b.len() == 19
        && digits.iter().all(|&i| b[i].is_ascii_digit())
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':';
    if !shape {
        return None;
    }
    let dt = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").ok()?;
    // A leap second (`:60`) parses as a fraction; reject it.
    (dt.and_utc().timestamp_subsec_nanos() == 0).then_some(dt)
}

impl Request {
    /// Validated bars in input order. `dt` must be non-decreasing; an equal
    /// neighbour is a revision of the previous bar.
    pub fn bars(&self) -> Result<Vec<RawBar>, (&'static str, String)> {
        let n = self.dt.len();
        if n == 0 {
            return Err(("invalid_request", "dt is empty".into()));
        }
        if n > MAX_BARS {
            return Err((
                "invalid_input",
                format!("{n} bars exceed the limit of {MAX_BARS}"),
            ));
        }
        for (name, len) in [
            ("high", self.high.len()),
            ("low", self.low.len()),
            ("close", self.close.len()),
            ("vol", self.vol.len()),
        ] {
            if len != n {
                return Err((
                    "invalid_request",
                    format!("{name} has {len} values, dt has {n}"),
                ));
            }
        }
        let mut previous: Option<NaiveDateTime> = None;
        (0..n)
            .map(|i| {
                let dt = parse_dt(&self.dt[i]).ok_or_else(|| {
                    (
                        "invalid_bar",
                        format!("dt[{i}]: not YYYY-MM-DDTHH:MM:SS: {}", self.dt[i]),
                    )
                })?;
                for (name, value) in [
                    ("high", self.high[i]),
                    ("low", self.low[i]),
                    ("close", self.close[i]),
                ] {
                    if !(value > 0.0 && value <= MAX_PRICE) {
                        return Err((
                            "invalid_bar",
                            format!("{name}[{i}]: {value} outside (0, {MAX_PRICE:e}]"),
                        ));
                    }
                }
                let (high, low, close) = (self.high[i], self.low[i], self.close[i]);
                if high < low {
                    return Err((
                        "invalid_bar",
                        format!("high[{i}]: {high} below low[{i}] {low}"),
                    ));
                }
                if !(low..=high).contains(&close) {
                    return Err((
                        "invalid_bar",
                        format!(
                            "close[{i}]: {close} outside [low[{i}], high[{i}]] = [{low}, {high}]"
                        ),
                    ));
                }
                if !(self.vol[i] >= 0.0 && self.vol[i] <= MAX_VOLUME) {
                    return Err((
                        "invalid_bar",
                        format!("vol[{i}]: {} outside [0, {MAX_VOLUME:e}]", self.vol[i]),
                    ));
                }
                if previous.is_some_and(|p| dt < p) {
                    return Err((
                        "invalid_bar",
                        format!("dt[{i}]: {} is earlier than dt[{}]", self.dt[i], i - 1),
                    ));
                }
                previous = Some(dt);
                Ok(RawBar {
                    dt,
                    high: self.high[i],
                    low: self.low[i],
                    close: self.close[i],
                    vol: self.vol[i],
                })
            })
            .collect()
    }
}

/// The response for an analyzer state.
pub fn respond(id: Value, c: &Analyzer) -> Response {
    Response {
        schema: SCHEMA.into(),
        id,
        geometry: geometry(c),
    }
}

/// Run one parsed `geometry` request with the default stroke parameters.
pub fn run(req: Request) -> Result<Response, ErrorResponse> {
    let fail = |kind, message| error(Command::Geometry, req.id.clone(), kind, message);
    if req.observe_from.is_some() {
        return Err(fail(
            "invalid_request",
            "observe_from applies only to stages".into(),
        ));
    }
    let bars = req.bars().map_err(|(kind, message)| fail(kind, message))?;
    Ok(respond(
        req.id,
        &Analyzer::from_bars(bars, Config::default()),
    ))
}

/// Run one parsed `stages` request with the default stroke parameters.
pub fn run_stages(req: Request) -> Result<StagesResponse, ErrorResponse> {
    let fail = |kind, message| error(Command::Stages, req.id.clone(), kind, message);
    let observe_from = match &req.observe_from {
        None => None,
        Some(text) => Some(parse_dt(text).ok_or_else(|| {
            fail(
                "invalid_request",
                format!("observe_from: not YYYY-MM-DDTHH:MM:SS: {text}"),
            )
        })?),
    };
    let bars = req.bars().map_err(|(kind, message)| fail(kind, message))?;
    Ok(StagesResponse {
        schema: STAGES_SCHEMA.into(),
        id: req.id,
        stages: observed_stages(&bars, observe_from, Config::default()),
    })
}

/// Run one parsed `levels` request with the default stroke parameters.
pub fn run_levels(req: LevelsRequest) -> Result<LevelsResponse, ErrorResponse> {
    let fail = |kind, message| error(Command::Levels, req.id.clone(), kind, message);
    if req.levels.is_empty() {
        return Err(fail("invalid_request", "levels is empty".into()));
    }
    let mut inputs: Vec<(String, Vec<RawBar>)> = Vec::with_capacity(req.levels.len());
    for (i, level) in req.levels.into_iter().enumerate() {
        if level.name.is_empty() {
            return Err(fail(
                "invalid_request",
                format!("levels[{i}].name is empty"),
            ));
        }
        if let Some(j) = inputs.iter().position(|(n, _)| *n == level.name) {
            return Err(fail(
                "invalid_request",
                format!("levels[{i}].name repeats levels[{j}].name: {}", level.name),
            ));
        }
        let series = Request {
            id: Value::Null,
            dt: level.dt,
            high: level.high,
            low: level.low,
            close: level.close,
            vol: level.vol,
            observe_from: None,
        };
        let bars = series
            .bars()
            .map_err(|(kind, message)| fail(kind, format!("levels[{i}].{message}")))?;
        inputs.push((level.name, bars));
    }
    Ok(LevelsResponse {
        schema: LEVELS_SCHEMA.into(),
        id: req.id,
        levels: levels(&inputs),
    })
}

fn first_byte_is_brace(text: &str) -> bool {
    text.trim_start_matches([' ', '\t', '\n', '\r'])
        .starts_with('{')
}

fn encode<T: Serialize>(result: Result<T, ErrorResponse>) -> (String, bool) {
    match result {
        Ok(r) => (
            serde_json::to_string(&r).expect("response serializes"),
            true,
        ),
        Err(e) => (serde_json::to_string(&e).expect("error serializes"), false),
    }
}

/// One `geometry` request text in, one compact JSON line out (no trailing
/// newline). Returns whether the response is a success.
pub fn handle(text: &str) -> (String, bool) {
    handle_command(Command::Geometry, text)
}

/// Parse one request object for `command`.
fn parse<T: DeserializeOwned>(command: Command, text: &str) -> Result<T, ErrorResponse> {
    if !first_byte_is_brace(text) {
        let kind = if serde_json::from_str::<Value>(text).is_ok() {
            "invalid_input"
        } else {
            "invalid_json"
        };
        return Err(error(
            command,
            Value::Null,
            kind,
            "a request must be one JSON object",
        ));
    }
    serde_json::from_str::<T>(text).map_err(|e| {
        // Keep the caller's id when the outer object is readable.
        let id = serde_json::from_str::<Value>(text)
            .ok()
            .and_then(|v| v.get("id").cloned())
            .unwrap_or(Value::Null);
        let kind = if e.is_syntax() || e.is_eof() {
            "invalid_json"
        } else {
            "invalid_request"
        };
        error(command, id, kind, e.to_string())
    })
}

/// One request text for `command` in, one compact JSON line out.
pub fn handle_command(command: Command, text: &str) -> (String, bool) {
    match command {
        Command::Geometry => match parse::<Request>(command, text) {
            Err(e) => encode::<Response>(Err(e)),
            Ok(req) => contained(command, req.id.clone(), || {
                #[cfg(test)]
                tests::panic_on_probe(&req.id);
                run(req)
            }),
        },
        Command::Stages => match parse::<Request>(command, text) {
            Err(e) => encode::<Response>(Err(e)),
            Ok(req) => contained(command, req.id.clone(), || {
                #[cfg(test)]
                tests::panic_on_probe(&req.id);
                run_stages(req)
            }),
        },
        Command::Levels => match parse::<LevelsRequest>(command, text) {
            Err(e) => encode::<Response>(Err(e)),
            Ok(req) => contained(command, req.id.clone(), || {
                #[cfg(test)]
                tests::panic_on_probe(&req.id);
                run_levels(req)
            }),
        },
    }
}

/// Run one request's computation; a panic inside it becomes that request's
/// `internal_error` instead of ending the process.
fn contained<T: Serialize>(
    command: Command,
    id: Value,
    compute: impl FnOnce() -> Result<T, ErrorResponse>,
) -> (String, bool) {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(compute)) {
        Ok(result) => encode(result),
        Err(payload) => encode::<Response>(Err(error(
            command,
            id,
            "internal_error",
            panic_message(payload.as_ref()),
        ))),
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    let text = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-text panic payload");
    format!("panic while computing the request: {text}")
}

/// [`handle_command`] for raw bytes; text that is not UTF-8 is an
/// `invalid_json` error. A panic outside the computation (where the id is
/// not known) is an `internal_error` with a `null` id.
pub fn handle_bytes(command: Command, bytes: &[u8]) -> (String, bool) {
    std::panic::catch_unwind(|| handle_bytes_unguarded(command, bytes)).unwrap_or_else(|payload| {
        encode::<Response>(Err(error(
            command,
            Value::Null,
            "internal_error",
            panic_message(payload.as_ref()),
        )))
    })
}

fn handle_bytes_unguarded(command: Command, bytes: &[u8]) -> (String, bool) {
    match std::str::from_utf8(bytes) {
        Ok(text) => handle_command(command, text),
        Err(e) => (
            serde_json::to_string(&error(
                command,
                Value::Null,
                "invalid_json",
                format!("not UTF-8: {e}"),
            ))
            .expect("error serializes"),
            false,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: &str = "panic-probe";

    /// Test builds only: a request with this id panics inside its computation.
    pub(super) fn panic_on_probe(id: &Value) {
        if *id == PROBE {
            panic!("probe panic for {PROBE}");
        }
    }

    fn request(id: &str) -> String {
        format!(
            r#"{{"id":"{id}","dt":["2020-01-02T15:00:00","2020-01-03T15:00:00"],"high":[2.0,2.1],"low":[1.0,1.1],"close":[1.5,1.6],"vol":[1.0,1.0]}}"#
        )
    }

    #[test]
    fn a_panic_in_one_request_is_its_internal_error() {
        for command in [Command::Geometry, Command::Stages] {
            let lines = [request("a"), request(PROBE), request("b")];
            let out: Vec<(Value, bool)> = lines
                .iter()
                .map(|l| {
                    let (text, ok) = handle_bytes(command, l.as_bytes());
                    (serde_json::from_str(&text).unwrap(), ok)
                })
                .collect();
            assert!(out[0].1 && out[2].1);
            assert_eq!(
                (&out[0].0["id"], &out[2].0["id"]),
                (&"a".into(), &"b".into())
            );
            let (e, ok) = &out[1];
            assert!(!ok);
            assert_eq!(e["schema"], command.schema());
            assert_eq!(e["id"], PROBE);
            assert_eq!(e["error"]["kind"], "internal_error");
            assert_eq!(
                e["error"]["message"],
                format!("panic while computing the request: probe panic for {PROBE}")
            );
            // The neighbours answer exactly as on their own.
            assert_eq!(
                out[0].0,
                serde_json::from_str::<Value>(&handle_command(command, &lines[0]).0).unwrap()
            );
        }
    }

    #[test]
    fn a_panic_in_a_levels_request_is_its_internal_error() {
        let level = r#"{"name":"d","dt":["2020-01-02T15:00:00"],"high":[2.0],"low":[1.0],"close":[1.5],"vol":[1.0]}"#;
        let ok = format!(r#"{{"id":"a","levels":[{level}]}}"#);
        let probe = format!(r#"{{"id":"{PROBE}","levels":[{level}]}}"#);
        assert!(handle_bytes(Command::Levels, ok.as_bytes()).1);
        let (text, ok) = handle_bytes(Command::Levels, probe.as_bytes());
        assert!(!ok);
        let e: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(e["schema"], LEVELS_SCHEMA);
        assert_eq!(e["id"], PROBE);
        assert_eq!(e["error"]["kind"], "internal_error");
    }

    #[test]
    fn a_non_text_panic_payload_is_named() {
        let (text, ok) = contained::<Response>(Command::Geometry, Value::Null, || {
            std::panic::panic_any(7_u8)
        });
        assert!(!ok);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            v["error"]["message"],
            "panic while computing the request: non-text panic payload"
        );
    }
}
