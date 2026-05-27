use std::marker::PhantomData;

use serde_json::{Map, Value};
use url::Url;

use super::fields::{equity_fields, etf_fields, fund_fields};
use super::predefined::PredefinedScreener;
use super::query::{
    Equity, EquityQuery, Etf, EtfQuery, Fund, FundQuery, Predefined, ResultOffset, ScreenerCount,
    ScreenerQuery, SortDirection, SortField, YahooQuoteType,
};
use super::response::{ScreenerResponse, parse_screener_body};
use crate::core::client::{CacheMode, RetryConfig};
use crate::{YfClient, YfError};

const DEFAULT_SCREENER_BASE: &str = "https://query1.finance.yahoo.com/v1/finance/screener";
const DEFAULT_PREDEFINED_SCREENER_BASE: &str =
    "https://query1.finance.yahoo.com/v1/finance/screener/predefined/saved";

#[derive(Debug, Clone)]
enum RequestKind {
    Predefined(PredefinedScreener),
    Custom {
        query: Value,
        quote_type: YahooQuoteType,
        sort_field: &'static str,
        sort_direction: SortDirection,
    },
}

#[derive(Debug, Clone)]
pub(super) struct CustomParts {
    query: Value,
    quote_type: YahooQuoteType,
    sort_field: &'static str,
    sort_direction: SortDirection,
}

impl CustomParts {
    pub(super) fn equity(
        sort_field: SortField<Equity>,
        sort_direction: SortDirection,
        query: EquityQuery,
    ) -> Self {
        Self {
            query: query.into_wire_value(),
            quote_type: YahooQuoteType::Equity,
            sort_field: sort_field.key(),
            sort_direction,
        }
    }

    pub(super) fn fund(
        sort_field: SortField<Fund>,
        sort_direction: SortDirection,
        query: FundQuery,
    ) -> Self {
        Self {
            query: query.into_wire_value(),
            quote_type: YahooQuoteType::MutualFund,
            sort_field: sort_field.key(),
            sort_direction,
        }
    }

    pub(super) fn etf(
        sort_field: SortField<Etf>,
        sort_direction: SortDirection,
        query: EtfQuery,
    ) -> Self {
        Self {
            query: query.into_wire_value(),
            quote_type: YahooQuoteType::Etf,
            sort_field: sort_field.key(),
            sort_direction,
        }
    }
}

/// Builder for Yahoo Finance screener requests.
#[derive(Debug, Clone)]
pub struct ScreenerBuilder<U = Predefined> {
    client: YfClient,
    screener_base: Url,
    predefined_base: Url,
    kind: RequestKind,
    count: Option<ScreenerCount>,
    offset: Option<ResultOffset>,
    cache_mode: CacheMode,
    retry_override: Option<RetryConfig>,
    marker: PhantomData<U>,
}

impl ScreenerBuilder<Predefined> {
    /// Creates a builder for a predefined Yahoo screener.
    ///
    /// # Panics
    ///
    /// Panics if hardcoded Yahoo screener URLs are invalid.
    #[must_use]
    pub fn predefined(client: &YfClient, screener: PredefinedScreener) -> Self {
        Self {
            client: client.clone(),
            screener_base: Url::parse(DEFAULT_SCREENER_BASE).expect("valid screener URL"),
            predefined_base: Url::parse(DEFAULT_PREDEFINED_SCREENER_BASE)
                .expect("valid predefined screener URL"),
            kind: RequestKind::Predefined(screener),
            count: None,
            offset: None,
            cache_mode: CacheMode::Use,
            retry_override: None,
            marker: PhantomData,
        }
    }

    /// Alias for [`ScreenerBuilder::predefined`].
    #[must_use]
    pub fn new(client: &YfClient, screener: PredefinedScreener) -> Self {
        Self::predefined(client, screener)
    }
}

