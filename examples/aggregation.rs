use gnort::*;

// pub const EXAMPLE_COUNT_METRIC: MetricName<MetricType::Count> = MetricName::count("gnort.test.bench.incr");
metric!(EXAMPLE_COUNT_METRIC, "gnort.test.bench.incr", Count);
// pub const EXAMPLE_GAUGE_METRIC: MetricName<MetricType::Gauge> = MetricName::gauge("gnort.test.bench.gauge");
metric!(EXAMPLE_GAUGE_METRIC, "gnort.test.bench.gauge", Gauge);
// pub const EXAMPLE_TIMING_COUNT_METRIC: MetricName<MetricType::TimingCount> = MetricName::timing_count("gnort.test.bench.timing_count");
metric!(
    EXAMPLE_TIMING_COUNT_METRIC,
    "gnort.test.bench.timing_count",
    TimingCount
);

metrics_struct![
    ExampleMetrics,
    (example_count, "gnort.test.bench.count", Count),
    (example_gauge, "gnort.test.bench.gauge", Gauge),
    (
        example_timing_count,
        "gnort.test.bench.timing_count",
        TimingCount
    )
];

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
pub async fn main() {
    // Override for local testing
    std::env::set_var(gnort::client::STATSD_HOST_ENV, "0.0.0.0");
    let client = GnortClient::default().expect("Failed to instantiate client!");
    let registry = MetricsRegistry::new(RegistryConfig::default().with_client(client));
    // Count metric
    // Short-hand just using the MetricName
    let count_instrument = registry
        .register_count(EXAMPLE_COUNT_METRIC)
        .expect("Failed to register count metric!");

    // Gauge
    // Explicitly creating the metric value before-hand
    let gauge_metric =
        Metric::new_gauge(EXAMPLE_GAUGE_METRIC).with_tags(&["test_tag2:test_tag_value2"]);
    let my_gauge = registry
        .register_gauge(gauge_metric)
        .expect("Failed to register gauge metric!");

    // Timing count
    let timing_count_metric = Metric::new_timing_count(EXAMPLE_TIMING_COUNT_METRIC)
        .with_tags(&["test_tag2:test_tag_value2"]);
    let timing_instrument = registry
        .register_timing_count(timing_count_metric)
        .expect("Failed to register count metric!");
    // Creating and registering the struct
    let my_example_metrics =
        ExampleMetrics::register(&registry).expect("Failed to register metrics!");
    loop {
        // Using the struct
        my_example_metrics.example_count.increment();
        my_example_metrics.example_gauge.swap(5.5);
        my_example_metrics
            .example_timing_count
            .add_timing(&std::time::Duration::from_secs(5));
        // individual instruments
        count_instrument.increment();
        println!("Incremented metric!");
        my_gauge.swap(5.5);
        println!("Swapped gauge metric!");
        timing_instrument.add_timing(&std::time::Duration::from_secs(5));
        println!("Added timing!");
        tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;
    }
}
