use std::{
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use governor::{
    clock::{Clock, DefaultClock},
    DefaultDirectRateLimiter, Quota, RateLimiter,
};
use nonzero_ext::nonzero;
use thiserror::Error;
use tracing::{debug, trace};

use crate::{
    client::{sync_client, GnortClient},
    instrument::{Count, Gauge, Instrument, TimingCount},
    MakeInstrument, Metric, MetricKey, MetricType,
};
use once_cell::sync::OnceCell;

/// INSTANCE is a singleton Kafka FutureProducer. This producer
/// is configured for non-blocking use so there's a high
/// linger time set.
static GLOBAL_BUCKET: OnceCell<MetricsRegistry> = OnceCell::new();
static TIME_TO_EMIT_METRICS: &str = "gnort.aggregate.time_to_emit_metrics.gauge";

pub fn global_metrics_registry() -> &'static MetricsRegistry {
    GLOBAL_BUCKET.get_or_init(|| MetricsRegistry::new(Default::default()))
}

type MetricsMap = Arc<DashMap<MetricKey, Instrument>>;
fn new_metric_map() -> MetricsMap {
    Arc::new(DashMap::new())
}

#[derive(Clone)]
/// Collection of metrics keyed by stat name and tags.
/// MetricsRegistry has its own thread that emits metrics to Datadog.
pub struct MetricsRegistry {
    /// Concurrent HashMap (DashMap) of metrics keyed to their associated instruments.
    // TODO: We need to benchmark/profile interning metric (stat) names and tag keys
    pub(crate) metrics: MetricsMap,
    /// client is optional because the registry can fallback to the global registry.
    /// This could impact default tags are used.
    client: Option<GnortClient>,
    // How often should the metric be emitted? Default is 10 seconds
    // Skipping this for now in lieu of a standard aggregation time.
    observation_period: Option<Duration>,
    // What delay should the metrical client use before emitting the first observation?
    delay_time: Option<Duration>,
    // Rate limiter
    rate_limiter: Arc<governor::DefaultDirectRateLimiter>,
}

// Client-side aggregation followed by agent aggregation may result in some undesirable effects like
//      * having no data points submitted during agent's aggregation window (that'd appear as gaps in time series)
//      * or having data points from multiple agent's aggregation windows submitted in a single aggregation window
//      * (that'd make e.g. count metric values from consecutive agg windows differ by ~100%).
//      * To avoid that, we're using a default observation period of 3 seconds for all metrics.
const DEFAULT_OBSERVATION_PERIOD_MILLIS: u64 = 3_000;
const OBSERVATION_PERIOD_MILLIS_ENV_VAR: &str = "GNORT_OBSERVATION_PERIOD_MILLIS";
const DEFAULT_DELAY_MILLIS: u64 = 3_000;
const DELAY_MILLIS_ENV_VAR: &str = "GNORT_DELAY_MILLIS";
const DEFAULT_RATE_LIMIT_PER_SECOND: NonZeroU32 = nonzero!(42_000u32);
const DEFAULT_BURST_LIMIT: NonZeroU32 = nonzero!(42u32);

#[derive(Clone, Default)]
pub struct RegistryConfig {
    pub client: Option<GnortClient>,
    pub observation_period: Option<Duration>,
    pub delay_time: Option<Duration>,
    pub rate_limit_per_second: Option<NonZeroU32>,
    pub burst_limit: Option<NonZeroU32>,
}

impl RegistryConfig {
    pub fn with_client(mut self, client: GnortClient) -> Self {
        self.client = Some(client);
        self
    }
}

fn get_env_or_fallback(env_var: &str, fallback: u64) -> u64 {
    match std::env::var(env_var) {
        Err(_) => {
            debug!("Falling back to delay time default, {env_var} wasn't specified and delay wasn't set in the code.");
            fallback
        }
        Ok(delay_millis) => delay_millis.parse::<u64>().unwrap_or_else(|_| {
            debug!("Couldn't parse {env_var} as a u64, falling back to default.");
            fallback
        }),
    }
}

#[derive(Debug, Error)]
pub enum MetricRegistrationError {
    #[error("Metric type mismatch, expected: {0:?}, was: {1:?}")]
    TypeMismatch(String, Instrument),
}

