use gnort::GnortClient;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
pub async fn main() {
    std::env::set_var(gnort::client::STATSD_HOST_ENV, "0.0.0.0");
    let client = GnortClient::default().expect("Failed to instantiate client!");
    let tags = &["name1:value1".to_string(), "outcome:success".to_string()];
    loop {
        client
            .count("gnort.test.bench.incr", 1, tags)
            .expect("Failed to emit metric!");
        println!("Emitted metric!");
        tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;
    }
}