impl ScreenerBuilder<Equity> {
    /// Creates a builder for a custom equity screener query.
    ///
    /// # Panics
    ///
    /// Panics if hardcoded Yahoo screener URLs are invalid.
    #[must_use]
    pub fn equity(client: &YfClient, query: EquityQuery) -> Self {
        Self::custom(
            client,
            query,
            YahooQuoteType::Equity,
            equity_fields::TICKER.key(),
        )
    }

    /// Sets the custom equity sort field and direction.
    #[must_use]
    pub const fn sort_by(mut self, field: SortField<Equity>, direction: SortDirection) -> Self {
        self.set_sort(field.key(), direction);
        self
    }
}

impl ScreenerBuilder<Fund> {
    /// Creates a builder for a custom mutual fund screener query.
    ///
    /// # Panics
    ///
    /// Panics if hardcoded Yahoo screener URLs are invalid.
    #[must_use]
    pub fn fund(client: &YfClient, query: FundQuery) -> Self {
        Self::custom(
            client,
            query,
            YahooQuoteType::MutualFund,
            fund_fields::TICKER.key(),
        )
    }

    /// Sets the custom mutual fund sort field and direction.
    #[must_use]
    pub const fn sort_by(mut self, field: SortField<Fund>, direction: SortDirection) -> Self {
        self.set_sort(field.key(), direction);
        self
    }
}

impl ScreenerBuilder<Etf> {
    /// Creates a builder for a custom ETF screener query.
    ///
    /// # Panics
    ///
    /// Panics if hardcoded Yahoo screener URLs are invalid.
    #[must_use]
    pub fn etf(client: &YfClient, query: EtfQuery) -> Self {
        Self::custom(client, query, YahooQuoteType::Etf, etf_fields::TICKER.key())
    }

    /// Sets the custom ETF sort field and direction.
    #[must_use]
    pub const fn sort_by(mut self, field: SortField<Etf>, direction: SortDirection) -> Self {
        self.set_sort(field.key(), direction);
        self
    }
}

impl<U: Send + Sync> ScreenerBuilder<U> {
    fn custom(
        client: &YfClient,
        query: ScreenerQuery<U>,
        quote_type: YahooQuoteType,
        default_sort_field: &'static str,
    ) -> Self {
        Self {
            client: client.clone(),
            screener_base: Url::parse(DEFAULT_SCREENER_BASE).expect("valid screener URL"),
            predefined_base: Url::parse(DEFAULT_PREDEFINED_SCREENER_BASE)
                .expect("valid predefined screener URL"),
            kind: RequestKind::Custom {
                query: query.into_wire_value(),
                quote_type,
                sort_field: default_sort_field,
                sort_direction: SortDirection::Desc,
            },
            count: Some(ScreenerCount::DEFAULT),
            offset: Some(ResultOffset::ZERO),
            cache_mode: CacheMode::Use,
            retry_override: None,
            marker: PhantomData,
        }
    }

    /// Overrides the base URL for custom screener POST requests.
    #[must_use]
    pub fn screener_base(mut self, base: Url) -> Self {
        self.screener_base = base;
        self
    }

    /// Overrides the base URL for predefined screener GET requests.
    #[must_use]
    pub fn predefined_screener_base(mut self, base: Url) -> Self {
        self.predefined_base = base;
        self
    }

    /// Sets the cache mode for this request.
    #[must_use]
    pub const fn cache_mode(mut self, mode: CacheMode) -> Self {
        self.cache_mode = mode;
        self
    }

    /// Overrides the retry policy for this request.
    #[must_use]
    pub fn retry_policy(mut self, cfg: Option<RetryConfig>) -> Self {
        self.retry_override = cfg;
        self
    }

    /// Sets the requested row count.
    #[must_use]
    pub const fn count(mut self, count: ScreenerCount) -> Self {
        self.count = Some(count);
        self
    }

