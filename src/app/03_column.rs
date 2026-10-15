use crate::app::Record;

#[derive(Debug, Clone)]
pub struct KlineColumns {
    pub time: Vec<i64>,
    pub open: Vec<f64>,
    pub high: Vec<f64>,
    pub low: Vec<f64>,
    pub close: Vec<f64>,
    pub volume: Vec<f64>,
}

impl KlineColumns {
    pub fn len(&self) -> usize {
        self.close.len()
    }

    pub fn is_empty(&self) -> bool {
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
