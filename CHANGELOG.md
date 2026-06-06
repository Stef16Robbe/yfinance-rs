# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0]

### Breaking Changes

- Public builder constructors now consistently borrow `&YfClient`, including
  `QuotesBuilder::new`, and builder execution methods borrow the configured
  builder instead of consuming it.
- `ScreenerNumber` is now an opaque validated value. Floating-point values must
  be constructed with `ScreenerNumber::new`, while integer values still use the
  existing `From` conversions.
- `ProjectionIssue::MissingRequiredFields` now owns `Vec<&'static str>` instead
  of requiring a static field slice.
- `StreamBuilder::start()` is now async. In `StreamMethod::Websocket` mode it waits for the
  initial WebSocket handshake and subscription write, returning startup failures directly.
- `YfError` has new variants for provider-data and retry failures: `InvalidData`, `RequestNotCloneable`, `Money`, and `OptionUnderlyingTypeUnavailable`.
- Removed `HistoryBuilder::keepna` and `DownloadBuilder::keepna`. `paft::Candle` requires valid OHLC prices, so malformed history rows are always dropped instead of fabricating placeholder prices.
- `DownloadBuilder` now rejects simultaneous `auto_adjust(true)` and `back_adjust(true)` with `YfError::InvalidParams`; call `.auto_adjust(false).back_adjust(true)` for back-adjusted downloads.
- History responses now use `paft`'s `price_basis` metadata instead of the old
  `adjusted` boolean. Back-adjusted downloads report adjusted open/high/low and
  raw close as per-field OHLC bases instead of ambiguous adjusted metadata.
- Replaced the old lossy float-to-money/price/decimal helpers with checked conversion helpers. `core::conversions` is now hidden from public docs and remains internal Yahoo-to-`paft` adapter plumbing with no stability guarantee.
- Missing or malformed provider classification/date fields now fail or drop the affected row instead of being coerced into plausible values such as epoch timestamps, `Hold`, `Maintain`, `Buy`, `Officer`, `Equity`, or `1970` periods.
- Missing or unparseable Yahoo currency metadata no longer silently falls back to USD. Required monetary responses now return typed data errors when no valid currency can be resolved, and optional monetary fields/actions are omitted instead of fabricated.
- `CacheMode` now has a policy-driven `Default` mode. Volatile endpoints such as quotes, options, news, and screeners bypass the response cache by default; use `CacheMode::Use` to opt them into caching.
- Removed lossy tuple-based `Ticker::dividends()`, `Ticker::splits()`, and `Ticker::capital_gains()` helpers. Use `Ticker::actions()` and match on typed `Action` variants instead.
- Removed the always-empty `Info::esg_scores` field. Yahoo no longer returns the backing `esgScores` module; use `Ticker::sustainability()` for explicit best-effort ESG requests.
- Removed the legacy HTML scraping fallback for profile lookups. Profiles now load only from Yahoo's quoteSummary API.
- Removed `YfClientBuilder::base_quote()`, which only configured the deleted Yahoo quote-page scraping path.
- `Ticker::fast_info()` now returns yfinance-rs' own `FastInfo` struct with
  instant quote data nested under `snapshot`; existing `fast_info.last`-style
  reads should move to `fast_info.snapshot.last`.
- The public model now follows the `paft` 0.9 shape: quote, snapshot, stream,
  history, option, and book-level price fields use currency-less
  `PriceAmount` values with currency carried by the containing model; stream,
  quote, and history volumes plus book-level sizes use `QuantityAmount`;
  `Candle` prices live under `candle.ohlc`; actions and share/calendar dates
  are calendar dates; and `ReportingPeriod` replaces the old period type.
- `QuoteUpdate::volume` now exposes Yahoo's latest cumulative session volume as
  a `QuantityAmount` instead of a computed per-update delta. The first update is
  no longer forced to `None` when Yahoo sends volume, and streams no longer keep
  reset/rollover state; callers that need deltas or session-boundary policy can
  derive them from successive cumulative values.
- `YfWarning::CurrencyInferred` now reports only diagnostic purpose and
  heuristic inference through `YfCurrencyPurpose` and `YfCurrencyInference`.
  Removed the public `YfCurrencySource` and `YfEvidenceStrength` provenance
  types; provider-backed currency provenance is now an internal resolver detail.
- Growable public enums such as diagnostics, errors, news tabs, and Yahoo
  screener vocabularies are now `#[non_exhaustive]`.

### Added

- Add adapter-level projection diagnostics through `YfResponse<T>`, `YfDiagnostics`, `YfWarning`, `ProjectionIssue`, and `DataQuality`, plus `strict()` and `*_with_diagnostics()` entry points on history, download, holders, fundamentals, analysis, ESG, news, search, and `Ticker::info()`.
- Add fixture-backed coverage for Yahoo's v7 and quoteSummary dividend-yield
  wire conventions, locking both paths to `paft`'s fractional yield model.
- History, holders, fundamentals, analysis, ESG, news, search, download, and aggregate info calls can now distinguish absent optional provider data from present Yahoo data that was dropped or omitted while projecting into strict `paft` values.
- Add `YfWarning::CoercedPresentField` for present provider fields that are represented only after a lossy coercion such as rounding.
- Add projection diagnostics entry points for quotes, fast info, key statistics, option chains, and screeners, including `Ticker::*_with_diagnostics()` methods and `QuotesBuilder::fetch_with_diagnostics()`.
- Add `Ticker::data_quality()` and `Ticker::strict()` so ticker convenience
  methods and builders created from a ticker can use strict projection
  consistently.
- Add explicit share-count windows through `Ticker::shares_between()`,
  `Ticker::quarterly_shares_between()`, and
  `FundamentalsBuilder::shares_between()`/`shares_between_with_diagnostics()`.
- Add `FastInfo::moving_averages` with Yahoo's 50-day and 200-day average
  prices, matching Python yfinance's `fast_info` placement without extending
  `paft::Snapshot`.
- Add `Info::moving_averages` as a sibling of `snapshot` and `key_statistics`,
  so `Ticker::info()` also surfaces Yahoo's `summaryDetail` moving-average
  fields without putting technical indicators in `paft::KeyStatistics`.