impl MetricsRegistry {
    pub fn new(registry_config: RegistryConfig) -> Self {
        let metrics = new_metric_map();
        let rate_limit_per_second = registry_config
            .rate_limit_per_second
            .unwrap_or(DEFAULT_RATE_LIMIT_PER_SECOND);
        let burst_limit = registry_config.burst_limit.unwrap_or(DEFAULT_BURST_LIMIT);
        let rate_limiter = Arc::new(RateLimiter::direct(
            Quota::per_second(rate_limit_per_second).allow_burst(burst_limit),
        ));
        let registry = Self {
            metrics,
            rate_limiter,
            client: registry_config.client,
            observation_period: registry_config.observation_period,
            delay_time: registry_config.delay_time,
        };
        registry.start();
        registry
    }
    fn get_client(&self) -> &GnortClient {
        self.client.as_ref().unwrap_or_else(|| sync_client())
    }
    fn get_delay(&self) -> std::time::Duration {
        self.delay_time.unwrap_or_else(|| {
            let delay_millis = get_env_or_fallback(DELAY_MILLIS_ENV_VAR, DEFAULT_DELAY_MILLIS);
            std::time::Duration::from_millis(delay_millis)
        })
    }
    fn get_observation_period(&self) -> std::time::Duration {
        self.observation_period.unwrap_or_else(|| {
            let observation_millis = get_env_or_fallback(
                OBSERVATION_PERIOD_MILLIS_ENV_VAR,
                DEFAULT_OBSERVATION_PERIOD_MILLIS,
            );
            std::time::Duration::from_millis(observation_millis)
        })
    }
    /// We have to clone the bucket and let the user keep `self` so
    /// that they can register new metrics
    fn start(&self) -> std::thread::JoinHandle<()> {
        let delay_duration = self.get_delay();
        let wait_duration = self.get_observation_period();
        let self_clone = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(delay_duration);
            loop {
                let start = Instant::now();
                let client = self_clone.get_client();
                self_clone.reset_and_emit(client);
                let runtime = start.elapsed();
                if let Some(remaining) = wait_duration.checked_sub(runtime) {
                    std::thread::sleep(remaining);
                }
            }
        })
    }
    /// [register_metric]() has get_or_insert semantics.
    pub fn register_metric<M, T: MetricType::Impl + MakeInstrument>(
        &self,
        metric: M,
    ) -> Result<<T as MakeInstrument>::InstrumentType, MetricRegistrationError>
    where
        M: Into<Metric<T>>,
        <T as MakeInstrument>::InstrumentType: Into<Instrument> + Clone + 'static,
    {
        let metric: Metric<T> = metric.into();
        let instrument = metric.make_instrument();
        let metric_key: MetricKey = metric.into();
        let entry = self.metrics.entry(metric_key);
        match entry {
            dashmap::mapref::entry::Entry::Occupied(ref occupied) => {
                let instrument_enum = occupied.get().to_owned();
                instrument_enum.downcast::<T>()
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                let instrument_enum: Instrument = instrument.clone().into();
                vacant.insert(instrument_enum);
                Ok(instrument)
            }
        }
    }
    /// [register_count]() has get_or_insert semantics.
    pub fn register_count<M>(&self, metric: M) -> Result<Count, MetricRegistrationError>
    where
        M: Into<Metric<MetricType::Count>>,
    {
        self.register_metric(metric)
    }
    /// [register_gauge]() has get_or_insert semantics.
    pub fn register_gauge<M>(&self, metric: M) -> Result<Gauge, MetricRegistrationError>
    where
        M: Into<Metric<MetricType::Gauge>>,
    {
        self.register_metric(metric)
    }
    /// [register_timing_count]() has get_or_insert semantics.
    pub fn register_timing_count<M>(
        &self,
        metric: M,
    ) -> Result<TimingCount, MetricRegistrationError>
    where
        M: Into<Metric<MetricType::TimingCount>>,
    {
        self.register_metric(metric)
    }
    pub(crate) fn reset_and_emit(&self, client: &GnortClient) {
        let clock = DefaultClock::default();
        let before_emit = Instant::now();
        for ref_multi in self.metrics.iter() {
            let (metric, instrument) = ref_multi.pair();
            check_and_wait(&clock, &self.rate_limiter, true);
            let _ = instrument
                .emit(client, metric)
                .map_err(|err| debug!("Got error emitting Datadog metric, was: {err}"));
        }
        let after_emit = Instant::now();
        let emission_micros = after_emit.duration_since(before_emit).as_micros();
        let tags: &[&str] = &[];
        let _ = client
            .gauge(
                TIME_TO_EMIT_METRICS,
                (emission_micros as i64).to_string(),
                tags,
            )
            .map_err(|err| debug!("Got error emitting Datadog metric, was: {err}"));
    }
}

