//! MACD(12, 26, 9) over raw closes: SMA-seeded EMAs, hist = DIF - DEA,
//! reported only from the bar where the seed no longer matters.
use crate::analyzer::Analyzer;

pub const PARAMS: [usize; 3] = [12, 26, 9];
/// Largest weight the long EMA's seed may still carry in a reported value.
pub const SEED_TOLERANCE: f64 = 1e-4;

/// One value per raw bar; NaN before [`ready_from`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MacdSeries {
    pub dif: Vec<f64>,
    pub dea: Vec<f64>,
    pub hist: Vec<f64>,
}

/// First bar index whose MACD no longer depends on where the series starts:
/// the long EMA is seeded at `long - 1` and the seed's weight at index `i` is
/// `((long - 1) / (long + 1))^(i - (long - 1))`; this is the first `i` at
/// which that weight is below `tolerance`. The short EMA (same seed bar) and
/// DEA (seeded later, but on `alpha = 2 / (m + 1)` with `m < long`) decay
/// faster, so the long EMA's seed decides.
pub fn ready_from(long: usize, tolerance: f64) -> usize {
    let decay = (long as f64 - 1.0) / (long as f64 + 1.0);
    let mut i = long - 1;
    let mut weight = 1.0;
    while weight >= tolerance {
        weight *= decay;
        i += 1;
    }
    i
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
/// seed on the same bar. Values before `ready_from(long, SEED_TOLERANCE)`
/// are NaN.
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
    for i in ready_from(long, SEED_TOLERANCE)..len {
        if dif_raw[i].is_finite() && dea[i].is_finite() {
            out.dif[i] = dif_raw[i];
            out.dea[i] = dea[i];
            out.hist[i] = dif_raw[i] - dea[i];
        }
    }
    out
}

/// MACD(12, 26, 9) of the analyzer's raw bars.
pub fn raw_macd(c: &Analyzer) -> MacdSeries {
    let closes: Vec<f64> = c.raw_bars().iter().map(|b| b.close).collect();
    let [short, long, m] = PARAMS;
    macd_series(&closes, short, long, m)
}

/// Bars before the first MACD value with the default parameters.
pub fn default_ready_from() -> usize {
    ready_from(PARAMS[1], SEED_TOLERANCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macd_starts_where_the_long_seed_weighs_less_than_the_tolerance() {
        assert_eq!(default_ready_from(), 145);
        let closes: Vec<f64> = (0..200).map(|i| 10.0 + (i as f64 * 0.3).sin()).collect();
        let s = macd_series(&closes, 12, 26, 9);
        for v in [&s.dif, &s.dea, &s.hist] {
            assert!(v[..145].iter().all(|x| x.is_nan()));
            assert!(v[145..].iter().all(|x| x.is_finite()));
        }
    }
}
