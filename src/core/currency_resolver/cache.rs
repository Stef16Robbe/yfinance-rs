use super::{CurrencyCacheKey, CurrencyHints, CurrencyKind, ResolvedCurrency};
use crate::core::YfClient;

impl YfClient {
    pub(crate) async fn store_currency_hints(&self, symbol: &str, hints: CurrencyHints) {
        let mut guard = self.currency_hints.write().await;
        guard
            .entry(symbol.to_string())
            .and_modify(|existing| existing.merge(hints.clone()))
            .or_insert(hints);
    }

    pub(crate) async fn cached_currency_hints(&self, symbol: &str) -> CurrencyHints {
        self.currency_hints
            .read()
            .await
            .get(symbol)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) async fn store_resolved_currency(
        &self,
        symbol: &str,
        kind: CurrencyKind,
        resolved: ResolvedCurrency,
    ) {
        let mut guard = self.currency_cache.write().await;
        let key = CurrencyCacheKey::new(symbol, kind);
        let should_store = guard
            .get(&key)
            .is_none_or(|existing| resolved.strength >= existing.strength);

        if should_store {
            crate::core::logging::trace_debug!(
                symbol,
                kind = ?kind,
                source = ?resolved.source,
                "cached resolved currency"
            );
            guard.insert(key, resolved);
        }
    }

    pub(crate) async fn cached_resolved_currency(
        &self,
        symbol: &str,
        kind: CurrencyKind,
    ) -> Option<ResolvedCurrency> {
        self.currency_cache
            .read()
            .await
            .get(&CurrencyCacheKey::new(symbol, kind))
            .cloned()
    }
}