    /// Sets the result offset.
    #[must_use]
    pub const fn offset(mut self, offset: ResultOffset) -> Self {
        self.offset = Some(offset);
        self
    }

    const fn set_sort(&mut self, field: &'static str, direction: SortDirection) {
        if let RequestKind::Custom {
            sort_field,
            sort_direction,
            ..
        } = &mut self.kind
        {
            *sort_field = field;
            *sort_direction = direction;
        }
    }

    /// Executes the screener request.
    ///
    /// # Errors
    ///
    /// Returns `YfError` if the network request fails, Yahoo returns an error
    /// status, or the response cannot be parsed.
    pub async fn fetch(self) -> Result<ScreenerResponse, YfError> {
        match self.kind.clone() {
            RequestKind::Predefined(screener) if self.offset.is_none() => {
                self.fetch_predefined_get(screener).await
            }
            RequestKind::Predefined(screener) => {
                let parts = screener.custom_parts()?;
                self.fetch_custom_post(parts).await
            }
            RequestKind::Custom {
                query,
                quote_type,
                sort_field,
                sort_direction,
            } => {
                self.fetch_custom_post(CustomParts {
                    query,
                    quote_type,
                    sort_field,
                    sort_direction,
                })
                .await
            }
        }
    }

    async fn fetch_predefined_get(
        &self,
        screener: PredefinedScreener,
    ) -> Result<ScreenerResponse, YfError> {
        let mut url = self.predefined_base.clone();
        append_common_params(&mut url);
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("scrIds", screener.id());
            if let Some(count) = self.count {
                qp.append_pair("count", &count.get().to_string());
            }
        }

        if self.cache_mode == CacheMode::Use
            && let Some(body) = self.client.cache_get(&url).await
        {
            return parse_screener_body(&body);
        }

