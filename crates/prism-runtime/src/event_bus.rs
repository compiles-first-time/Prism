//! Event bus publisher (REUSABLE_EventBusPublisher).
//!
//! Publishes governance events for downstream consumers. The trait abstracts
//! the transport so that unit tests use an in-memory implementation and
//! production uses Redis Streams (partitioned by tenant_id).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use prism_core::types::TenantId;

/// A governance event published to the bus.
///
/// This is a lightweight envelope -- the full event details are in the
/// audit trail. The bus carries just enough for subscribers to filter
/// and decide whether to fetch the full event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusEvent {
    pub tenant_id: TenantId,
    pub event_type: String,
    pub source_entity_id: uuid::Uuid,
    pub timestamp: DateTime<Utc>,
    pub payload: serde_json::Value,
}

/// Trait for publishing events to the governance event bus.
///
/// Implements: REUSABLE_EventBusPublisher
#[async_trait]
pub trait EventBusPublisher: Send + Sync {
    /// Publish a single event. Fire-and-forget semantics: the caller
    /// does not wait for subscriber acknowledgment.
    async fn publish(&self, event: BusEvent) -> Result<(), EventBusError>;
}

/// Event bus errors.
#[derive(Debug, thiserror::Error)]
pub enum EventBusError {
    #[error("event bus unavailable: {0}")]
    Unavailable(String),

    #[error("serialization error: {0}")]
    Serialization(String),
}

/// In-memory event bus for testing and development.
///
/// Stores published events in a `Vec` behind a `Mutex`.
/// Useful for asserting that the correct events were published
/// without requiring Redis infrastructure.
pub struct InMemoryEventBus {
    events: std::sync::Mutex<Vec<BusEvent>>,
}

impl InMemoryEventBus {
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Read all published events (test helper).
    pub fn events(&self) -> Vec<BusEvent> {
        self.events.lock().unwrap().clone()
    }

    /// Count of published events (test helper).
    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventBusPublisher for InMemoryEventBus {
    async fn publish(&self, event: BusEvent) -> Result<(), EventBusError> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

/// No-op event bus that silently drops all events.
///
/// Used when event publishing is not yet wired up or is intentionally
/// disabled (e.g., during migration scripts).
pub struct NoOpEventBus;

#[async_trait]
impl EventBusPublisher for NoOpEventBus {
    async fn publish(&self, _event: BusEvent) -> Result<(), EventBusError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_event(tenant_id: TenantId) -> BusEvent {
        BusEvent {
            tenant_id,
            event_type: "test.event".into(),
            source_entity_id: uuid::Uuid::nil(),
            timestamp: Utc::now(),
            payload: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn in_memory_bus_stores_events() {
        let bus = InMemoryEventBus::new();
        let tid = TenantId::new();

        bus.publish(test_event(tid)).await.unwrap();
        bus.publish(test_event(tid)).await.unwrap();

        assert_eq!(bus.len(), 2);
        assert_eq!(bus.events()[0].event_type, "test.event");
    }

    #[tokio::test]
    async fn noop_bus_drops_silently() {
        let bus = NoOpEventBus;
        bus.publish(test_event(TenantId::new())).await.unwrap();
        // No panic, no storage -- just succeeds.
    }

    #[tokio::test]
    async fn events_are_tenant_scoped() {
        let bus = InMemoryEventBus::new();
        let t1 = TenantId::new();
        let t2 = TenantId::new();

        bus.publish(test_event(t1)).await.unwrap();
        bus.publish(test_event(t2)).await.unwrap();

        let events = bus.events();
        assert_eq!(events[0].tenant_id, t1);
        assert_eq!(events[1].tenant_id, t2);
    }
}
