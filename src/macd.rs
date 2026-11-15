//! MACD(12, 26, 9) over retained raw closes: SMA-seeded EMAs, hist = DIF - DEA.
use crate::analyzer::Analyzer;

pub const PARAMS: [usize; 3] = [12, 26, 9];
/// Bars before the first finite histogram value: 26 - 1 + 9 - 1 + 1.
pub const LOOKBACK: usize = 33;

/// One value per retained raw bar; NaN before warm-up.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacdSeries {
    pub dif: Vec<f64>,
    pub dea: Vec<f64>,
    pub hist: Vec<f64>,
}

fn ema_sma_seed(series: &[f64], n: usize) -> Vec<f64> {
    let len = series.len();
    let mut out = vec![f64::NAN; len];
    if len < n || n == 0 {
        return out;
    }
    let alpha = 2.0 / (n as f64 + 1.0);
    out[n - 1] = series[..n].iter().sum::<f64>() / n as f64;
    for i in n..len {
        out[i] = alpha * series[i] + (1.0 - alpha) * out[i - 1];
    }
    out
}

fn ema_after_leading_nan(series: &[f64], n: usize) -> Vec<f64> {
    let mut out = vec![f64::NAN; series.len()];
    let Some(start) = series.iter().position(|x| x.is_finite()) else {
        return out;
    };
    for (i, value) in ema_sma_seed(&series[start..], n).into_iter().enumerate() {
        out[start + i] = value;
    }
    out
}

/// TA-Lib convention: the short EMA starts at `long - short` so both EMAs
/// seed on the same bar.
pub fn macd_series(series: &[f64], short: usize, long: usize, m: usize) -> MacdSeries {
    let len = series.len();
    let mut out = MacdSeries {
        dif: vec![f64::NAN; len],
        dea: vec![f64::NAN; len],
        hist: vec![f64::NAN; len],
    };
    if len == 0 || short == 0 || long == 0 || m == 0 || len < long {
        return out;
    }
    let offset = long.saturating_sub(short);
    let ema_short = ema_sma_seed(&series[offset..], short);
    let ema_long = ema_sma_seed(series, long);
    let mut dif_raw = vec![f64::NAN; len];
    for i in offset..len {
        let s = ema_short[i - offset];
        if s.is_finite() && ema_long[i].is_finite() {
            dif_raw[i] = s - ema_long[i];
        }
    }
    let dea = ema_after_leading_nan(&dif_raw, m);
    for i in 0..len {
        if dif_raw[i].is_finite() && dea[i].is_finite() {
            out.dif[i] = dif_raw[i];
            out.hist[i] = dif_raw[i] - dea[i];
        }
    }
    out.dea = dea;
    out
}

/// MACD(12, 26, 9) of the analyzer's retained raw bars.
pub fn retained_macd(c: &Analyzer) -> MacdSeries {
    let closes: Vec<f64> = c.raw_bars().iter().map(|b| b.close).collect();
    let [short, long, m] = PARAMS;
    macd_series(&closes, short, long, m)
}
