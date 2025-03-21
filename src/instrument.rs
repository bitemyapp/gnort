use std::{
    future::Future,
    sync::{
        atomic::{AtomicU64, AtomicUsize},
        Arc,
    },
};

use dogstatsd::DogstatsdError;

use crate::{
    GnortClient, MakeInstrument, MetricKey, MetricRegistrationError,
    MetricType::{self, Impl},
};

const DEFAULT_ORDERING: std::sync::atomic::Ordering = std::sync::atomic::Ordering::SeqCst;

#[derive(Clone, Debug, Default)]
pub struct AtomicF64 {
    storage: Arc<AtomicU64>,
}
impl AtomicF64 {
    pub(crate) fn swap(&self, value: f64) -> f64 {
        let as_u64 = value.to_bits();
        f64::from_bits(self.storage.swap(as_u64, DEFAULT_ORDERING))
    }
    pub(crate) fn load(&self) -> f64 {
        let as_u64 = self.storage.load(DEFAULT_ORDERING);
        f64::from_bits(as_u64)
    }
}

pub type CountUnit = usize;
pub type CountValue = Arc<AtomicUsize>;
pub type GaugeUnit = f64;
pub type GaugeValue = Arc<AtomicF64>;
pub type TimingUnit = CountUnit;
pub type TimingValue = CountValue;

#[derive(Clone, Debug, Default)]
pub struct Count(CountValue);

