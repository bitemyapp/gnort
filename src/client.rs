use std::{borrow::Cow, env, sync::Arc};

use dogstatsd::*;
use once_cell::sync::OnceCell;

pub const STATSD_HOST_ENV: &str = "STATSD_HOST";
pub const STATSD_PORT_ENV: &str = "STATSD_PORT";
const DEFAULT_ORIGIN: &str = "0.0.0.0:0";
// Port 8125(UDP) is for metrics,
// port 8126(TCP) is for Datadog APM (tracing)

const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_PORT: &str = "8125";

static SYNC_INSTANCE: OnceCell<GnortClient> = OnceCell::new();

pub(crate) fn sync_client() -> &'static GnortClient {
    SYNC_INSTANCE.get_or_init(|| {
        GnortClient::default().expect("Couldn't instantiate sync Datadog metrics client")
    })
}

/// The only metrics client is synchronous because you should be aggregating your metrics
/// using [crate::registry::MetricsRegistry].
#[derive(Clone)]
pub struct GnortClient {
    /// The Arc around the dogstatsd client is a hack to work around the lack of a native Clone implementation.
    client: Arc<Client>,
}

pub(crate) fn get_default_tags() -> Vec<String> {
    let env = env::var("DD_ENV").map(|t| format!("env:{}", t));
    let version = env::var("DD_VERSION").map(|t| format!("version:{}", t));
    let service = env::var("DD_SERVICE").map(|t| format!("service:{}", t));
    let optional_tags = vec![env, version, service];
    let only_existing_tags = optional_tags.into_iter().filter_map(|t| t.ok()).collect();
    only_existing_tags
}

impl GnortClient {
    pub fn default() -> Result<Self, DogstatsdError> {
        let no_tags = &[] as &[&str];
        Self::new(None, no_tags)
    }

    pub fn new<I, T>(
        namespace: Option<&str>,
        extra_default_tags: I,
    ) -> Result<GnortClient, dogstatsd::DogstatsdError>
    where
        T: AsRef<str>,
        I: IntoIterator<Item = T>,
    {
        let mut extra_default_tags = extra_default_tags
            .into_iter()
            .map(|t| t.as_ref().to_string())
            .collect::<Vec<_>>();
        extra_default_tags.sort();

        let statsd_host = env::var(STATSD_HOST_ENV).unwrap_or(DEFAULT_HOST.to_string());
        let statsd_port = env::var(STATSD_PORT_ENV).unwrap_or(DEFAULT_PORT.to_string());
        let udp_origin = DEFAULT_ORIGIN.to_string();
        let udp_target = format!("{}:{}", statsd_host, statsd_port);
        let actual_namespace = namespace.unwrap_or("");
        let mut default_tags = get_default_tags();
        default_tags.extend(extra_default_tags);
        let options = Options {
            socket_path: None,
            batching_options: None,
            default_tags,
            from_addr: udp_origin,
            to_addr: udp_target,
            namespace: actual_namespace.to_string(),
        };
        let client = Client::new(options)?;

        let gnort_client = GnortClient {
            client: Arc::new(client),
        };
        Ok(gnort_client)
    }

    pub fn count<'a, I, S, T>(&self, stat: S, count: i64, tags: I) -> DogstatsdResult
    where
        I: IntoIterator<Item = T>,
        S: Into<Cow<'a, str>>,
        T: AsRef<str>,
    {
        self.client.count(stat, count, tags)
    }

    pub fn event<'a, I, S, SS, T>(&self, title: S, text: SS, tags: I) -> DogstatsdResult
    where
        I: IntoIterator<Item = T>,
        S: Into<Cow<'a, str>>,
        SS: Into<Cow<'a, str>>,
        T: AsRef<str>,
    {
        self.client.event(title, text, tags)
    }

    pub fn gauge<'a, I, S, SS, T>(&self, stat: S, val: SS, tags: I) -> DogstatsdResult
    where
        I: IntoIterator<Item = T>,
        S: Into<Cow<'a, str>>,
        SS: Into<Cow<'a, str>>,
        T: AsRef<str>,
    {
        self.client.gauge(stat, val, tags)
    }

    pub fn timing<'a, I, S, T>(&self, stat: S, milliseconds: i64, tags: I) -> DogstatsdResult
    where
        I: IntoIterator<Item = T>,
        S: Into<Cow<'a, str>>,
        T: AsRef<str>,
    {
        self.client.timing(stat, milliseconds, tags)
    }
}
