//! Provider registry: the only place that knows which adapters exist.

use crate::capabilities::CapabilityProfile;
use crate::ids::ProviderId;
use crate::provider::{ProviderAdapter, ProviderDescriptor};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderInfo {
    pub descriptor: ProviderDescriptor,
    pub capabilities: CapabilityProfile,
}

#[derive(Default, Clone)]
pub struct ProviderRegistry {
    adapters: BTreeMap<ProviderId, Arc<dyn ProviderAdapter>>,
    order: Vec<ProviderId>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an adapter. Registering the same id twice replaces the previous one.
    pub fn register(&mut self, adapter: Arc<dyn ProviderAdapter>) {
        let id = adapter.id();
        if !self.adapters.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.adapters.insert(id, adapter);
    }

    pub fn get(&self, id: &ProviderId) -> Option<Arc<dyn ProviderAdapter>> {
        self.adapters.get(id).cloned()
    }

    /// Adapters in registration order.
    pub fn adapters(&self) -> Vec<Arc<dyn ProviderAdapter>> {
        self.order
            .iter()
            .filter_map(|id| self.adapters.get(id).cloned())
            .collect()
    }

    pub fn infos(&self) -> Vec<ProviderInfo> {
        self.adapters()
            .into_iter()
            .map(|a| ProviderInfo {
                descriptor: a.descriptor(),
                capabilities: a.capabilities(),
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{Capabilities, ImplementedFeatures};
    use crate::ids::SessionId;
    use crate::provider::{ProviderError, StopMode};

    struct Fake(&'static str);

    #[async_trait::async_trait]
    impl ProviderAdapter for Fake {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: ProviderId::new(self.0),
                display_name: self.0.to_uppercase(),
                accent_color: "#fff".into(),
                badge: "F".into(),
                executable_names: vec![],
                homepage: None,
                simulated: false,
            }
        }
        fn capabilities(&self) -> CapabilityProfile {
            CapabilityProfile {
                managed: Capabilities::NONE,
                external: Capabilities::NONE,
                implemented: ImplementedFeatures::default(),
                notes: vec![],
            }
        }
    }

    #[tokio::test]
    async fn keeps_registration_order_and_defaults_are_unsupported() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(Fake("zeta")));
        registry.register(Arc::new(Fake("alpha")));
        registry.register(Arc::new(Fake("zeta")));
        let ids: Vec<_> = registry
            .infos()
            .into_iter()
            .map(|i| i.descriptor.id.0)
            .collect();
        assert_eq!(ids, vec!["zeta", "alpha"]);

        let adapter = registry.get(&ProviderId::new("alpha")).unwrap();
        let err = adapter
            .stop_session(&SessionId::new("x"), StopMode::Graceful)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ProviderError::Unsupported { capability: "stop" }
        ));
    }
}
