#[macro_export]
macro_rules! metric {
    ( $binding:ident, $metric_name:literal, Count ) => {
        pub const $binding: $crate::metric::MetricName<MetricType::Count> =
            $crate::metric::MetricName::count($metric_name);
    };
    ( $binding:ident, $metric_name:literal, Gauge ) => {
        pub const $binding: $crate::metric::MetricName<MetricType::Gauge> =
            $crate::metric::MetricName::gauge($metric_name);
    };
    ( $binding:ident, $metric_name:literal, TimingCount ) => {
        pub const $binding: $crate::metric::MetricName<MetricType::TimingCount> =
            $crate::metric::MetricName::timing_count($metric_name);
    };
}

// TODO: metrics_module has a similar but not identical thing for this that is Metric instead of MetricName
#[macro_export]
macro_rules! adhoc_metrics_struct {
    ($StructName:ident, $(($field_name:ident, $metric_name:literal, $metric_type:ident $(, $tags:expr)?)),*) => {
        #[derive(Clone)]
        pub struct $StructName {
            $(
                pub $field_name: $crate::metric::Metric<$crate::metric::MetricType::$metric_type>,
            )+
        }

        impl $StructName {
            pub fn new(
            ) -> Self {
                $(
                    let metric_name = MetricName::new($metric_name);
                    let metric: Metric<MetricType::$metric_type> = metric_name.into();
                    $(
                        let metric = metric.with_array_tags($tags);
                    )*
                    let $field_name = metric;
                )+
                Self {
                    $(
                        $field_name,
                    )+
                }
            }
        }
    }
}

#[macro_export]
macro_rules! metrics_struct {
    // TODO: Add more options for the metrics/instruments
    ($StructName:ident, $(($field_name:ident, $metric_name:literal, $metric_type:ident $(, $tags:expr)?)),*) => {
        #[derive(Clone)]
        pub struct $StructName {
            $(
                pub $field_name: $crate::instrument::$metric_type,
            )+
        }

        impl $StructName {
            pub fn register(
                registry: &MetricsRegistry,
            ) -> Result<Self, MetricRegistrationError> {
                $(
                    let metric_name = $crate::metric::MetricName::new($metric_name);
                    let metric: $crate::metric::Metric<$crate::metric::MetricType::$metric_type> = metric_name.into();
                    $(
                        let metric = metric.with_array_tags($tags);
                    )*
                    let $field_name = registry.register_metric(metric)?;
                )+
                Ok(Self {
                    $(
                        $field_name,
                    )+
                })
            }
        }
    }
}

#[macro_export]
macro_rules! metrics_module {
    // TODO: Add more options for the metrics/instruments
    ($ModName:ident, $(($field_name:ident, $metric_name:literal, $metric_type:ident $(, $tags:expr)?)),*) => {
        mod $ModName {
            #[derive(Clone)]
            pub struct Metrics {
                $(
                    // TODO: Generate module with two struct types, one of the metrics/metric names, one of the instruments?
                    // pub $field_name: Metric<MetricType::$metric_type>,
                    pub $field_name: $crate::metric::Metric<$crate::metric::MetricType::$metric_type>,
                )+
            }

            impl Metrics {
                pub fn new(
                ) -> Self {
                    $(
                        let metric_name = $crate::metric::MetricName::new($metric_name);
                        let metric: $crate::metric::Metric<$crate::metric::MetricType::$metric_type> = metric_name.into();
                        $(
                            let metric = metric.with_array_tags($tags);
                        )*
                        let $field_name = metric;
                    )+
                    Self {
                        $(
                            $field_name,
                        )+
                    }
                }

                pub fn register(
                    &self,
                    registry: &$crate::registry::MetricsRegistry,
                ) -> Result<Instruments, $crate::registry::MetricRegistrationError> {
                    $(
                        let $field_name = registry.register_metric::<$crate::metric::Metric<$crate::metric::MetricType::$metric_type>, $crate::metric::MetricType::$metric_type>(self.$field_name.clone())?;
                    )+
                    Ok(Instruments {
                        $(
                            $field_name,
                        )+
                    })
                }
            }

            #[derive(Clone)]
            pub struct Instruments {
                $(
                    // TODO: Generate module with two struct types, one of the metrics/metric names, one of the instruments?
                    pub $field_name: crate::instrument::$metric_type,
                )+
            }

            impl Instruments {
                pub fn register(
                    registry: &$crate::registry::MetricsRegistry,
                ) -> Result<Self, $crate::registry::MetricRegistrationError> {
                    $(
                        let metric_name = $crate::metric::MetricName::new($metric_name);
                        let metric: $crate::metric::Metric<$crate::metric::MetricType::$metric_type> = metric_name.into();
                        $(
                            let metric = metric.with_array_tags($tags);
                        )*
                        let $field_name = registry.register_metric::<$crate::metric::Metric<$crate::metric::MetricType::$metric_type>, $crate::metric::MetricType::$metric_type>(metric)?;
                    )+
                    Ok(Self {
                        $(
                            $field_name,
                        )+
                    })
                }
            }
        }
    }
}
