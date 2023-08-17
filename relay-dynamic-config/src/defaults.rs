use relay_common::DataCategory;
use relay_general::store::LazyGlob;
use relay_sampling::{EqCondition, RuleCondition};
use serde_json::Value;

use crate::error_boundary::ErrorBoundary;
use crate::feature::Feature;
use crate::metrics::{MetricExtractionConfig, MetricSpec, TagMapping, TagSpec};
use crate::project::ProjectConfig;

macro_rules! tag_specs {
    ($($key:literal = $value:literal),* $(,)?) => {
        vec![
            $(
                TagSpec {
                    key: $key.to_owned(),
                    field: Some($value.to_owned()),
                    value: None,
                    condition: None,
                },
            )*
        ]
    }
}

pub fn add_transaction_metrics(project_config: &mut ProjectConfig) {
    let tx_config = match project_config.transaction_metrics {
        Some(ErrorBoundary::Ok(ref tx_config)) if tx_config.is_enabled() => tx_config,
        _ => return,
    };

    let config = project_config
        .metric_extraction
        .get_or_insert_with(MetricExtractionConfig::empty);

    config._transaction_metrics_extended = true;
    if config.version == 0 {
        config.version = MetricExtractionConfig::VERSION;
    }

    // TODO: Add defaults here.

    config.metrics.extend([]);

    config.tags.extend([TagMapping {
        metrics: vec![
            // TODO(ja): Add metrics
            LazyGlob::new("".into()),
        ],
        tags: tag_specs![
            // TODO
        ],
    }])
}

/// Adds configuration for extracting metrics from spans.
///
/// This configuration is temporarily hard-coded here. It will later be provided by the upstream.
/// This requires the `SpanMetricsExtraction` feature. This feature should be set to `false` if the
/// default should no longer be placed.
pub fn add_span_metrics(project_config: &mut ProjectConfig) {
    if !project_config.features.has(Feature::SpanMetricsExtraction) {
        return;
    }

    let config = project_config
        .metric_extraction
        .get_or_insert_with(MetricExtractionConfig::empty);

    if !config.is_supported() || config._span_metrics_extended {
        return;
    }

    config.metrics.extend([
        MetricSpec {
            category: DataCategory::Span,
            mri: "d:spans/exclusive_time@millisecond".into(),
            field: Some("span.exclusive_time".into()),
            condition: None,
            tags: vec![TagSpec {
                key: "transaction".into(),
                field: Some("span.data.transaction".into()),
                value: None,
                condition: None,
            }],
        },
        MetricSpec {
            category: DataCategory::Span,
            mri: "d:spans/exclusive_time_light@millisecond".into(),
            field: Some("span.exclusive_time".into()),
            condition: None,
            tags: Default::default(),
        },
    ]);

    config.tags.extend([
        TagMapping {
            metrics: vec![LazyGlob::new("d:spans/exclusive_time*@millisecond".into())],
            tags: tag_specs![
                "environment" = "span.data.environment",
                "http.status_code" = "span.data.http\\.status_code",
                "span.action" = "span.data.span\\.action",
                "span.category" = "span.data.span\\.category",
                "span.description" = "span.data.span\\.description",
                "span.domain" = "span.data.span\\.domain",
                "span.group" = "span.data.span\\.group",
                "span.module" = "span.data.span\\.module",
                "span.op" = "span.data.span\\.op",
                "span.status_code" = "span.data.span\\.status_code",
                "span.status" = "span.data.span\\.status",
                "span.system" = "span.data.span\\.system",
                "transaction.method" = "span.data.transaction\\.method",
                "transaction.op" = "span.data.transaction\\.op",
            ],
        },
        TagMapping {
            metrics: vec![LazyGlob::new("d:spans/exclusive_time*@millisecond".into())],
            tags: ["release", "device.class"]
                .into_iter()
                .map(|key| TagSpec {
                    key: key.into(),
                    field: Some(format!("span.data.{}", key.replace('.', "\\."))),
                    value: None,
                    condition: Some(RuleCondition::Eq(EqCondition {
                        name: "span.data.mobile".into(),
                        value: Value::Bool(true),
                        options: Default::default(),
                    })),
                })
                .collect(),
        },
    ]);

    config._span_metrics_extended = true;
    if config.version == 0 {
        config.version = MetricExtractionConfig::VERSION;
    }
}
