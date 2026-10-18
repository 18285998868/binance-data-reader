use crate::app::Record;

/// Column-oriented OHLCV output.
#[derive(Debug, Clone)]
pub struct KlineColumns {
    /// Bar open times in milliseconds.
    pub time: Vec<i64>,
    /// Open prices.
    pub open: Vec<f64>,
    /// High prices.
    pub high: Vec<f64>,
    /// Low prices.
    pub low: Vec<f64>,
    /// Close prices.
    pub close: Vec<f64>,
    /// Volumes.
    pub volume: Vec<f64>,
}

impl KlineColumns {
    #[must_use]
    pub const fn len(&self) -> usize {
        self.close.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.close.is_empty()
    }
}

impl From<Vec<Record>> for KlineColumns {
    fn from(records: Vec<Record>) -> Self {
        let n = records.len();
        let mut cols = KlineColumns {
            time: Vec::with_capacity(n),
            open: Vec::with_capacity(n),
            high: Vec::with_capacity(n),
            low: Vec::with_capacity(n),
            close: Vec::with_capacity(n),
            volume: Vec::with_capacity(n),
        };
        for r in records {
            cols.time.push(r.open_time);
            cols.open.push(r.open);
            cols.high.push(r.high);
            cols.low.push(r.low);
            cols.close.push(r.close);
            cols.volume.push(r.volume);
        }
        cols
    }
}