- Add `Ticker::profile()` as a high-level convenience method for company, ETF,
  and mutual-fund profiles.
- Add `examples/16_diagnostics_audit.rs`, a live public-surface audit that
  demonstrates checking projection diagnostics and sanity-validating parsed
  Yahoo data across the crate.
- The live diagnostics audit now includes `XRP-USD` quote and stream checks to
  exercise low-price crypto decimal precision.

### Dependencies

- Use the published `paft` 0.9.0 crate instead of tracking the Git `develop`
  branch.

### Fixed

- Batch quotes with diagnostics now report requested symbols that Yahoo omits
  from the v7 response instead of silently returning a shorter quote vector.
- `YfClient::default()` and builder-created clients now apply a 30-second total
  request timeout and a 10-second connect timeout by default, so stalled
  connections fail and can trigger timeout retries instead of hanging forever.
- Listing-currency inference now reaches Yahoo exchange alias fallbacks such as
  `NASDAQ`, `LONDON`, and `FRA` before strict exchange parsing can reject them.
- Cached quoteSummary and fundamentals-timeseries responses are now returned before
  acquiring Yahoo cookie/crumb credentials.
- Polling streams with `diff_only(true)` now advance their last-price filter
  only after a quote update is successfully emitted, so a skipped malformed
  quote cannot suppress a later valid quote at the same price.
- WebSocket quote updates now preserve equity prices when Yahoo omits the
  currency field but includes an exchange code, and they no longer project
  protobuf default zero prices as real monetary values.
- WebSocket quote updates now map known Yahoo numeric stream quote types into
  typed `AssetKind`s instead of using the untyped stream fallback whenever the
  instrument cache is cold.
- Fundamentals timeseries and share-count parsing no longer rejects Yahoo result items that
  omit the unused `meta` object.
- Normal Yahoo diagnostics are reduced for current live responses by preferring chart
  `exchangeTimezoneName`, accepting EPS revision `downLast*Days` casing, skipping
  metadata-only timeseries items, and preserving blank-text no-cash insider exercise rows.
- ETF profile loading now falls back to Yahoo `quoteType: ETF` when
  `fundProfile.legalType` is absent, matching the existing mutual-fund fallback.
- General proxy configuration through `YfClientBuilder::proxy()` and
  `try_proxy()` now applies to Yahoo's HTTPS requests instead of only matching
  plain HTTP URLs.
- `DownloadBuilder` best-effort batches now drop only the symbol whose chart metadata lacks
  a usable instrument kind and report a diagnostic, instead of failing the whole batch.
- `ScreenerNumber` no longer exposes public enum variants that can bypass
  finite-float validation and panic during screener query serialization.
- Text redaction now masks crumb and auth-like query parameters after comma-separated
  Yahoo query values such as quoteSummary modules.
- Historical share-count helpers now request Yahoo's annual/quarterly
  `OrdinarySharesNumber` timeseries instead of the semantically different
  basic-average-shares fields.
- `Ticker::info()` now batches its quoteSummary modules into one request, avoids duplicate `financialData` fetches, and no longer exposes an always-empty ESG field for Yahoo's dead `esgScores` module; use `Ticker::sustainability()` for explicit best-effort ESG requests.
- `Ticker::isin()` now returns typed HTTP status errors for non-success Business Insider
  responses and keeps suffix-qualified symbols distinct while matching ISIN suggestions.
- `Ticker::isin()` now validates ISIN check digits and avoids raw fallback matches that are not tied to the requested symbol.
- Option chains with contracts but no usable Yahoo underlying `quoteType` now fail with `YfError::OptionUnderlyingTypeUnavailable` instead of a generic missing-data error. Empty chains no longer need typed underlying metadata.
- Holder convenience methods now request only the quoteSummary module they project instead of fetching every holder/insider module for each call.
- `StreamMethod::Websocket` startup failures are now returned from `StreamBuilder::start().await`
  instead of being logged in the spawned task while the caller receives `Ok`.
- WebSocket streams now flush pong replies for ping frames and treat remote close/EOF as stream
  failures, allowing `WebsocketWithFallback` to fall back to polling unless the caller stopped it.
- Polling streams now timestamp quote updates with Yahoo's `regularMarketTime` when available
  instead of always using the local polling receive time.
- Streaming quote updates now pass Yahoo cumulative volume through directly
  across WebSocket and polling streams, and untyped stream instrument fallbacks
  no longer poison the client instrument cache.
- WebSocket price conversion now converts Yahoo protobuf `float` values directly
  into decimals, avoiding widened `f64` artifacts such as
  `311.5799865722656`.
- Half-present EPS revision pairs now emit projection diagnostics instead of being silently
  omitted from earnings-trend responses.
- Exponential retry backoff now uses real random jitter instead of a deterministic
  attempt-number formula, avoiding synchronized retries across clients.
- Public retry policies, stream intervals, and user-provided symbols are now validated before
  use, returning `YfError::InvalidParams` for invalid values instead of panicking or issuing
  malformed Yahoo requests.
- Convert malformed Yahoo/user-provided symbols, missing quote symbols, missing currency metadata, and uncloneable retry requests into `Result` errors instead of panicking.
- Surface unavailable Yahoo ESG modules through `ProviderFeatureUnavailable` diagnostics and strict-mode data-quality errors, so missing provider data is not indistinguishable from a valid zero-involvement result for callers that audit projection quality.
- Missing optional quoteSummary feature modules, including earnings, analyst recommendations, price targets, upgrades/downgrades, and holder ownership modules, now return empty data plus `ProviderFeatureUnavailable` diagnostics in best-effort mode and data-quality errors in strict mode.
- Normalize HTTP status handling through shared fetch helpers so quoteSummary and fundamentals-timeseries failures return typed `YfError` variants and are not cached as parseable response bodies.
- Surface Yahoo v7 quote payload errors as `YfError::Api` before treating a null `quoteResponse.result` as missing data.
- Surface Yahoo fundamentals-timeseries payload errors as `YfError::Api` before treating
  null results as valid empty statements or share counts; malformed responses
  with no result now return `YfError::MissingData`.