fn check_and_sleep(
    clock: &DefaultClock,
    rate_limiter: &DefaultDirectRateLimiter,
    sleep: bool,
) -> bool {
    let governed = rate_limiter.check();
    match governed {
        Ok(_) => {
            // Go ahead
            true
        }
        Err(negative) => {
            trace!(
                "Forced to wait, negative was: {negative:?}",
                negative = negative
            );
            if sleep {
                let wait_time = negative.wait_time_from(clock.now()).as_millis();
                std::thread::sleep(Duration::from_millis(wait_time as u64));
            }
            // Try again
            false
        }
    }
}

fn check_and_wait(clock: &DefaultClock, rate_limiter: &DefaultDirectRateLimiter, sleep: bool) {
    loop {
        let go_ahead = check_and_sleep(clock, rate_limiter, sleep);
        if go_ahead {
            break;
        } else {
            continue;
        }
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use approx::*;
    use governor::{
        clock::{self, Clock, FakeRelativeClock, QuantaInstant, Reference},
        middleware,
        nanos::Nanos,
        state, Quota, RateLimiter,
    };

    #[test]
    fn test_approx() {
        assert!(!relative_eq!(1.0f64, 0.8f64, max_relative = 0.1));
        assert!(relative_eq!(1.0f64, 0.9f64, max_relative = 0.1));
        assert!(relative_eq!(1.0f64, 1.1f64, max_relative = 0.1));
        assert!(!relative_eq!(1.0f64, 1.2f64, max_relative = 0.1));
    }
    type Counter = Arc<AtomicUsize>;

    /// A rate limiter representing a single item of state in memory, running on the default clock.
    ///
    /// See the [`RateLimiter`] documentation for details.
    pub type TestDirectRateLimiter<
        MW = middleware::NoOpMiddleware<<clock::FakeRelativeClock as clock::Clock>::Instant>,
    > = RateLimiter<state::direct::NotKeyed, state::InMemoryState, clock::FakeRelativeClock, MW>;

    fn slam_rate_limiter(
        counter: Counter,
        clock: FakeRelativeClock,
        rl: Arc<TestDirectRateLimiter>,
        start: Nanos,
        time_limit: Nanos,
    ) {
        loop {
            if clock.now().duration_since(start) > time_limit {
                break;
            }
            let governed = rl.check();
            match governed {
                Ok(_) => {
                    let _ = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                Err(negative) => {
                    let wait_time = negative.wait_time_from(clock.now()).as_millis();
                    std::thread::sleep(Duration::from_millis(wait_time as u64));
                }
            }
            clock.advance(std::time::Duration::from_nanos(1_000));
        }
    }

    fn slam_rate_limiter_real(
        counter: Counter,
        clock: DefaultClock,
        rl: &DefaultDirectRateLimiter,
        start: QuantaInstant,
        time_limit: Nanos,
        sleep: bool,
    ) {
        loop {
            if clock.now().duration_since(start) > time_limit {
                break;
            }
            check_and_wait(&clock, &rl, sleep);
            let _ = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn test_rate_limiter() -> (Arc<TestDirectRateLimiter>, FakeRelativeClock) {
        let clock = governor::clock::FakeRelativeClock::default();
        let q = Quota::per_second(DEFAULT_RATE_LIMIT_PER_SECOND).allow_burst(DEFAULT_BURST_LIMIT);
        let rl = RateLimiter::direct_with_clock(q, clock.clone());
        (Arc::new(rl), clock)
    }

    fn test_rate_limiter_real() -> Arc<DefaultDirectRateLimiter> {
        let q = Quota::per_second(DEFAULT_RATE_LIMIT_PER_SECOND).allow_burst(DEFAULT_BURST_LIMIT);
        let rl = RateLimiter::direct(q);
        Arc::new(rl)
    }

    // Make sure you're building this in release mode or it'll be too slow
    // I was able to hit 42k in release mode w/ a DefaultClock on an M1 Macbook Pro.
    // Performance of this test was too inconsistent in CI/CD to be useful.
    // #[test]
    // fn test_governor_threaded_real() {
    //     let rl = test_rate_limiter_real();
    //     let counter = Arc::new(AtomicUsize::new(0));
    //     let time_limit = Duration::from_secs(1).into();
    //     let clock = DefaultClock::default();
    //     let start = clock.now();
    //     let mut handles = vec![];
    //     // The results get worse with increased contention
    //     for _ in 0..5 {
    //         let cloned_counter = counter.clone();
    //         let cloned_rl = rl.clone();
    //         let cloned_clock = clock.clone();
    //         let join_handle = std::thread::spawn(move || {
    //             slam_rate_limiter_real(cloned_counter, cloned_clock, &cloned_rl, start, time_limit, false);
    //         });
    //         handles.push(join_handle);
    //     }
    //     for handle in handles {
    //         handle.join().unwrap();
    //     }
    //     let rate_limit = DEFAULT_RATE_LIMIT_PER_SECOND.get() as f64;
    //     let final_count = counter.load(std::sync::atomic::Ordering::Relaxed);
    //     println!("Final count was: {}", final_count);
    //     assert!(relative_eq!(final_count as f64, rate_limit, max_relative = 0.01));
    // }

    // Release mode y'all.
    #[test]
    fn test_governor_real() {
        let rl = test_rate_limiter_real();
        let counter = Arc::new(AtomicUsize::new(0));
        let time_limit = Duration::from_secs(1).into();
        let clock = DefaultClock::default();
        let start = clock.now();
        slam_rate_limiter_real(counter.clone(), clock, &rl, start, time_limit, true);
        let rate_limit = DEFAULT_RATE_LIMIT_PER_SECOND.get() as f64;
        let final_count = counter.load(std::sync::atomic::Ordering::Relaxed);
        println!("Final count was: {}", final_count);
        assert!(relative_eq!(
            final_count as f64,
            rate_limit,
            max_relative = 0.10
        ));
    }

    #[test]
    fn test_governor() {
        let (rl, clock) = test_rate_limiter();
        let counter = Arc::new(AtomicUsize::new(0));
        let start = FakeRelativeClock::default().now();
        let time_limit = Duration::from_secs(1).into();
        slam_rate_limiter(counter.clone(), clock, rl, start, time_limit);
        // assert_eq!(counter, DEFAULT_RATE_LIMIT_PER_SECOND.get() as usize);
        let rate_limit = DEFAULT_RATE_LIMIT_PER_SECOND.get() as f64;
        let final_count = counter.load(std::sync::atomic::Ordering::Relaxed);
        println!("Final count was: {}", final_count);
        assert!(relative_eq!(
            final_count as f64,
            rate_limit,
            max_relative = 0.01
        ));
    }

    #[test]
    fn test_governor_threaded() {
        let (rl, clock) = test_rate_limiter();
        let counter = Arc::new(AtomicUsize::new(0));
        let start = FakeRelativeClock::default().now();
        let time_limit = Duration::from_secs(1).into();
        let mut handles = vec![];
        for _ in 0..10 {
            let cloned_counter = counter.clone();
            let cloned_rl = rl.clone();
            let cloned_clock = clock.clone();
            let join_handle = std::thread::spawn(move || {
                slam_rate_limiter(cloned_counter, cloned_clock, cloned_rl, start, time_limit);
            });
            handles.push(join_handle);
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let rate_limit = DEFAULT_RATE_LIMIT_PER_SECOND.get() as f64;
        let final_count = counter.load(std::sync::atomic::Ordering::Relaxed);
        println!("Final count was: {}", final_count);
        assert!(relative_eq!(
            final_count as f64,
            rate_limit,
            max_relative = 0.01
        ));
    }
}
