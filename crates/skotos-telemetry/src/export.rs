//! Real-Time Data Export Sink Formatter & Dispatcher (Phase 4).
//!
//! Serializes evaluation/exposure/custom event batches into Zstd-compressed NDJSON
//! for zero-egress R2/S3 archival or streams formatted JSON envelopes to external
//! sinks (Kinesis, Pub/Sub, Snowflake Snowpipe, Segment HTTP API, or Webhook NDJSON).

use serde_json::{json, Value};
use skotos_domain::{DataExportSink, DataExportSinkKind};

use crate::TelemetryError;

/// Compresses JSON records as Zstd-3 NDJSON bytes (`(zstd_bytes, uncompressed_count)`).
pub fn encode_zstd_ndjson(records: &[Value]) -> Result<Vec<u8>, TelemetryError> {
    let mut raw = Vec::with_capacity(records.len() * 128);
    for rec in records {
        serde_json::to_writer(&mut raw, rec)
            .map_err(|e| TelemetryError::InvalidPayload(e.to_string()))?;
        raw.push(b'\n');
    }
    zstd::bulk::compress(&raw, 3)
        .map_err(|e| TelemetryError::Backend(format!("zstd compression failed: {e}")))
}

/// Formats a batch payload appropriate for the target `DataExportSinkKind`.
pub fn format_sink_payload(sink: &DataExportSink, records: &[Value]) -> Value {
    match sink.kind {
        DataExportSinkKind::Segment => json!({
            "batch": records.iter().map(|r| json!({
                "type": "track",
                "event": r.get("kind").and_then(Value::as_str).unwrap_or("skotos.event"),
                "properties": r,
            })).collect::<Vec<_>>()
        }),
        DataExportSinkKind::Kinesis => json!({
            "StreamName": sink.destination,
            "Records": records.iter().map(|r| json!({
                "PartitionKey": r.get("flagKey").or_else(|| r.get("key")).and_then(Value::as_str).unwrap_or("skotos"),
                "Data": r,
            })).collect::<Vec<_>>()
        }),
        DataExportSinkKind::Pubsub => json!({
            "messages": records.iter().map(|r| json!({
                "attributes": { "source": "skotos" },
                "data": r,
            })).collect::<Vec<_>>()
        }),
        DataExportSinkKind::Snowflake => json!({
            "table": sink.destination,
            "rows": records,
        }),
        DataExportSinkKind::S3Parquet | DataExportSinkKind::WebhookNdjson => json!({
            "source": "skotos",
            "sinkId": sink.id,
            "count": records.len(),
            "events": records,
        }),
    }
}

/// Dispatches an HTTP export batch when `sink.destination` is an HTTP(S) URL.
pub async fn dispatch_http_sink(
    http: &reqwest::Client,
    sink: &DataExportSink,
    records: &[Value],
) -> Result<usize, TelemetryError> {
    if !sink.destination.starts_with("http://") && !sink.destination.starts_with("https://") {
        return Err(TelemetryError::InvalidPayload(format!(
            "HTTP dispatch requires http(s) destination, got '{}'",
            sink.destination
        )));
    }
    let payload = format_sink_payload(sink, records);
    let body =
        serde_json::to_vec(&payload).map_err(|e| TelemetryError::InvalidPayload(e.to_string()))?;
    let bytes_len = body.len();
    let resp = http
        .post(&sink.destination)
        .header("content-type", "application/json")
        .header("x-skotos-export-sink", sink.id.to_string())
        .body(body)
        .send()
        .await
        .map_err(|e| TelemetryError::Backend(format!("data export HTTP error: {e}")))?;
    if !resp.status().is_success() {
        return Err(TelemetryError::Backend(format!(
            "data export HTTP {} from {}",
            resp.status(),
            sink.destination
        )));
    }
    Ok(bytes_len)
}
