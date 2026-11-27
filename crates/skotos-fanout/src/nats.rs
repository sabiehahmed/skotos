use async_nats::jetstream::stream::{Config as StreamConfig, RetentionPolicy, StorageType};
use async_nats::{jetstream, Client, Subscriber};
use bytes::Bytes;
use uuid::Uuid;

use crate::protocol::{
    env_events_subject, AuthRegistryEvent, DeliveryEvent, FanoutError, AUTH_KEYS_SUBJECT,
    NATS_STREAM_NAME,
};

/// NATS JetStream fanout client shared by `skotos-fanout` workers and `services/delivery-edge`.
#[derive(Clone)]
pub struct NatsFanout {
    client: Client,
    jetstream: jetstream::Context,
}

impl NatsFanout {
    pub async fn connect(nats_url: &str) -> Result<Self, FanoutError> {
        let client = async_nats::connect(nats_url)
            .await
            .map_err(|e| FanoutError::Nats(format!("connect {nats_url}: {e}")))?;
        let jetstream = jetstream::new(client.clone());

        // Ensure the JetStream stream exists for durable audit/replay + real-time fanout.
        let stream_cfg = StreamConfig {
            name: NATS_STREAM_NAME.to_string(),
            subjects: vec![
                "skotos.env.*.events".to_string(),
                "skotos.auth.>".to_string(),
            ],
            retention: RetentionPolicy::Limits,
            storage: StorageType::File,
            max_messages_per_subject: 1_000,
            ..Default::default()
        };
        jetstream
            .get_or_create_stream(stream_cfg)
            .await
            .map_err(|e| FanoutError::Nats(format!("ensure stream {NATS_STREAM_NAME}: {e}")))?;

        Ok(Self { client, jetstream })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Publishes an environment delivery event into NATS JetStream (automatically delivers
    /// to live subscribers on `skotos.env.{env_id}.events` in < 2 ms).
    pub async fn publish_delivery_event(
        &self,
        env_id: Uuid,
        event: &DeliveryEvent,
    ) -> Result<(), FanoutError> {
        let subject = env_events_subject(env_id);
        let payload = Bytes::from(serde_json::to_vec(event)?);
        let ack_fut = self
            .jetstream
            .publish(subject.clone(), payload)
            .await
            .map_err(|e| FanoutError::Nats(format!("publish {subject}: {e}")))?;
        ack_fut
            .await
            .map_err(|e| FanoutError::Nats(format!("ack {subject}: {e}")))?;
        Ok(())
    }

    /// Publishes an auth registry update on `skotos.auth.keys`.
    pub async fn publish_auth_event(&self, event: &AuthRegistryEvent) -> Result<(), FanoutError> {
        let payload = Bytes::from(serde_json::to_vec(event)?);
        let ack_fut = self
            .jetstream
            .publish(AUTH_KEYS_SUBJECT, payload)
            .await
            .map_err(|e| FanoutError::Nats(format!("publish {AUTH_KEYS_SUBJECT}: {e}")))?;
        ack_fut
            .await
            .map_err(|e| FanoutError::Nats(format!("ack {AUTH_KEYS_SUBJECT}: {e}")))?;
        Ok(())
    }

    /// Subscribes to live events for a single environment (`1 NATS sub per active env_id`).
    pub async fn subscribe_env(&self, env_id: Uuid) -> Result<Subscriber, FanoutError> {
        let subject = env_events_subject(env_id);
        self.client
            .subscribe(subject.clone())
            .await
            .map_err(|e| FanoutError::Nats(format!("subscribe {subject}: {e}")))
    }

    /// Subscribes to live auth registry key updates across all environments.
    pub async fn subscribe_auth_keys(&self) -> Result<Subscriber, FanoutError> {
        self.client
            .subscribe(AUTH_KEYS_SUBJECT)
            .await
            .map_err(|e| FanoutError::Nats(format!("subscribe {AUTH_KEYS_SUBJECT}: {e}")))
    }
}