impl Count {
    const DEFAULT_VALUE: CountUnit = 0;
    pub fn increment(&self) -> CountUnit {
        self.fetch_add(1)
    }
    pub fn fetch_add(&self, val: usize) -> CountUnit {
        self.0.fetch_add(val, DEFAULT_ORDERING)
    }
    fn reset(&self) -> CountUnit {
        self.0.swap(Self::DEFAULT_VALUE, DEFAULT_ORDERING)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Gauge(GaugeValue);
impl Gauge {
    pub fn swap(&self, value: f64) -> GaugeUnit {
        self.0.swap(value)
    }
    pub fn load(&self) -> GaugeUnit {
        self.0.load()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub enum UnitOfTime {
    Micros,
    #[default]
    Millis,
    Seconds,
}

/// [](UnitOfTime)
#[derive(Clone, Debug, Default)]
pub struct TimingCount {
    sum: TimingValue,
    count: TimingValue,
    unit: UnitOfTime,
}

impl TimingCount {
    const DEFAULT_VALUE: TimingUnit = 0;
    pub fn with_unit(self, unit: UnitOfTime) -> Self {
        Self { unit, ..self }
    }
    pub fn add_timing(&self, duration: &std::time::Duration) -> (TimingUnit, TimingUnit) {
        self.add_timing_with_count(duration, 1)
    }
    fn duration_via_unit(unit: UnitOfTime, duration: &std::time::Duration) -> TimingUnit {
        match unit {
            UnitOfTime::Micros => duration.as_micros() as TimingUnit,
            UnitOfTime::Millis => duration.as_millis() as TimingUnit,
            UnitOfTime::Seconds => duration.as_secs() as TimingUnit,
        }
    }
    pub fn add_timing_with_count(
        &self,
        duration: &std::time::Duration,
        count: TimingUnit,
    ) -> (TimingUnit, TimingUnit) {
        let duration_sum = Self::duration_via_unit(self.unit, duration);
        let sum = self.sum.fetch_add(duration_sum, DEFAULT_ORDERING);
        let count = self.count.fetch_add(count, DEFAULT_ORDERING);
        (sum, count)
    }
    fn reset(&self) -> (CountUnit, CountUnit) {
        (
            self.sum.swap(Self::DEFAULT_VALUE, DEFAULT_ORDERING),
            self.count.swap(Self::DEFAULT_VALUE, DEFAULT_ORDERING),
        )
    }
    pub fn measure_sync_fn<T, F: FnOnce() -> T>(&self, f: F) -> T {
        let (result, duration) = Self::measure_sync_fn_(f);
        let _ = self.add_timing(&duration);
        result
    }
    pub fn measure_sync_fn_<T, F: FnOnce() -> T>(f: F) -> (T, std::time::Duration) {
        let start_time = std::time::Instant::now();
        let result = f();
        let end_time = std::time::Instant::now();
        let duration = end_time - start_time;
        (result, duration)
    }
    pub async fn measure_async_fut<T, F: Future<Output = T>>(&self, f: F) -> T {
        let (result, duration) = Self::measure_async_fut_(f).await;
        let _ = self.add_timing(&duration);
        result
    }
    pub async fn measure_async_fut_<T, F: Future<Output = T>>(f: F) -> (T, std::time::Duration) {
        let start_time = std::time::Instant::now();
        let result = f.await;
        let end_time = std::time::Instant::now();
        let duration = end_time - start_time;
        (result, duration)
    }
}

impl From<Count> for Instrument {
    fn from(count: Count) -> Self {
        Self::Count(count)
    }
}

impl From<TimingCount> for Instrument {
    fn from(timing_count: TimingCount) -> Self {
        Self::TimingCount(timing_count)
    }
}

impl From<Gauge> for Instrument {
    fn from(gauge: Gauge) -> Self {
        Self::Gauge(gauge)
    }
}

#[derive(Clone, Debug)]
pub enum Instrument {
    Count(Count),
    Gauge(Gauge),
    TimingCount(TimingCount),
}

impl Instrument {
    pub(crate) fn count() -> Count {
        Count::default()
    }
    pub(crate) fn gauge() -> Gauge {
        Gauge::default()
    }
    pub(crate) fn timing_count() -> TimingCount {
        TimingCount::default()
    }
    pub(crate) fn emit(
        &self,
        client: &GnortClient,
        metric_key: &MetricKey,
    ) -> Result<(), DogstatsdError> {
        let name = metric_key.get_name();
        match self {
            Instrument::Count(count) => {
                // Reset the count and get the final value before emitting
                let metric_value = count.reset();
                client.count(name, metric_value as i64, metric_key.get_tags())
            }
            Instrument::Gauge(gauge) => {
                let val_str = gauge.load().to_string();
                client.gauge(name, &val_str, metric_key.get_tags())
            }
            Instrument::TimingCount(timing_count) => {
                let (sum, count) = timing_count.reset();
                let sum_name = format!("{}.time", name);
                let count_name = name;
                let tags = metric_key.get_tags();
                client.count(sum_name, sum as i64, tags)?;
                client.count(count_name, count as i64, tags)
            }
        }
    }
    pub fn downcast<T: MetricType::Impl + MakeInstrument>(
        &self,
    ) -> Result<<T as MakeInstrument>::InstrumentType, MetricRegistrationError>
    where
        <T as MakeInstrument>::InstrumentType: Into<Instrument> + Clone + 'static,
    {
        match self {
            Instrument::Count(count) => {
                let any_count = Box::new(count.clone()) as Box<dyn core::any::Any>;
                let downcasted: Result<
                    Box<<T as MakeInstrument>::InstrumentType>,
                    Box<dyn core::any::Any + 'static>,
                > = any_count.downcast();
                match downcasted {
                    Ok(downcasted) => Ok(*downcasted),
                    Err(_) => Err(MetricRegistrationError::TypeMismatch(
                        MetricType::Count::name(),
                        Instrument::Count(count.to_owned()),
                    )),
                }
            }
            Instrument::Gauge(gauge) => {
                let any_gauge = Box::new(gauge.clone()) as Box<dyn core::any::Any>;
                let downcasted: Result<
                    Box<<T as MakeInstrument>::InstrumentType>,
                    Box<dyn core::any::Any + 'static>,
                > = any_gauge.downcast();
                match downcasted {
                    Ok(downcasted) => Ok(*downcasted),
                    Err(_) => Err(MetricRegistrationError::TypeMismatch(
                        MetricType::Gauge::name(),
                        Instrument::Gauge(gauge.to_owned()),
                    )),
                }
            }
            Instrument::TimingCount(timing_count) => {
                let any_timing_count = Box::new(timing_count.clone()) as Box<dyn core::any::Any>;
                let downcasted: Result<
                    Box<<T as MakeInstrument>::InstrumentType>,
                    Box<dyn core::any::Any + 'static>,
                > = any_timing_count.downcast();
                match downcasted {
                    Ok(downcasted) => Ok(*downcasted),
                    Err(_) => Err(MetricRegistrationError::TypeMismatch(
                        MetricType::TimingCount::name(),
                        Instrument::TimingCount(timing_count.to_owned()),
                    )),
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use crate::MetricType;

    use super::*;

    #[test]
    fn test_count() {
        let make = || Instrument::Count(Count::default());
        // Matching type should succeed
        let downcasted = make().downcast::<MetricType::Count>();
        assert!(downcasted.is_ok());
        // Non-matching types should fail
        let downcasted = make().downcast::<MetricType::Gauge>();
        assert!(downcasted.is_err());
        let downcasted = make().downcast::<MetricType::TimingCount>();
        assert!(downcasted.is_err());
    }
    #[test]
    fn test_gauge() {
        let make = || Instrument::Gauge(Gauge::default());
        // Matching type should succeed
        let downcasted = make().downcast::<MetricType::Gauge>();
        assert!(downcasted.is_ok());
        // Non-matching types should fail
        let downcasted = make().downcast::<MetricType::Count>();
        assert!(downcasted.is_err());
        let downcasted = make().downcast::<MetricType::TimingCount>();
        assert!(downcasted.is_err());
    }
    #[test]
    fn test_timing_count() {
        let make = || Instrument::TimingCount(TimingCount::default());
        // Matching type should succeed
        let downcasted = make().downcast::<MetricType::TimingCount>();
        assert!(downcasted.is_ok());
        // Non-matching types should fail
        let downcasted = make().downcast::<MetricType::Gauge>();
        assert!(downcasted.is_err());
        let downcasted = make().downcast::<MetricType::Count>();
        assert!(downcasted.is_err());
    }

    #[test]
    fn test_measure_fn() {
        let timing_count = TimingCount::default().with_unit(UnitOfTime::Millis);
        let _result = timing_count.measure_sync_fn(|| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            5
        });
        let (sum, count) = timing_count.reset();
        assert!(sum.abs_diff(100) < 10);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_measure_fn_micros() {
        let time = 100;
        let timing_count = TimingCount::default().with_unit(UnitOfTime::Micros);
        let _result = timing_count.measure_sync_fn(|| {
            std::thread::sleep(std::time::Duration::from_micros(time));
        });
        let (sum, count) = timing_count.reset();
        assert!(sum >= 100);
        assert_eq!(count, 1);
    }

    #[test]
    fn test_regular_counts() {
        let example_count = Count::default();
        let prev_count = example_count.fetch_add(1);
        println!("prev_count: {}", prev_count);
        assert_eq!(prev_count, 0);
        let prev_count = example_count.fetch_add(0);
        println!("prev_count: {}", prev_count);
        assert_eq!(prev_count, 1);
        let prev_count = example_count.fetch_add(1);
        println!("prev_count: {}", prev_count);
        assert_eq!(prev_count, 1);
        let prev_count = example_count.fetch_add(0);
        println!("prev_count: {}", prev_count);
        assert_eq!(prev_count, 2);
    }

    #[test]
    fn test_timing_counts() {
        let time = 100;
        let timing_count = TimingCount::default().with_unit(UnitOfTime::Micros);
        let duration = std::time::Duration::from_micros(time);
        let (sum, count) = timing_count.add_timing(&duration);
        println!("sum: {}, count: {}", sum, count);
        assert_eq!(sum, 0);
        assert_eq!(count, 0);
        let zero = std::time::Duration::from_micros(0);
        let (sum, count) = timing_count.add_timing(&zero);
        println!("sum: {}, count: {}", sum, count);
        assert_eq!(sum, 100);
        assert_eq!(count, 1);
        let (sum, count) = timing_count.add_timing(&zero);
        println!("sum: {}, count: {}", sum, count);
        assert_eq!(sum, 100);
        assert_eq!(count, 2);
        let (sum, count) = timing_count.add_timing(&zero);
        println!("sum: {}, count: {}", sum, count);
        assert_eq!(sum, 100);
        assert_eq!(count, 3);
    }
}
