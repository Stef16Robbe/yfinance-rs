use super::{CurrencyCacheKey, CurrencyCacheKind, CurrencyHints, ResolvedCurrency};
use crate::core::YfClient;

impl YfClient {
    pub(crate) fn store_currency_hints(&self, symbol: &str, hints: CurrencyHints) {
        let key = symbol.to_string();
        self.currency_hints.compute_with(key, |existing| {
            let mut hints = hints;
            if let Some(mut existing) = existing {
                existing.merge(hints);
                hints = existing;
            }
            hints
        });
    }

    pub(crate) fn cached_currency_hints(&self, symbol: &str) -> CurrencyHints {
        self.currency_hints.get_str(symbol).unwrap_or_default()
    }

    pub(crate) fn store_resolved_currency(
        &self,
        symbol: &str,
        kind: CurrencyCacheKind,
        resolved: ResolvedCurrency,
    ) {
        let key = CurrencyCacheKey::new(symbol, kind);
        let mut candidate = Some(resolved);
        self.currency_cache.compute_with(key, |existing| {
            if existing.as_ref().is_some_and(|existing| {
                !candidate
                    .as_ref()
                    .expect("candidate currency is available")
                    .cache_rank_ge(existing)
            }) {
                return existing.expect("existing currency was checked");
            }

            let resolved = candidate.take().expect("candidate currency is stored once");
            crate::core::logging::trace_debug!(
                symbol,
                kind = ?kind,
                evidence = ?resolved.evidence(),
                "cached resolved currency"
            );
            resolved
        });
    }

    pub(crate) fn cached_resolved_currency(
        &self,
        symbol: &str,
        kind: CurrencyCacheKind,
    ) -> Option<ResolvedCurrency> {
        self.currency_cache
            .get_cloned(&CurrencyCacheKey::new(symbol, kind))
            .map(|resolved| resolved.with_cached_acquisition(kind))
    }
}