- Invalid Yahoo floats (`NaN`, infinities, and values that cannot fit the decimal backend) no longer become zero-valued financial data.
- Optional `paft` fields now become `None` when Yahoo supplies an invalid numeric value, while valid sibling records are still preserved.
- Calendar, holder, ESG, and analyst mappers now route present-but-unrepresentable date and decimal fields through projection diagnostics instead of silently omitting them or failing best-effort calls.
- Recommendation, analyst-count, search-exchange, history-timezone, inferred cash-flow, and share-count projections now report present-but-invalid, rounded, or inferred provider values through diagnostics; strict mode rejects those losses instead of silently returning `None` or coerced data.
- Malformed required records, including bad OHLC candles, option contracts with invalid strikes, and invalid dividend/capital-gain amounts, are skipped item-by-item.
- Batch quote projection now skips semantically or structurally malformed quote nodes item-by-item in best-effort mode and reports `DroppedItem` diagnostics, matching search, options, holders, and fundamentals row handling; strict mode still rejects the first malformed quote node.
- Yahoo counter fields such as v7 quote volume/book sizes, screener volume, and option contract volume/open interest now accept numeric strings without dropping otherwise valid nodes; internal v7 quote fetches used by polling streams and currency enrichment now skip malformed quote nodes item-by-item in best-effort mode.
- Option-chain projection now parses contracts item-by-item, so one structurally malformed contract no longer aborts the whole chain in best-effort mode.
- Holder list projections now parse rows item-by-item, so one structurally malformed ownership, insider transaction, or insider roster row no longer aborts valid siblings in best-effort mode.
- Search projection now parses quote results item-by-item, so one structurally malformed search quote no longer aborts valid sibling results in best-effort mode.
- Screener projection now parses quote results item-by-item, so one structurally malformed screener quote no longer aborts valid sibling results in best-effort mode.
- News projection now parses stream entries item-by-item, so one structurally malformed article no longer aborts valid sibling articles in best-effort mode.
- Fundamentals timeseries projections now diagnose malformed values item-by-item instead of dropping every period for the affected field.
- Fundamentals timeseries statement rows are no longer emitted when Yahoo sends present-but-empty `reportedValue` wrappers with no raw value.
- Missing or invalid Yahoo timestamps no longer become Unix epoch/default datetimes in quote, history, news, holder, analyst, calendar, and fundamentals mappings.
- Missing quote/search/screener/download instrument kinds no longer default to equity; provider asset-kind metadata is required where the public model needs an instrument.
- Malformed raw OHLC rows are validated before auto-adjustment, so adjustment math cannot turn invalid Yahoo prices into emitted candles.
- Download rounding and repair now leave values unchanged when conversion fails instead of falling back to zero.
- Download repair now scales OHLC rows atomically, leaving the whole row unchanged if any repaired price cannot be represented.
- Download repair now scales `Candle::close_unadj` alongside OHLC when repairing a row.
- History now uses `chart.meta.currency` for candles and default dividend/capital-gain currency before attempting any inferred fallback, while event-level action currencies override the chart default.
- History best-effort responses now report unresolved candle currency as a dropped-candle diagnostic instead of aborting the whole response.
- Yahoo unit currency codes such as `GBp`, `GBX`, `ZAc`, and `ILA` are normalized to their major ISO currencies; per-share `Price` values are scaled from quote units, while aggregate `Money` values stay in major units.
- v7 quote key statistics now distinguish quote-unit prices from major-unit market cap, financial EPS, and quote-major dividend fields, so minor-unit listings such as `TSCO.L` no longer scale EPS/dividend values by the quote-price unit.
- Holder and insider transaction values now use the symbol's trading major currency instead of reporting currency, matching Yahoo's shares times market-price aggregate values.
- Empty Yahoo quoteSummary numeric wrappers such as ETF `marketCap: {}` now parse as absent optional values instead of suppressing otherwise valid sibling statistics.
- Recorded key-statistics fixtures now lock currency-unit scaling for minor-unit listings, normal USD equities, and funds across v7 quote and quoteSummary backfill paths.
- Recorded v7 quote fixtures now pin Yahoo's asymmetric dividend-yield units: trailing yield arrives as a decimal fraction, while forward yield arrives as percent points.
- Currency enrichment now caches successful empty v7 quote currency responses as confirmed missing and consistently reuses typed contextual currency cache entries.
- Heuristic currency cache entries are now provisional until stronger Yahoo currency fields are confirmed missing, so later direct/enriched evidence can replace stale profile or listing inference.
- Currency listing fallback now infers Yahoo quote units for minor-unit exchanges such as London (`GBp`) instead of assuming major ISO units.
- Fundamentals timeseries statements now use same-payload `currencyCode` values before issuing quote/profile enrichment requests, and invalid provider currency codes surface as data errors instead of falling through to heuristics.
- Yahoo exchange and quote-type vocabulary is now normalized through one shared adapter across quote, fast info, info, search, screener, history, options, stream, and currency-inference paths; valid higher-priority quote exchange metadata no longer produces lower-priority exchange diagnostics or strict-mode failures.
- Fundamentals statement money values and aggregate market caps now parse Yahoo wire numbers directly into decimals, avoiding `f64` precision loss for large integers such as revenues, assets, cash flows, and market capitalizations.
- Fundamentals timeseries statements now process every flattened field in each Yahoo result object instead of silently keeping only one field when Yahoo groups multiple requested fields together.
- Earnings trend rows now validate the required period before currency enrichment, avoiding quote lookups and currency diagnostics for rows that are dropped.
- Analyst estimate row-level currency fields no longer poison symbol-level inferred caches, and optional holder monetary values are omitted when currency cannot be resolved.
- Analyst revenue estimate currency now stays scoped to analyst estimate rows instead of overwriting reporting-currency cache entries.
- Option chains now route missing contract currency through the trading-currency resolver, allowing v7 quote enrichment, listing/exchange inference, and profile enrichment instead of depending on already-converted quote prices.
- Currency projection policy is now shared by analysis, fundamentals, holders, and options: invalid caller overrides stay hard errors, while invalid direct provider currencies, failed enrichment, or unresolved heuristics omit affected best-effort values with diagnostics.
- Currency resolution now lets valid lower-priority provider hints such as quoteSummary reporting currency recover from invalid enriched quote hints, while preserving diagnostics for the malformed hint.
- Centralized Yahoo crumb-auth retries so optional- and required-crumb endpoints clear stale cached crumbs and reacquire credentials when authenticated responses return 401/403, including quote v7, options, search, screener, quoteSummary, and fundamentals-timeseries.
- Crumb-auth retries now evict cached invalid-crumb response bodies, bypass cache reads during the fresh-credential retry, and return an auth error if Yahoo still returns an invalid-crumb body after refresh.
- Yahoo auth now sends the stored cookie explicitly during crumb acquisition and crumb-authenticated requests, so `custom_client(reqwest::Client::new())` no longer depends on `reqwest` cookie storage.
- Crumb acquisition now rejects non-success HTTP statuses before reading the body and trims successful crumb bodies before caching them.
- Status errors, HTTP-client errors, and tracing URL fields now redact crumb and auth-like query parameters before formatting.
- Build Yahoo symbol path URLs with one percent-encoding helper instead of `Url::join`, preventing symbols containing URL syntax from changing the request target.
- Expired URL cache entries are now pruned opportunistically on cache reads and writes.
- Crumb refresh now relies on exact response-cache key eviction instead of an unreachable sweep for crumb-bearing cache keys.
- `Ticker`-level cache and retry settings now propagate consistently through history builders, action helpers, and profile loading inside `Ticker::info()`.
- Caller-supplied currency overrides no longer emit `CurrencyInferred` diagnostics or fail strict-mode projection.
- Provider-backed quote, quoteSummary, direct, override, and cached currency
  evidence no longer emits `CurrencyInferred` diagnostics or fails strict-mode
  projection; only listing/profile-country heuristics are diagnostic inferred
  currency fallbacks.
