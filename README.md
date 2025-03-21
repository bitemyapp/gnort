# gnort

- [crates.io](https://crates.io/crates/gnort)

<img src="gnort.webp" alt="alien dog named G-n-o-r-t looking nonplussed" width="200"/>

`gnort` is a Datadog client library that provides efficient in-process metrics aggregation. I wrote this because I wanted to be able to do a mixture of aggregated and ad-hoc metrics with minimal boilerplate. Accordingly, a lot of the value of this library is the codegen via macros, registry, and aggregation windows.

I say this a "Datadog" library because the aggregation windows and push-based mechanisms aren't really compatible with the assumptions of the Extended Prometheus Cinematic Universe. I find Prometheus makes metrical analysis and processing more difficult rather than easier although I do appreciate why they went with pull-based metrics.

This library does use the `dogstatsd` crate under the hood, but I could imagine making it more generally "statsd" oriented.

## Wishlist

I'd like it if Datadog made distributions something we could aggregate client-side. [Daddy needs his count-min sketch.](https://dsf.berkeley.edu/cs286/papers/countmin-latin2004.pdf)