        let (body, final_url) = self.get_with_auth_retry(url, screener.id()).await?;
        if self.cache_mode != CacheMode::Bypass {
            self.client.cache_put(&final_url, &body, None).await;
        }
        parse_screener_body(&body)
    }

    async fn fetch_custom_post(&self, parts: CustomParts) -> Result<ScreenerResponse, YfError> {
        let mut url = self.screener_base.clone();
        append_common_params(&mut url);

        let fixture_key = parts.quote_type.as_str().to_ascii_lowercase();
        let body = self.custom_body(parts);
        let response = self.post_with_auth_retry(url, &body, &fixture_key).await?;
        parse_screener_body(&response)
    }

    fn custom_body(&self, parts: CustomParts) -> Value {
        let mut body = Map::new();

        if let Some(offset) = self.offset {
            body.insert("offset".into(), Value::from(offset.get()));
        }
        if let Some(count) = self.count {
            body.insert("count".into(), Value::from(count.get()));
        }

        body.insert(
            "sortField".into(),
            Value::String(parts.sort_field.to_string()),
        );
        body.insert(
            "sortType".into(),
            Value::String(parts.sort_direction.as_str().to_string()),
        );
        body.insert("userId".into(), Value::String(String::new()));
        body.insert("userIdType".into(), Value::String("guid".into()));
        body.insert(
            "quoteType".into(),
            Value::String(parts.quote_type.as_str().into()),
        );
        body.insert("query".into(), parts.query);

        Value::Object(body)
    }

    async fn get_with_auth_retry(
        &self,
        url: Url,
        fixture_key: &str,
    ) -> Result<(String, Url), YfError> {
        let resp = self
            .client
            .send_with_retry(
                self.client
                    .http()
                    .get(url.clone())
                    .header("accept", "application/json"),
                self.retry_override.as_ref(),
            )
            .await?;

        if resp.status().is_success() {
            let body = crate::core::net::get_success_text(
                resp,
                &url,
                "screener_predefined",
                fixture_key,
                "json",
            )
            .await?;
            return Ok((body, url));
        }

        let status = resp.status().as_u16();
        if status != 401 && status != 403 {
            return Err(crate::core::net::status_error_code(status, &url));
        }

        self.client.ensure_credentials().await?;
        let crumb = self
            .client
            .crumb()
            .await
            .ok_or_else(|| YfError::Auth("Crumb is not set".into()))?;

        let mut url2 = url.clone();
        url2.query_pairs_mut().append_pair("crumb", &crumb);
        let resp = self
            .client
            .send_with_retry(
                self.client
                    .http()
                    .get(url2.clone())
                    .header("accept", "application/json"),
                self.retry_override.as_ref(),
            )
            .await?;

        if !resp.status().is_success() {
            return Err(crate::core::net::status_error(resp.status(), &url2));
        }

        let body = crate::core::net::get_success_text(
            resp,
            &url2,
            "screener_predefined",
            fixture_key,
            "json",
        )
        .await?;
        Ok((body, url2))
    }

    async fn post_with_auth_retry(
        &self,
        url: Url,
        body: &Value,
        fixture_key: &str,
    ) -> Result<String, YfError> {
        let resp = self
            .client
            .send_with_retry(
                self.client
                    .http()
                    .post(url.clone())
                    .header("accept", "application/json")
                    .json(body),
                self.retry_override.as_ref(),
            )
            .await?;

        if resp.status().is_success() {
            return crate::core::net::get_success_text(
                resp,
                &url,
                "screener_custom",
                fixture_key,
                "json",
            )
            .await;
        }

        let status = resp.status().as_u16();
        if status != 401 && status != 403 {
            return Err(crate::core::net::status_error_code(status, &url));
        }

        self.client.ensure_credentials().await?;
        let crumb = self
            .client
            .crumb()
            .await
            .ok_or_else(|| YfError::Auth("Crumb is not set".into()))?;

        let mut url2 = url.clone();
        url2.query_pairs_mut().append_pair("crumb", &crumb);
        let resp = self
            .client
            .send_with_retry(
                self.client
                    .http()
                    .post(url2.clone())
                    .header("accept", "application/json")
                    .json(body),
                self.retry_override.as_ref(),
            )
            .await?;

        if !resp.status().is_success() {
            return Err(crate::core::net::status_error(resp.status(), &url2));
        }

        crate::core::net::get_success_text(resp, &url2, "screener_custom", fixture_key, "json")
            .await
    }
}

fn append_common_params(url: &mut Url) {
    let mut qp = url.query_pairs_mut();
    qp.append_pair("corsDomain", "finance.yahoo.com");
    qp.append_pair("formatted", "false");
    qp.append_pair("lang", "en-US");
    qp.append_pair("region", "US");
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::screener::{PercentPoints, Region, equity_fields};

    #[test]
    fn custom_body_matches_python_wire_shape() {
        let client = YfClient::default();
        let query = EquityQuery::and(vec![
            equity_fields::PERCENT_CHANGE.gt(PercentPoints::new(3.0).unwrap()),
            equity_fields::REGION.eq(Region::Us),
        ])
        .unwrap();
        let builder = ScreenerBuilder::equity(&client, query);
        let body = match &builder.kind {
            RequestKind::Custom {
                query,
                quote_type,
                sort_field,
                sort_direction,
            } => builder.custom_body(CustomParts {
                query: query.clone(),
                quote_type: *quote_type,
                sort_field,
                sort_direction: *sort_direction,
            }),
            RequestKind::Predefined(_) => unreachable!(),
        };

        assert_eq!(
            body,
            json!({
                "offset": 0,
                "count": 25,
                "sortField": "ticker",
                "sortType": "DESC",
                "userId": "",
                "userIdType": "guid",
                "quoteType": "EQUITY",
                "query": {
                    "operator": "AND",
                    "operands": [
                        {"operator": "GT", "operands": ["percentchange", 3.0]},
                        {"operator": "EQ", "operands": ["region", "us"]}
                    ]
                }
            })
        );
    }
}
