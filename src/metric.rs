use std::{collections::BTreeSet, marker::PhantomData};

use dogstatsd::DogstatsdResult;
use maplit::btreeset;

use crate::{
    instrument::{Count, Gauge, Instrument, TimingCount},
    GnortClient,
};

#[allow(non_snake_case)]
pub mod MetricType {
    /// Implemented exclusively by members of this phantom type enum
    /// This is for use as a trait bound
    pub trait Impl {
        fn name() -> String;
    }
    /// Count
    #[derive(Copy, Clone)]
    pub enum Count {}
    impl Impl for Count {
        fn name() -> String {
            "count".to_string()
        }
    }

    /// Gauge
    #[derive(Copy, Clone)]
    pub enum Gauge {}
    impl Impl for Gauge {
        fn name() -> String {
            "gauge".to_string()
        }
    }

    /// TimingCount
    #[derive(Copy, Clone)]
    pub enum TimingCount {}
    impl Impl for TimingCount {
        fn name() -> String {
            "timing_count".to_string()
        }
    }
}

#[derive(Clone)]
pub struct MetricName<'a, T: MetricType::Impl>(&'a str, PhantomData<T>);
impl<'a, T: MetricType::Impl> MetricName<'a, T> {
    pub const fn new(name: &'a str) -> Self {
        Self(name, PhantomData)
    }
    pub fn get_name(&self) -> &'a str {
        self.0
    }
}

impl<'a> MetricName<'a, MetricType::Count> {
    pub const fn count(name: &'a str) -> Self {
        Self(name, PhantomData)
    }
}
impl<'a> MetricName<'a, MetricType::Gauge> {
    pub const fn gauge(name: &'a str) -> Self {
        Self(name, PhantomData)
    }
}
impl<'a> MetricName<'a, MetricType::TimingCount> {
    pub const fn timing_count(name: &'a str) -> Self {
        Self(name, PhantomData)
    }
    // This is an alias so the generic paste macro doesn't have to get weird.
    // pub const fn timingcount(name: &'a str) -> Self {
    //     Self::timing_count(name)
    // }
}

impl From<MetricName<'static, MetricType::Count>> for Metric<MetricType::Count> {
    fn from(m: MetricName<'static, MetricType::Count>) -> Metric<MetricType::Count> {
        Metric::new_count(m)
    }
}

impl<T: MetricType::Impl> From<MetricName<'static, T>> for &'static str {
    fn from(m: MetricName<'static, T>) -> Self {
        m.0
    }
}

impl From<MetricName<'static, MetricType::Gauge>> for Metric<MetricType::Gauge> {
    fn from(m: MetricName<'static, MetricType::Gauge>) -> Metric<MetricType::Gauge> {
        Metric::new_gauge(m)
    }
}

impl From<MetricName<'static, MetricType::TimingCount>> for Metric<MetricType::TimingCount> {
    fn from(m: MetricName<'static, MetricType::TimingCount>) -> Metric<MetricType::TimingCount> {
        Metric::new_timing_count(m)
    }
}

// TODO: Histogram/Distribution is needed before we can do non-count timings
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub struct Metric<T: MetricType::Impl> {
    /// Name of the metric, called stat in dogstatsd
    metric_name: &'static str,
    /// What kind of metric is it?
    metric_type: PhantomData<T>,
    /// Tags for the metric
    metric_tags: BTreeSet<String>,
    // metric_tags: &'static [&'static str],
}

impl From<&'static str> for Metric<MetricType::Count> {
    // Default metrics derived from bare names to counts
    fn from(metric_name: &'static str) -> Metric<MetricType::Count> {
        Metric::new_count(MetricName::count(metric_name))
    }
}

impl From<&'static str> for Metric<MetricType::Gauge> {
    // Default metrics derived from bare names to counts
    fn from(metric_name: &'static str) -> Metric<MetricType::Gauge> {
        Metric::new_gauge(MetricName::gauge(metric_name))
    }
}

impl From<&'static str> for Metric<MetricType::TimingCount> {
    // Default metrics derived from bare names to counts
    fn from(metric_name: &'static str) -> Metric<MetricType::TimingCount> {
        Metric::new_timing_count(MetricName::timing_count(metric_name))
    }
}

impl Metric<MetricType::Count> {
    pub fn new_count(metric_name: MetricName<'static, MetricType::Count>) -> Self {
        Self {
            metric_name: metric_name.into(),
            // metric_tags: &[],
            metric_tags: btreeset![],
            metric_type: PhantomData,
        }
    }
    pub fn adhoc_count(
        &self,
        client: &GnortClient,
        count: i64,
        adhoc_tags: BTreeSet<String>,
    ) -> DogstatsdResult {
        let emission_tags = self.metric_tags.union(&adhoc_tags);
        client.count(self.metric_name, count, emission_tags)
    }
}

