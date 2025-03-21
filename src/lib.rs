//! gnort handles metrics reporting for Rust users.
//! The library handles configuring the client correctly and provides a simple interface
//! for performantly aggregating your metrics so that you aren't blasting UDP
//! packets for every metrical update. Instead, the [MetricsRegistry](registry::MetricsRegistry)
//! will aggregate your metrics and emit them every 10 seconds.
//! ### Example: Using [MetricsRegistry](registry::MetricsRegistry) for emitting aggregated metrics
//!
//! ```
//! use gnort::*;
//!
//! metrics_struct![
//!     ExampleMetrics,
//!     (count, "gnort.test.bench.count", Count),
//!     (gauge, "gnort.test.bench.gauge", Gauge, ["metric_tag:tag_value"]),
//!     (timing_count, "gnort.test.bench.timing_count", TimingCount)
//! ];
//! #[tokio::main(flavor = "multi_thread", worker_threads = 4)]
//! pub async fn main() {
//!     let client = GnortClient::default().expect("Failed to instantiate client!");
//!     let registry =
//!         MetricsRegistry::new(
//!             RegistryConfig::default().with_client(client)
//!     );
//!     let metrics = ExampleMetrics::register(&registry).expect("Failed to register metrics!");
//!     {
//!         metrics.count.increment();
//!         println!("Incremented metric!");
//!         metrics.gauge.swap(5.5);
//!         println!("Swapped gauge metric!");
//!         let _ = metrics.timing_count.measure_async_fut(async {
//!             tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await
//!         }).await;
//!     }
//! }
//! ```
//! # Usage
//!
//! ## Aggregated metrics
//!
//! Generally you should default to using aggregated metrics wherever possible. A convenient API for this is the `metrics_struct!` macro.
//!
//! ```
//! use gnort::*;
//! metrics_struct![
//!     ExampleMetrics,
//!     (example_count, "gnort.test.bench.count", Count),
//!     (example_gauge, "gnort.test.bench.gauge", Gauge),
//!     (example_timing_count, "gnort.test.bench.timing_count", TimingCount)
//! ];
//! ```
//!
//! In this example, `ExampleMetrics` is the name of the struct type, `example_count` is the field name for the count metric, and `"gnort.test.bench.count"`
//! is the stat name that will appear in DataDog. `Count` is declaring the metric type to be a count. `Count` and `Gauge` work in fairly straight-forward
//! ways. `TimingCount` is a little particular to Gnort but very helpful. It lets you aggregate timing metrics efficiently by tracking the sum-of-time-spent
//! and count-of-measures-taken as separate count metrics. You then divide the sum by the count in the DataDog dashboard to get an accurate average for each
//! aggregation window.
//!
//! Gnort will automatically suffix the "sum of durations" as your stat name plus `".time"`. The count will be the stat name verbatim. So if your stat name is `"gnort.test.bench.timing_count"`, then you divide `"gnort.test.bench.timing_count.time"` by `"gnort.test.bench.timing_count"` to get the average time spent.
//!
//! ### Instantiating the MetricsRegistry for your metrics and registering them
//!
//! ```
//! use gnort::*;
//! # metrics_struct![
//! # ExampleMetrics,
//! #     (example_count, "gnort.test.bench.count", Count),
//! #     (example_gauge, "gnort.test.bench.gauge", Gauge),
//! #     (example_timing_count, "gnort.test.bench.timing_count", TimingCount)
//! # ];
//!
//! let client = GnortClient::default().expect("Failed to instantiate client!");
//! let registry =
//!     MetricsRegistry::new(
//!         RegistryConfig::default().with_client(client)
//!     );
//! // then to register the actual metrics:
//! let my_example_metrics = ExampleMetrics::register(&registry).expect("Failed to register metrics!");
//! ```
//!
//! The metrics struct returned by the `register` static method is a struct-of-instruments that can be used to record observations with a well-typed interface that doesn't require juggling a bunch of individual metric references. You can then put one or more of these structs wherever you put the rest of your application's shared state, such as where you share your database connection pool.
//!
//! ## Ad-hoc metrics (high-level API)
//!
//! In cases where you need ad-hoc metrics (highly variable tagging perhaps?), you aren't limited to dropping down to the raw client. There's an `adhoc_metrics_struct` macro as well.
//!
//! Here's an example:
//!
//! ```rust,ignore
//! let client = metrics::gnort_client();
//! let cronjob = metrics::cronjob();
//! if let Some(high_watermark) = self.highest_observed_watermark {
//!     let _ = cronjob
//!         .highest_observed_watermark
//!         .adhoc_gauge(client, high_watermark.timestamp() as f64, btreeset!{});
//! };
//! let _ = cronjob
//!     .columns_count
//!     .adhoc_count(client, self.columns as i64, btreeset!{});
//! ```
//!
//! ## Synchronous-only client
//!
//! The client is sync-only because you should be aggregating your metrical emissions and emitting them in a background thread once every ~10-30 seconds depending on your needs.
//!
//! ## Using the raw client for ad-hoc metrics
//!
//! You can use the raw client API if desired but be aware that this isn't recommended. Aggregated metrics are best and there's a higher-level API for ad-hoc metrics available.
//!

/// [GnortClient] is the client for emitting dogstatsd metrics to a statsd server.
/// You usually don't need to poke around this module, you just instantiate clients
/// for use with [MetricsRegistry](registry::MetricsRegistry).
pub mod client;
/// [Instrument](instrument::Instrument) is the core type for metrical values. It is the value type used to register metrics with [MetricsRegistry](registry::MetricsRegistry).
pub mod instrument;
pub mod macros;
/// [Metric] is the core type for metrical metadata. It is the key type used to register metrics with [MetricsRegistry](registry::MetricsRegistry).
pub mod metric;
/// [MetricsRegistry] is how metrics are registered and emitted.
pub mod registry;

pub use client::GnortClient;
pub use metric::*;
pub use registry::*;
