#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_ticker_quote_for_record() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = yfinance_rs::YfClient::builder().build().unwrap();

    for sym in ["AAPL", "MSFT"] {
        let t = yfinance_rs::Ticker::new(&client, sym);
        let q = t.quote().await.unwrap();

        if !crate::common::is_recording() {
            assert_eq!(q.instrument.symbol.as_str(), sym);
            assert!(q.price.is_some() || q.previous_close.is_some());
        }
    }
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_ticker_key_statistics_for_record() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = yfinance_rs::YfClient::builder().build().unwrap();
    let sym = "MSFT";
    let t = yfinance_rs::Ticker::new(&client, sym);
    let stats = t.key_statistics().await.unwrap();

    if crate::common::is_recording() {
        assert!(
            crate::common::fixture_exists(
                crate::common::KEY_STATISTICS_FIXTURE_ENDPOINT,
                sym,
                "json"
            ),
            "recording pass should persist quoteSummary key statistics fixture for {sym}"
        );
        assert!(
            stats.beta.is_some(),
            "recording pass should capture quoteSummary beta for {sym}"
        );
    } else {
        assert!(
            stats.market_cap.is_some() || stats.beta.is_some(),
            "live key statistics lookup for {sym} should return at least one core statistic"
        );
    }
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_ticker_currency_unit_scale_fixtures_for_record() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = yfinance_rs::YfClient::builder().build().unwrap();

    for sym in ["TSCO.L", "SBK.JO", "SAP", "MSFT", "SPY"] {
        let t = yfinance_rs::Ticker::new(&client, sym);
        let stats = t.key_statistics().await.unwrap();

        if crate::common::is_recording() {
            assert!(
                crate::common::fixture_exists("quote_v7", sym, "json"),
                "recording pass should persist quote_v7 fixture for {sym}"
            );
            assert!(
                crate::common::fixture_exists(
                    crate::common::KEY_STATISTICS_FIXTURE_ENDPOINT,
                    sym,
                    "json"
                ),
                "recording pass should persist quoteSummary key statistics fixture for {sym}"
            );
        } else {
            assert!(
                stats.fifty_two_week_high.is_some()
                    || stats.market_cap.is_some()
                    || stats.dividend_per_share_forward.is_some(),
                "live key statistics lookup for {sym} should return scale-relevant fields"
            );
        }
    }
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_ticker_options_for_record() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = yfinance_rs::YfClient::builder().build().unwrap();

    let sym = "AAPL";
    let t = yfinance_rs::Ticker::new(&client, sym);

    let expiries = t.options().await.unwrap();

    if crate::common::is_recording() {
        assert!(
            crate::common::fixture_exists("options_v7", sym, "json"),
            "recording pass should persist options_v7 fixture for {sym}"
        );
    }

    if !crate::common::is_recording() {
        // In live mode (non-recording), we expect Yahoo to return at least one expiry.
        assert!(
            !expiries.is_empty(),
            "live options lookup for {sym} should return expirations"
        );
    }

    if let Some(first) = expiries.first().copied() {
        let chain = t.option_chain(Some(first)).await.unwrap();

        if crate::common::is_recording() {
            let key = format!("{sym}_{first}");
            assert!(
                crate::common::fixture_exists("options_v7", &key, "json"),
                "recording pass should persist dated options_v7 fixture for {key}"
            );
            assert!(
                chain.calls().chain(chain.puts()).next().is_some(),
                "recorded chain for {sym} should include at least one contract"
            );
        }

        if !crate::common::is_recording() {
            // Instead of a useless `>= 0` check on usize, ensure the chain is coherent:
            // every returned contract (if any) must match the requested expiration.
            assert!(
                chain
                    .calls()
                    .chain(chain.puts())
                    .all(|c| c.expiration_at.unwrap().timestamp() == first),
                "all option contracts should match the requested expiration for {sym}"
            );
        }
    }
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_etf_options_preserve_underlying_identity() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = yfinance_rs::YfClient::builder().build().unwrap();
    let sym = "SPY";
    let t = yfinance_rs::Ticker::new(&client, sym);
    let expiries = t.options().await.unwrap();
    let first = expiries
        .first()
        .copied()
        .expect("SPY should have listed option expirations");
    let chain = t.option_chain(Some(first)).await.unwrap();

    if crate::common::is_recording() {
        let key = format!("{sym}_{first}");
        assert!(
            crate::common::fixture_exists("options_v7", &key, "json"),
            "recording pass should persist dated options_v7 fixture for {key}"
        );
    }

    let contract = chain
        .calls()
        .chain(chain.puts())
        .next()
        .expect("SPY option chain should include contracts");
    assert_eq!(contract.key.underlying.symbol.as_str(), sym);
    assert!(matches!(
        &contract.key.underlying.kind,
        paft::domain::AssetKind::Fund
    ));
    assert!(
        contract.key.underlying.exchange.is_some(),
        "Yahoo options quote should include underlying exchange metadata"
    );
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_ticker_shares_for_record() {
    if !crate::common::is_recording() {
        return;
    }
    let client = yfinance_rs::YfClient::builder().build().unwrap();
    let t = yfinance_rs::Ticker::new(&client, "MSFT");
    let annual = t.shares().await.unwrap();
    let quarterly = t.quarterly_shares().await.unwrap();
    assert!(!annual.is_empty());
    assert!(!quarterly.is_empty());
    assert!(crate::common::fixture_exists(
        "timeseries_annualOrdinarySharesNumber",
        "MSFT",
        "json"
    ));
    assert!(crate::common::fixture_exists(
        "timeseries_quarterlyOrdinarySharesNumber",
        "MSFT",
        "json"
    ));
}

#[tokio::test]
#[ignore = "exercise live Yahoo Finance API"]
async fn live_ticker_capital_gains_for_record() {
    if !crate::common::is_recording() {
        return;
    }

    let client = yfinance_rs::YfClient::builder().build().unwrap();
    let t = yfinance_rs::Ticker::new(&client, "VFINX");
    let _ = t.actions(None).await.unwrap();
}