- Currency evidence ranking now treats caller-supplied overrides as stronger
  than provider and heuristic evidence.
- POST endpoints with `cache_mode(CacheMode::Use)`, including news and custom screeners, now use body-aware response cache keys instead of effectively bypassing or colliding on URL-only keys.
- Fundamentals timeseries and share-count default windows now round their implicit end to the next UTC midnight, keeping response-cache keys stable within a day.
- Profile loading now maps Yahoo `MUTUALFUND` quote types into `FundProfile` instead of rejecting them despite fund support being documented.
- Quote, fast-info, key-statistics, option-chain, and screener projections now report present prices, market caps, strikes, and related fields that cannot be represented because currency metadata is missing or invalid instead of silently returning `None` or dropping contracts.
- Quote exchange, market-state, timestamp, analyst currency-source, and optional upgrade/downgrade grade/action projection losses now emit diagnostics consistently; strict mode rejects those present malformed provider fields instead of silently omitting them or dropping otherwise valid rows.
- Options endpoints now surface Yahoo `optionChain.error` payloads as `YfError::Api` instead of reporting them as empty option results.
- Recommendation summaries now populate `mean_rating_text` from Yahoo's `recommendationKey`.
- Best-effort projection policy is now applied uniformly to statement currency conflicts, invalid history chart currencies, and single-ticker v7 quote node type drift instead of surfacing raw typed-adapter errors before diagnostics can record the affected item.
- Fundamentals timeseries currency evidence now prefers the first valid same-payload
  `currencyCode` and compares parsed currency units, so invalid earlier codes or
  equivalent Yahoo unit aliases do not cause valid statement values to be omitted
  as conflicts.
- Country/currency inference rules are now covered by an invariant test that
  parses every configured currency code and forces both lazy lookup tables.
- Profile-country currency inference now normalizes configured country aliases
  before exact and fuzzy lookup table construction, so punctuated country names
  such as `Timor-Leste` resolve correctly.

### Changed

- `DownloadBuilder` now caps per-symbol history fetch concurrency to 8 requests by default and exposes `DownloadConcurrency` plus `DownloadBuilder::concurrency()` for callers that need a different limit.
- Centralize repeated per-call builder option setters through one internal macro.
- Clean up example output so holder rows, corporate actions, historical action dates, and handled live Yahoo errors render as user-facing text instead of debug-shaped values.
- Declare Rust 1.91 as the crate MSRV and enable direct Tokio `sync` and `time` features used by the crate.
- Update ESG examples/docs to handle Yahoo's currently unavailable `esgScores` response instead of advertising unavailable live data.
- README examples no longer advertise direct use of conversion helpers such as `money_to_f64`.
- Build-time protobuf generation now uses a vendored `protoc` binary instead of relying on a system installation.
- Currency auto-resolution is now source-aware and typed by purpose (`Trading`, `Reporting`, `CorporateAction`, and `AnalystEstimate`). Direct Yahoo evidence wins over quote/quoteSummary enrichment, listing inference, and profile-country heuristics.
- Profile-country currency inference now uses one country/currency alias table for exact and fuzzy matches instead of maintaining a duplicate contains-based fallback.
- Internal currency resolution now uses purpose-specific evidence types rather than a generic raw currency-code argument, reducing the chance that endpoint-specific fields mutate the wrong contextual cache.
- `None` currency overrides continue to auto-enrich by querying Yahoo for stronger currency evidence when an endpoint omits currency data. `Some(currency)` overrides remain per-call only and no longer mutate inferred currency caches.
- `YfClient::clear_cache()` now clears URL response cache, currency hint cache, resolved currency cache, and instrument cache; `invalidate_cache_entry()` remains URL-cache only.
- In-memory response caching now has per-endpoint TTL overrides through `YfClientBuilder::cache_ttl_for`, a default 1024-entry cap, and least-recently-used eviction via `YfClientBuilder::cache_max_entries`.
- Simplify fundamentals statement and analyst earnings-trend projection internals without changing the public API.
- Extract shared internal projection helpers for required fields, optional parser diagnostics, and optional value projection; analyst estimate currencies are now resolved once per direct-code group instead of per row.
- CI now covers `main` and `develop` with separate MSRV, formatting, lint, offline-test, and package dry-run jobs, while Yahoo live smoke testing runs in a separate non-required workflow.
- The crates.io publish job now requires the protected `crates-io` GitHub Actions environment, and CI action pins have been refreshed.
- Published crate packages now exclude repository workflow metadata and tracked macOS editor artifacts.
- Internal debug diagnostics now use the optional `tracing` feature consistently instead of `YF_DEBUG`-gated stderr output.
- Consolidate per-call cache, retry, and data-quality plumbing behind a shared internal call-options struct.

