//! JSON request/response contract of `chanorbit geometry` and `chanorbit
//! stages`. Pure functions: text in, text out; the binary owns stdin/stdout.
use crate::analyzer::{Analyzer, Config};
use crate::bar::RawBar;
use crate::geometry::{Geometry, geometry};
use crate::observe::{StageFact, observed_stages};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Output contract version of `geometry`, carried by each of its responses.
pub const SCHEMA: &str = "chanorbit.geometry/1";
/// Output contract version of `stages`.
pub const STAGES_SCHEMA: &str = "chanorbit.stages/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Geometry,
    Stages,
}

impl Command {
    pub fn schema(self) -> &'static str {
        match self {
            Command::Geometry => SCHEMA,
            Command::Stages => STAGES_SCHEMA,
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
    /// `stages` only: first input time to observe.
    #[serde(default)]
    pub observe_from: Option<String>,
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

/// One request text for `command` in, one compact JSON line out.
pub fn handle_command(command: Command, text: &str) -> (String, bool) {
    let req = if !first_byte_is_brace(text) {
        let kind = if serde_json::from_str::<Value>(text).is_ok() {
            "invalid_input"
        } else {
            "invalid_json"
        };
        Err(error(
            command,
            Value::Null,
            kind,
            "a request must be one JSON object",
        ))
    } else {
        match serde_json::from_str::<Request>(text) {
            Ok(req) => Ok(req),
            Err(e) => {
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
                Err(error(command, id, kind, e.to_string()))
            }
        }
    };
    match (command, req) {
        (_, Err(e)) => encode::<Response>(Err(e)),
        (Command::Geometry, Ok(req)) => encode(run(req)),
        (Command::Stages, Ok(req)) => encode(run_stages(req)),
    }
}

/// [`handle_command`] for raw bytes; text that is not UTF-8 is an
/// `invalid_json` error.
pub fn handle_bytes(command: Command, bytes: &[u8]) -> (String, bool) {
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
