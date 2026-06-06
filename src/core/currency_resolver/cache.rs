use super::{CurrencyCacheKey, CurrencyCacheKind, CurrencyHints, ResolvedCurrency};
use crate::core::YfClient;

impl YfClient {
    pub(crate) async fn store_currency_hints(&self, symbol: &str, hints: CurrencyHints) {
        let mut guard = self.currency_hints.write().await;
        let key = symbol.to_string();
        let mut hints = hints;
        if let Some(mut existing) = guard.remove(symbol) {
            existing.merge(hints);
            hints = existing;
        }
        guard.insert(key, hints);
    }

    pub(crate) async fn cached_currency_hints(&self, symbol: &str) -> CurrencyHints {
        self.currency_hints
            .write()
            .await
            .get_cloned(symbol)
            .unwrap_or_default()
    }

    pub(crate) async fn store_resolved_currency(
        &self,
        symbol: &str,
        kind: CurrencyCacheKind,
        resolved: ResolvedCurrency,
    ) {
        let mut guard = self.currency_cache.write().await;
        let key = CurrencyCacheKey::new(symbol, kind);
        let should_store = guard
            .get_cloned(&key)
            .as_ref()
            .is_none_or(|existing| resolved.cache_rank_ge(existing));

        if should_store {
            crate::core::logging::trace_debug!(
                symbol,
                kind = ?kind,
                evidence = ?resolved.evidence(),
                "cached resolved currency"
            );
            guard.insert(key, resolved);
        }
    }

    pub(crate) async fn cached_resolved_currency(
        &self,
        symbol: &str,
        kind: CurrencyCacheKind,
    ) -> Option<ResolvedCurrency> {
        self.currency_cache
            .write()
            .await
            .get_cloned(&CurrencyCacheKey::new(symbol, kind))
            .map(|resolved| resolved.with_cached_acquisition(kind))
    }
}