## [0.8.0] - 2026-05-27

### Breaking Changes

- Bump the crate to `0.8.0` and move the public API onto the `paft` 0.8 model. This is a breaking release for consumers that construct or destructure exported `paft` types directly.
- Per-unit quoted values now use `paft::money::Price` where `paft` 0.8 does so. This affects quotes, snapshots/fast-info, history candles, option prices and strikes, analyst EPS/price-target values, and action dividend/capital-gain amounts. Settled totals, such as market cap, remain `Money`.
- `Ticker::info()` now returns a composed, structured `Info` rather than a flat market/fundamentals struct. Market snapshot fields live under `info.snapshot`, statistics live under `info.key_statistics`, and optional modules live under `info.profile`, `info.calendar`, `info.price_target`, `info.recommendation_summary`, and `info.esg_scores`.
- Quote, search, option, and stream payloads now prefer `paft::domain::Instrument` over raw symbol fields where the updated `paft` model does so. Download entries already carried instruments, but the updated `paft` model changes access to public fields such as `entry.instrument.symbol`.
- Historical split actions now expose non-zero split ratios through `std::num::NonZeroU32` numerator and denominator fields.
- `Ticker::fast_info()` returns `paft::aggregates::Snapshot`, keeping it strictly scoped to instant-in-time quote data (`last`, `previous_close`, `open`, `day_high`, `day_low`, `volume`, and market state). Exchange identity now lives on `snapshot.instrument.exchange`, and currency is carried by the price fields.

### Added

- Add strongly typed Yahoo Finance screeners:
  - `PredefinedScreener`, `screen`, and `ScreenerBuilder` for predefined Yahoo screeners;
  - `EquityQuery`, `FundQuery`, and `EtfQuery` for custom typed screener queries;
  - closed field/value vocabularies, typed operators, bounded count/offset values, finite numeric values, and percent-point filters;
  - `ScreenerResponse` and `ScreenerResult` with paft identity parsing where possible and preserved Yahoo-specific extra fields.
- Add `Ticker::key_statistics()` and re-export `KeyStatistics` from the crate root.
- Re-export common `paft::domain` types from the crate root: `AssetKind`, `Exchange`,
  `Instrument`, `MarketState`, `Period`, and `Symbol`.
- Map more Yahoo v7 quote fields into provider-agnostic `paft` models: bid/ask top-of-book levels, regular-market open/high/low/time, market cap, shares outstanding, trailing EPS, trailing PE, dividend rate/yields, 52-week high/low, and three-month average volume.
- Expand financial statement mappings from Yahoo fundamentals-timeseries:
  - income statement: interest expense, income tax expense, depreciation and amortization;
  - balance sheet: current assets/liabilities, accounts receivable, inventory, accounts payable, net PPE, goodwill, and intangible assets excluding goodwill;
  - cash flow: depreciation and amortization.
- Add tests covering the new quote, key-statistics, fast-info, statement, and DataFrame conversion paths.

### Changed

- Align `Ticker::info()` more closely with Python yfinance's broad `info` intent while keeping the output grouped by `paft`'s provider-agnostic models instead of mirroring Yahoo's raw response shape.
- Treat optional `info()` submodules as best-effort: a valid v7 quote remains the required core, while profile, calendar, analyst, and ESG modules are included when available.
- Carry the existing calendar dividend-date semantics into the new composed info/key-statistics model: Yahoo `calendarEvents.exDividendDate` maps to `Calendar.ex_dividend_date`, Yahoo `calendarEvents.dividendDate` maps to `Calendar.dividend_payment_date`, and v7 `dividendDate` is used only as a fallback payment date. `KeyStatistics.ex_dividend_date` remains unset unless the upstream data is actually an ex-dividend date.
- Convert ratio and percentage-like analysis, ESG, holder, and quote statistics to `paft::Decimal` where the updated `paft` API expects decimal values.
- Extend range and interval conversion support for the additional variants provided by the updated `paft` market request model.
- Map options into the `paft` 0.8 option model with `OptionContractKey`, `OptionSide`, `contract_instrument`, and a single `contracts` list exposed through `calls()` and `puts()` iterators.

### Fixed

- Stop split-adjusting Yahoo chart volumes during historical `auto_adjust`; Python yfinance adjusts OHLC prices but leaves reported volume unchanged. This avoids double-adjusting already split-adjusted volumes around events such as NVDA's 2024-06-10 10:1 split.
- Tolerate fractional Yahoo split numerator/denominator values in history responses by normalizing them into gcd-simplified non-zero split actions. Oversized normalized pairs are skipped instead of aborting the whole history response.
- Emit the current cumulative day volume as the first stream volume delta after a detected reset/rollover, instead of emitting `None`; the first observed tick and the stateless WebSocket decoder still emit `None`.
- Prefer Yahoo long names over short names for search results, quotes, and fast-info snapshots
  when both names are available.
- Preserve option-chain underlying identity from Yahoo's options response metadata, including ETF/fund quote types and exchange context, instead of falling back to an equity-only request-symbol instrument.
- Populate `Ticker::key_statistics()` and `info.key_statistics` fallbacks from quoteSummary `summaryDetail`/`defaultKeyStatistics`, including beta, ex-dividend date, market cap, shares outstanding, trailing EPS/PE, dividends, 52-week range, and average volume when the v7 quote response omits them.

### Dependencies

- Bump `paft` from crates.io `0.7.1` to crates.io `0.8.0`.
- Bump Polars support to `0.53`.
- Set the `chrono` dependency to `0.4.41` for compatibility with the updated dependency graph.

### Migration Notes

