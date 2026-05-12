use yfinance_rs::{
    EquityQuery, PredefinedScreener, Region, ResultOffset, ScreenerBuilder, ScreenerCount,
    YfClient, equity_fields, screen,
};

#[tokio::test]
#[ignore = "hits live Yahoo Finance screener endpoints"]
async fn live_predefined_and_custom_smoke() {
    if !crate::common::live_or_record_enabled() {
        return;
    }

    let client = YfClient::default();
    let predefined = screen(&client, PredefinedScreener::DayGainers)
        .await
        .unwrap();
    assert!(!predefined.results.is_empty());

    let query = EquityQuery::and(vec![
        equity_fields::REGION.eq(Region::Us),
        equity_fields::INTRADAY_PRICE.gt(0),
    ])
    .unwrap();
    let custom = ScreenerBuilder::equity(&client, query)
        .count(ScreenerCount::new(5).unwrap())
        .offset(ResultOffset::new(0))
        .fetch()
        .await
        .unwrap();
    assert!(!custom.results.is_empty());
}