impl Metric<MetricType::Gauge> {
    pub fn new_gauge(metric_name: MetricName<'static, MetricType::Gauge>) -> Self {
        Self {
            metric_name: metric_name.into(),
            metric_tags: btreeset![],
            metric_type: PhantomData,
        }
    }
    pub fn adhoc_gauge(
        &self,
        client: &GnortClient,
        value: f64,
        adhoc_tags: BTreeSet<String>,
    ) -> DogstatsdResult {
        let emission_tags = self.metric_tags.union(&adhoc_tags);
        client.gauge(self.metric_name, value.to_string(), emission_tags)
    }
}

impl Metric<MetricType::TimingCount> {
    pub fn new_timing_count(metric_name: MetricName<'static, MetricType::TimingCount>) -> Self {
        Self {
            metric_name: metric_name.into(),
            metric_tags: btreeset![],
            metric_type: PhantomData,
        }
    }
    pub fn adhoc_timing_count(
        &self,
        client: &GnortClient,
        sum: i64,
        count: i64,
        adhoc_tags: BTreeSet<String>,
    ) -> DogstatsdResult {
        let emission_tags = self.metric_tags.union(&adhoc_tags);
        client.count(self.metric_name, sum, emission_tags.clone())?;
        client.count(self.metric_name, count, emission_tags)
    }
}

#[allow(dead_code)]
impl<T: MetricType::Impl + MakeInstrument> Metric<T> {
    pub fn make_instrument(&self) -> T::InstrumentType {
        <T as MakeInstrument>::make_instrument()
    }
    pub fn with_tags<I, S>(self, metric_tags: I) -> Self
    where
        S: AsRef<str>,
        I: IntoIterator<Item = S>,
    {
        let metric_tags: BTreeSet<String> = metric_tags
            .into_iter()
            .map(|t| t.as_ref().to_string())
            .collect();
        Self {
            metric_tags,
            ..self
        }
    }
    pub fn with_array_tags<S, const N: usize>(self, metric_tags: [S; N]) -> Self
    where
        S: AsRef<str> + Into<String>,
    {
        let metric_tags: BTreeSet<String> = metric_tags.into_iter().map(|t| t.into()).collect();
        Self {
            metric_tags,
            ..self
        }
    }
    pub fn with_vec_tags(self, metric_tags: Vec<String>) -> Self {
        let metric_tags: BTreeSet<String> = metric_tags.into_iter().map(|t| t).collect();
        Self {
            metric_tags,
            ..self
        }
    }
    pub fn with_set_tags(self, metric_tags: BTreeSet<String>) -> Self {
        Self {
            metric_tags,
            ..self
        }
    }
    pub fn get_name(&self) -> &'static str {
        self.metric_name
    }
    pub fn get_tags(&self) -> &BTreeSet<String> {
        &self.metric_tags
    }
}

pub trait MakeInstrument {
    type InstrumentType;
    fn make_instrument() -> Self::InstrumentType;
}

impl MakeInstrument for MetricType::Count {
    type InstrumentType = Count;
    fn make_instrument() -> Self::InstrumentType {
        Instrument::count()
    }
}

impl MakeInstrument for MetricType::Gauge {
    type InstrumentType = Gauge;
    fn make_instrument() -> Self::InstrumentType {
        Instrument::gauge()
    }
}

impl MakeInstrument for MetricType::TimingCount {
    type InstrumentType = TimingCount;
    fn make_instrument() -> Self::InstrumentType {
        Instrument::timing_count()
    }
}

/// Type-erased Metric type for the metric map
#[derive(Clone, Debug, Default, Hash, PartialEq, Eq)]
pub(crate) struct MetricKey {
    /// Name of the metric, called stat in dogstatsd
    metric_name: &'static str,
    /// Tags for the metric
    metric_tags: BTreeSet<String>,
}

impl MetricKey {
    pub fn new(metric_name: &'static str, metric_tags: BTreeSet<String>) -> Self {
        Self {
            metric_name,
            metric_tags,
        }
    }
    pub fn get_name(&self) -> &'static str {
        self.metric_name
    }
    pub fn get_tags(&self) -> &BTreeSet<String> {
        &self.metric_tags
    }
}

impl<T: MetricType::Impl> From<Metric<T>> for MetricKey {
    fn from(m: Metric<T>) -> MetricKey {
        MetricKey::new(m.metric_name, m.metric_tags)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        adhoc_metrics_struct, metric, metrics_module, metrics_struct, MetricRegistrationError,
        MetricsRegistry, RegistryConfig,
    };

    metric!(TEST_COUNT_METRIC, "gnort.test.bench.incr", Count);
    metric!(TEST_GAUGE_METRIC, "gnort.test.bench.gauge", Gauge);
    metric!(
        TEST_TIMING_COUNT_METRIC,
        "gnort.test.bench.timing_count",
        TimingCount
    );

    metrics_struct![
        TaglessMetrics,
        (test_count, "gnort.test.bench.count", Count),
        // You can have multiple metrics with the same name as long as the tags are different.
        (_test_count_success, "gnort.test.commit.count", Count),
        (test_gauge, "gnort.test.bench.gauge", Gauge),
        (
            test_timing_count,
            "gnort.test.bench.timing_count",
            TimingCount
        )
    ];