- Replace flat `info` reads with the appropriate group, for example `info.symbol` to `info.snapshot.instrument.symbol`, `info.exchange` to `info.snapshot.instrument.exchange`, `info.last` to `info.snapshot.last`, `info.volume` to `info.snapshot.volume`, and `info.market_cap` to `info.key_statistics.market_cap`.
- Read dividend payment dates from `info.calendar.as_ref().and_then(|c| c.dividend_payment_date)` and ex-dividend dates from `info.calendar.as_ref().and_then(|c| c.ex_dividend_date)`.
- New `quote.bid` and `quote.ask` fields are `Option<BookLevel>`; read prices through `level.price` and sizes through `level.size`.
- Access instrument symbols through public fields, for example `quote.instrument.symbol.as_str()`, `search_result.instrument.symbol.as_str()`, `download_entry.instrument.symbol.as_str()`, and `update.instrument.symbol.as_str()`.
- Read split action ratios with `numerator.get()` and `denominator.get()` after matching `Action::Split`.
- Option chains now expose `contracts` plus `calls()`/`puts()` iterators. Contract economic identity lives under `contract.key`, while Yahoo's contract symbol is available through `contract.contract_instrument.as_ref().map(|i| i.symbol.as_str())`.

## [0.7.2] - 2025-10-31

### Dependencies

- Bump `paft` to `v0.7.1`.

### Note

Yahoo Finance appears to have removed or relocated the ESG data endpoint. As a result, `ticker.sustainability()` currently panics during normal usage and live testing. This issue is under investigation.

## [0.7.1] - 2025-10-30

### Fixed

- Format fundamentals timeseries statement row period from epoch to YYYY-MM-DD.
- Correct `calendarEvents` mapping and extraction for `exDividendDate` and `dividendDate`.
- Correct gross profit and operating income in income statement.

## [0.7.0] - 2025-10-28

### Added

- Per-update volume deltas in real-time streaming: `QuoteUpdate.volume` now reflects the delta
  since the previous update for a symbol. First tick per symbol and after a detected reset/rollover
  yields `None`. Applies to both WebSocket and HTTP polling streams.
- Expose intraday cumulative volume on snapshots: populate `Quote.day_volume` from v7 quotes and
  surface it on convenience types (`Ticker::quote()` and `Ticker::info()` as `Info.volume`).
- SearchBuilder accessors: `lang_ref()` and `region_ref()` to inspect configured parameters.
- Populate convenience `Info` with analytics and ESG when available: `price_target`,
  `recommendation_summary`, `esg_scores`.

### Breaking Change

- Upgrade to `paft` v0.7.0 adds a new field to `paft::market::quote::QuoteUpdate`:
  `volume: Option<u64>`. If you construct or exhaustively destructure `QuoteUpdate`, update your
  code to include the new field or use `..`. Stream APIs and typical consumers that only read
  updates are unaffected.

### Changed

- Stream volume semantics: WebSocket and polling streams compute per-update volume deltas. The
  low-level decoder helper remains stateless and always returns `volume = None`.
- Polling stream `diff_only` now emits when either price or volume changes.

### Documentation

- README: added a "Volume semantics" section for streaming; clarified delta behavior and how to
  obtain cumulative volume.
- Examples: updated streaming and convenience examples to display volume; SearchBuilder example now
  demonstrates `lang_ref()`/`region_ref()`.

### Dependencies

- Bump `paft` to `v0.7.0`.

## [0.6.1] - 2025-10-27

### Fixed

- Fixed critical timestamp interpretation bug in WebSocket stream processing: use `DateTime::from_timestamp_millis()` instead of `i64_to_datetime()` to correctly interpret millisecond timestamps, preventing incorrect date values in quote updates

#### Notes

- **WebSocket Stream Timestamps:** Users may occasionally observe `QuoteUpdate` messages arriving via the WebSocket stream with timestamps that are older than previously received messages ("time traveling ticks"), sometimes by significant amounts (minutes or hours). This behavior appears to originate from the **Yahoo Finance data feed itself** and is not a bug introduced by `yfinance-rs`. To provide the most direct representation of the source data, `yfinance-rs` **does not automatically filter** these out-of-order messages. Applications requiring strictly chronological quote updates should implement their own filtering logic based on the timestamp (`ts`) field of the received `QuoteUpdate`.

## [0.6.0] - 2025-10-21

### Breaking Change

- `DownloadBuilder::run()` now returns `paft::market::responses::download::DownloadResponse` with an `entries: Vec<DownloadEntry>` instead of the previous `DownloadResult` maps. Access candles via `entry.history.candles` and the symbol via `entry.instrument.symbol_str()`.

### Changed

- Re-export `DownloadEntry` and `DownloadResponse` at the crate root for convenient imports.
- Examples and tests updated to iterate over `entries` rather than `series`.

### Performance

- Introduced an instrument cache in `YfClient` and populate it opportunistically from v7 quote responses to reduce symbol resolution overhead during multi-symbol downloads.

### Documentation

- Updated README examples to reflect the new `DownloadResponse.entries` usage.

### Dependencies

- Bump `paft` to `v0.6.0`.

## [0.5.2] - 2025-10-20

### Added

- Optional `tracing` feature: emits spans and key events across network I/O and major logical boundaries. Instrumented `send_with_retry`, profile fallback, quote summary fetch (including invalid crumb retry), history `fetch_full`, and `Ticker` public APIs (`info`, `quote`, `history`, etc.). Disabled by default; zero overhead when not enabled.
- Optional `tracing-subscriber` feature (dev/testing): convenience initializer `init_tracing_for_tests()` to set up a basic subscriber in examples/tests. The library itself does not configure a subscriber.

### Dependencies

- Bump `paft` to `v0.5.2`.

### Docs

- Readme now includes a "Tracing" section.

## [0.5.1] - 2025-10-17

### Changed

- Updated to paft v0.5.1

## [0.5.0] - 2025-10-16

### Breaking

- Adopted `paft` 0.5.0 identity and money types across search, streaming, and ticker info. `Quote.symbol`, `SearchResult.symbol`, `OptionContract.contract_symbol`, and `QuoteUpdate.symbol` now use `paft::domain::Symbol`; values are uppercased and validated during construction, and invalid search results are dropped.
- `Ticker::Info` now re-exports `paft::aggregates::Info`. The previous struct with raw strings and floats has been removed, and fields such as `sector`, `industry`, analyst targets, recommendation metrics, and ESG scores are no longer populated on this convenience type. Monetary and exchange data now use `Money`, `Currency`, `Exchange`, and `MarketState`.
- Real-time streaming emits `paft::market::quote::QuoteUpdate`. `last_price` is renamed to `price` and now carries `Money` (with embedded currency metadata), the standalone `currency` string is gone, and `ts` is now a `DateTime<Utc>`. Update stream consumers accordingly.
- Search now returns `paft::market::responses::search::SearchResponse` with a `results` list. Each item exposes `Symbol`, `AssetKind`, and `Exchange` enums. Replace usages of `resp.quotes` and `quote.longname/shortname` with `resp.results` and `result.name`.

