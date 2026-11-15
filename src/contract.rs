//! JSON request/response contract of `chanorbit geometry`. Pure functions:
//! text in, text out; the binary owns stdin/stdout.
use crate::analyzer::{Analyzer, Config};
use crate::bar::RawBar;
use crate::geometry::{Geometry, geometry};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Output contract version, carried by every response.
pub const SCHEMA: &str = "chanorbit.geometry/1";

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub schema: String,
    pub id: Value,
    #[serde(flatten)]
    pub geometry: Geometry,
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

fn error(id: Value, kind: &str, message: impl Into<String>) -> ErrorResponse {
    ErrorResponse {
        schema: SCHEMA.into(),
        id,
        error: ErrorBody {
            kind: kind.into(),
            message: message.into(),
        },
    }
}

/// Parse an exchange-local wall-clock time. A zone suffix is rejected.
pub fn parse_dt(text: &str) -> Option<NaiveDateTime> {
    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(text, format) {
            return Some(dt);
        }
    }
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
}

impl Request {
    /// Validated bars in input order. `dt` must be non-decreasing; an equal
    /// neighbour is a revision of the previous bar.
    pub fn bars(&self) -> Result<Vec<RawBar>, (&'static str, String)> {
        let n = self.dt.len();
        if n == 0 {
            return Err(("invalid_request", "dt is empty".into()));
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
                        format!("dt[{i}]: not a naive ISO time: {}", self.dt[i]),
                    )
                })?;
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

/// Run one parsed request with the default stroke parameters.
pub fn run(req: Request) -> Result<Response, ErrorResponse> {
    let bars = req
        .bars()
        .map_err(|(kind, message)| error(req.id.clone(), kind, message))?;
    Ok(respond(
        req.id,
        &Analyzer::from_bars(bars, Config::default()),
    ))
}

/// One request text in, one compact JSON line out (no trailing newline).
/// Returns whether the response is a success.
pub fn handle(text: &str) -> (String, bool) {
    let result = match serde_json::from_str::<Request>(text) {
        Ok(req) => run(req),
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
            Err(error(id, kind, e.to_string()))
        }
    };
    match result {
        Ok(r) => (
            serde_json::to_string(&r).expect("response serializes"),
            true,
        ),
        Err(e) => (serde_json::to_string(&e).expect("error serializes"), false),
    }
}