    // Should be able to mix and match tags and no tags.
    metrics_struct![
        TestMetrics,
        (test_count, "gnort.test.bench.count", Count),
        // You can have multiple metrics with the same name as long as the tags are different.
        (
            _test_count_success,
            "gnort.test.commit.count",
            Count,
            ["outcome:success"]
        ),
        (
            _test_count_failure,
            "gnort.test.commit.count",
            Count,
            ["outcome:failure"]
        ),
        (test_gauge, "gnort.test.bench.gauge", Gauge),
        (
            test_timing_count,
            "gnort.test.bench.timing_count",
            TimingCount
        )
    ];

    #[test]
    fn test_metrics_macros() {
        let registry = MetricsRegistry::new(RegistryConfig::default());
        let test_metrics = TestMetrics::register(&registry).expect("Failed to register metrics!");
        assert_eq!(TEST_COUNT_METRIC.get_name(), "gnort.test.bench.incr");
        assert_eq!(TEST_GAUGE_METRIC.get_name(), "gnort.test.bench.gauge");
        assert_eq!(
            TEST_TIMING_COUNT_METRIC.get_name(),
            "gnort.test.bench.timing_count"
        );
        assert_eq!(test_metrics.test_count.increment(), 0);
        assert_eq!(test_metrics.test_count.increment(), 1);
        assert_eq!(test_metrics.test_gauge.swap(5.5), 0.0);
        assert_eq!(test_metrics.test_gauge.swap(6.5), 5.5);
        assert_eq!(
            test_metrics
                .test_timing_count
                .add_timing(&std::time::Duration::from_secs(5)),
            (0, 0)
        );
        // microseconds is the default unit of time
        assert_eq!(
            test_metrics
                .test_timing_count
                .add_timing(&std::time::Duration::from_secs(10)),
            (5_000, 1)
        );
    }

    #[test]
    fn test_tagless_metrics() {
        let registry = MetricsRegistry::new(RegistryConfig::default());
        let test_metrics =
            TaglessMetrics::register(&registry).expect("Failed to register metrics!");
        assert_eq!(test_metrics.test_count.increment(), 0);
        assert_eq!(test_metrics.test_count.increment(), 1);
        assert_eq!(test_metrics.test_gauge.swap(5.5), 0.0);
        assert_eq!(test_metrics.test_gauge.swap(6.5), 5.5);
        assert_eq!(
            test_metrics
                .test_timing_count
                .add_timing(&std::time::Duration::from_secs(5)),
            (0, 0)
        );
        assert_eq!(
            test_metrics
                .test_timing_count
                .add_timing(&std::time::Duration::from_secs(10)),
            (5_000, 1)
        );
    }

    metrics_module![
        test_module,
        (test_count, "gnort.test.bench.count", Count),
        // You can have multiple metrics with the same name as long as the tags are different.
        (_test_count_success, "gnort.test.commit.count", Count),
        (test_gauge, "gnort.test.bench.gauge", Gauge),
        (
            test_timing_count,
            "gnort.test.bench.timing_count",
            TimingCount
        )
    ];

    #[test]
    fn test_module_metrics() {
        let registry = MetricsRegistry::new(RegistryConfig::default());
        let test_instruments =
            test_module::Instruments::register(&registry).expect("Failed to register metrics!");
        assert_eq!(test_instruments.test_count.increment(), 0);
        assert_eq!(test_instruments.test_gauge.swap(5.5), 0.0);
        assert_eq!(
            test_instruments
                .test_timing_count
                .add_timing(&std::time::Duration::from_secs(5)),
            (0, 0)
        );

        let registry = MetricsRegistry::new(RegistryConfig::default());
        let mut test_metrics = test_module::Metrics::new();
        test_metrics.test_count = test_metrics.test_count.with_tags(vec!["outcome:cornholio"]);
        let test_instruments = test_metrics.register(&registry).unwrap();
        assert_eq!(test_instruments.test_count.increment(), 0);
        assert_eq!(test_instruments.test_gauge.swap(5.5), 0.0);
        assert_eq!(
            test_instruments
                .test_timing_count
                .add_timing(&std::time::Duration::from_secs(5)),
            (0, 0)
        );
    }

    adhoc_metrics_struct![
        TestAdhocMetrics,
        (test_count, "gnort.test.bench.count", Count),
        // You can have multiple metrics with the same name as long as the tags are different.
        (
            test_count_success,
            "gnort.test.commit.count",
            Count,
            ["outcome:success"]
        ),
        (
            test_count_failure,
            "gnort.test.commit.count",
            Count,
            ["outcome:failure"]
        )
    ];

    #[test]
    fn test_metrics_name_macro() {
        // Don't need to register, you'd usually use this with the ad-hoc client
        let test_name_metrics = TestAdhocMetrics::new();
        assert_eq!(
            test_name_metrics.test_count.get_name(),
            "gnort.test.bench.count"
        );
        assert_eq!(
            test_name_metrics.test_count_success.get_name(),
            "gnort.test.commit.count"
        );
        assert_eq!(
            test_name_metrics.test_count_failure.get_name(),
            "gnort.test.commit.count"
        );
    }
}