### Changed

- Bumped `paft` to 0.5.0 via the workspace checkout and aligned with the new symbol validation.
- Updated dependencies and fixtures: `reqwest 0.12.24`, `tokio 1.48`.

### Documentation

- Added troubleshooting guidance for consent-related errors in `README.md` (thanks to [@hrishim](https://github.com/hrishim) for the contribution!)
- Expanded `CONTRIBUTING.md` with `just` helpers and clarified repository setup.

### Internal

- Added `.github/FUNDING.yml` to advertise GitHub Sponsors support.
- Removed stray `.DS_Store` files and regenerated fixtures for the new models.

### Migration notes

- Symbols are now uppercase-validated `paft::domain::Symbol`. Use `.as_str()` for string comparisons or construct values with `Symbol::new("AAPL")` (handle the `Result` when user input is dynamic).
- Stream updates now expose `update.price` (`Money`) and `update.ts: DateTime<Utc>`. Replace direct `last_price`/`ts` usage with the new typed fields and derive primitive values as needed.
- Search responses provide `resp.results` instead of `resp.quotes`. Access display data via `result.name`, `result.kind`, and `result.exchange`.
- The convenience info snapshot no longer embeds fundamentals, analyst, or ESG data. Fetch those via `profile::load_profile`, `analysis::AnalysisBuilder`, and `esg::EsgBuilder` if you still need them.

---

## [0.4.0] - 2025-10-12

### Added

- Enabled `paft` facade `aggregates` feature.
  - `Ticker::fast_info()` now returns `paft_aggregates::FastInfo` (typed enums and `Money`), offering a richer, consistent snapshot model.
- Options models expanded (re-exported from `paft-market`):
  - `OptionContract` gains `expiration_date` (NaiveDate), `expiration_at` (Option<DateTime\<Utc>>), `last_trade_at` (Option<DateTime\<Utc>>), and `greeks` (Option\<OptionGreeks>).
- DataFrame support for options types is available when enabling this crate’s `dataframe` feature (forwards to `paft/dataframe`).

### Changed

- History response alignment with `paft` 0.4.0:
  - `Candle` now carries `close_unadj: Option<Money>` (original unadjusted close, when available).
  - `HistoryResponse` no longer includes a top-level `unadjusted_close` vector.
- Examples and tests updated to use Money-typed values and typed enums (Exchange, MarketState, Currency).

### Breaking

- Fast Info return type changed:
  - Old: struct with `last_price: f64`, `previous_close: Option<f64>`, string-y `currency`/`exchange`/`market_state`.
  - New: `paft_aggregates::FastInfo` with `last: Option<Money>`, `previous_close: Option<Money>`, `currency: Option<paft_money::Currency>`, `exchange: Option<paft_domain::Exchange>`, `market_state: Option<paft_domain::MarketState>`, plus `name: Option<String>`.
- Options contract fields changed:
  - Old: `OptionContract { ..., expiration: DateTime<Utc>, ... }`
  - New: `OptionContract { ..., expiration_date: NaiveDate, expiration_at: Option<DateTime<Utc>>, last_trade_at: Option<DateTime<Utc>>, greeks: Option<OptionGreeks>, ... }`
- History unadjusted close location changed:
  - Old: `HistoryResponse { ..., unadjusted_close: Option<Vec<Money>> }`
  - New: `Candle { ..., close_unadj: Option<Money> }` (per-candle).

### Migration notes

- Fast Info
  - Price as f64: replace `fi.last_price` with `fi.last.as_ref().map(money_to_f64).or_else(|| fi.previous_close.as_ref().map(money_to_f64))`.
  - Currency string: replace `fi.currency` (String) with `fi.currency.map(|c| c.to_string())`.
  - Exchange/MarketState strings: `.map(|e| e.to_string())`.
- Options
  - Replace usages of `contract.expiration` with `contract.expiration_at.unwrap_or_else(|| ...)`, or use `contract.expiration_date` for calendar-only logic.
  - New optional fields `last_trade_at` and `greeks` are available (greeks currently not populated from Yahoo v7).
- History
  - Replace `resp.unadjusted_close[i]` with `resp.candles[i].close_unadj.as_ref()`.

### Internal

- Tests updated for `httpmock` 0.8 API changes.
- Lints and examples adjusted for Money/typed enums.

## [0.3.2] - 2025-10-03

### Changed

- Bump `paft` to 0.3.2 (docs-only upstream release; no functional impact).

## [0.3.1] - 2025-10-02

### Changed

- Internal migration to `paft` 0.3.0 without changing the public API surface.
  - Switched internal imports to `paft::domain` (domain types) and `paft::money` (money/currency).
  - Updated internal `Money` construction to the new `Result`-returning API and replaced scalar ops with `try_mul` where appropriate.
- Examples and docs now import DataFrame traits from `paft::prelude::{ToDataFrame, ToDataFrameVec}`.
- Conversion helpers in `core::conversions` now document potential panics if a non-ISO currency lacks registered metadata (behavior aligned with `paft-money`).
- Profile ISIN fields now validate ISIN format using `paft::domain::Isin` - invalid ISINs are filtered out and stored as `None`.
- Updated tokio-tungstenite to version 0.28

## [0.3.0] - 2025-09-20

### Changed

- Migrated to `paft` 0.2.0 with explicit module paths; removed all `paft::prelude` imports across the codebase, tests, and examples.
- Updated enum/string conversions to use `FromStr/TryFrom` parsing from `paft` 0.2.0 (e.g., `MarketState`, `Exchange`, `Period`, insider/transaction/recommendation types).
- Adjusted `Money` operations to use `try_*` methods and made conversions more robust against non-finite values.
- Consolidated public re-exports under `core::models` (e.g., `Interval`, `Range`, `Quote`, `Action`, `Candle`, `HistoryMeta`, `HistoryResponse`) to provide stable, explicit paths.
- Simplified the Polars example behind the `dataframe` feature to avoid prelude usage and to compile cleanly with the new APIs.

### Fixed

- Updated examples and tests to import `Interval`/`Range` from `yfinance_rs::core` explicitly and to avoid wildcard matches in pattern tests.

### Notes

- This release removes reliance on `paft` preludes and may require users to update imports to explicit module paths if depending on re-exported paft items directly.

## [0.2.1] - 2025-09-18

### Added

- Profile-based reporting currency inference with per-symbol caching. The client now inspects the profile country on first use to determine an appropriate currency and reuses that decision across fundamentals and analysis calls.
- ESG involvement exposure: `Ticker::sustainability()` now returns involvement flags (e.g., tobacco, thermal_coal) alongside component scores via `EsgSummary`.

### Changed

- **Breaking change:** `Ticker` convenience methods for fundamentals and analysis (and their corresponding builders) now accept an extra `Option<Currency>` argument. Pass `None` to use the inferred reporting currency, or `Some(currency)` to override the heuristic explicitly.
- **Breaking change:** `Ticker::sustainability()` and `esg::EsgBuilder::fetch()` now return `EsgSummary` instead of `EsgScores`. Access component values via `summary.scores` and involvement via `summary.involvement`.

## [0.2.0] - 2025-09-16

### Added

- New optional `dataframe` feature: all `paft` data models now support `.to_dataframe()` when the feature is enabled, returning Polars `DataFrame`s. Added example `14_polars_dataframes.rs` and README section.
- Custom HTTP client support via `YfClient::builder().custom_client(...)` for full control over `reqwest` configuration.
- Proxy configuration helpers on the client builder: `.proxy()`, `.https_proxy()`, `.try_proxy()`, `.try_https_proxy()`. Added example `13_custom_client_and_proxy.rs`.
- Explicit `User-Agent` is set on all HTTP/WebSocket requests by default, with `.user_agent(...)` to customize it.
- Improved numeric precision in historical adjustments and conversions using `rust_decimal`.

### Changed

- **Breaking change:** All public data models (such as `Quote`, `HistoryBar`, `EarningsTrendRow`, etc.) now use types from the [`paft`](https://crates.io/crates/paft) crate instead of custom-defined structs. This unifies data structures with other financial Rust libraries and improves interoperability, but may require code changes for downstream users.
- Monetary value handling now uses `paft::Money` with currency awareness across APIs and helpers.
- Consolidated and simplified fundamentals timeseries fetching via a generic helper for consistency.
- Error handling refined: `YfError` variants and messages standardized for 404/429/5xx and unexpected statuses.
- Dependencies updated and internal structure adjusted to support the new features.

### Fixed

- Minor clippy findings and documentation typos.

### Known Issues

- Currency inference relies on company profile metadata. If Yahoo omits or mislabels the headquarters country, the inferred currency can still be incorrect—use the new override parameter to force a specific currency in that case.

## [0.1.3] - 2025-08-31

### Added

- Re-exported `CacheMode` and `RetryConfig` from the `core` module.

### Changed

- `Ticker::new` now takes `&YfClient` instead of taking ownership.
- `SearchBuilder` now takes `&YfClient` instead of taking ownership.

## [0.1.2] - 2025-08-30

### Added

- New examples: `10_convenience_methods.rs`, `11_builder_configuration.rs`, `12_advanced_client.rs`.
- Development tooling: `just` recipes `lint`, `lint-fix`, and `lint-strict`.
- Re-exported `YfClientBuilder` at the crate root (`use yfinance_rs::YfClientBuilder`).

### Changed

- Centralized raw wire types (e.g., `RawNum`) into `src/core/wire.rs`.
- Gated debug file dumps behind the `debug-dumps` feature flag.

### Fixed

- Analyst recommendations now read from `financialData` instead of the incorrect `recommendationMean` field.
- Fixed unnecessary mutable borrow in `StreamBuilder` `run_websocket_stream`

## [0.1.1] - 2025-08-28

### Added

- `ticker.earnings_trend()` for analyst earnings and revenue estimates.
- `ticker.shares()` and `ticker.quarterly_shares()` for historical shares outstanding.
- `ticker.capital_gains()` and inclusion of capital gains in `ticker.actions()`.
- Documentation: added doc comments for `EarningsTrendRow`, `ShareCount`, and `Action::CapitalGain`.

## [0.1.0] - 2025-08-27

### Added

- Initial release of `yfinance-rs`.
- Core functionality: `info`, `history`, `quote`, `fast_info`.
- Advanced data: `options`, `option_chain`, `news`, `income_stmt`, `balance_sheet`, `cashflow`.
- Analysis tools: `recommendations`, `sustainability`, `major_holders`, `institutional_holders`.
- Utilities: `DownloadBuilder`, `StreamBuilder`, `SearchBuilder`.

[Unreleased]: https://github.com/gramistella/yfinance-rs/compare/v0.9.0...HEAD
[0.9.0]: https://github.com/gramistella/yfinance-rs/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/gramistella/yfinance-rs/compare/v0.7.2...v0.8.0
[0.7.2]: https://github.com/gramistella/yfinance-rs/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/gramistella/yfinance-rs/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/gramistella/yfinance-rs/compare/v0.6.1...v0.7.0
[0.6.1]: https://github.com/gramistella/yfinance-rs/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/gramistella/yfinance-rs/compare/v0.5.2...v0.6.0
[0.5.2]: https://github.com/gramistella/yfinance-rs/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/gramistella/yfinance-rs/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/gramistella/yfinance-rs/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/gramistella/yfinance-rs/compare/v0.3.1...v0.4.0
[0.3.2]: https://github.com/gramistella/yfinance-rs/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/gramistella/yfinance-rs/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/gramistella/yfinance-rs/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/gramistella/yfinance-rs/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/gramistella/yfinance-rs/compare/v0.1.3...v0.2.0
[0.1.3]: https://github.com/gramistella/yfinance-rs/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/gramistella/yfinance-rs/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/gramistella/yfinance-rs/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/gramistella/yfinance-rs/releases/tag/v0.1.0
