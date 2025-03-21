## build release
build-release:
	cargo build --release --example aggregation

## run aggregation example
run-aggregation:
	cargo run --release --example aggregation

## run raw client example
run-raw-client:
	cargo run --release --example raw_client

## single-threaded test execution to minimize perf noise
test:
	RUST_BACKTRACE=1 cargo test --release -- --nocapture --test-threads 1
