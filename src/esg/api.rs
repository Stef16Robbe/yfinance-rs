use crate::{
    core::{
        DataQuality, ProjectionContext, ProjectionIssue, YfClient, YfError, YfResponse,
        client::{CacheMode, RetryConfig},
        diagnostics::optional_decimal_f64,
        quotesummary,
        wire::from_raw,
    },
    esg::wire::V10Result,
};
use paft::fundamentals::esg::{EsgInvolvement, EsgScores, EsgSummary};

pub(super) async fn fetch_esg_scores(
    client: &YfClient,
    symbol: &str,
    cache_mode: CacheMode,
    retry_override: Option<&RetryConfig>,
    data_quality: DataQuality,
) -> Result<YfResponse<EsgSummary>, YfError> {
    let mut ctx = ProjectionContext::new("esg", data_quality);
    let root: V10Result = quotesummary::fetch_module_result(
        client,
        symbol,
        "esgScores",
        "esg",
        cache_mode,
        retry_override,
    )
    .await?;

    let Some(esg) = root.esg_scores else {
        ctx.provider_feature_unavailable(
            "esgScores",
            ProjectionIssue::ProviderUnavailable {
                feature: "esgScores",
            },
        )?;
        return Ok(ctx.finish(EsgSummary {
            scores: None,
            involvement: Vec::new(),
        }));
    };

    // Map to paft types: paft::fundamentals::EsgScores now has only environmental/social/governance.
    let scores = EsgScores {
        environmental: optional_decimal_f64(
            &mut ctx,
            "esgScores.environmentScore",
            Some(symbol.to_string()),
            from_raw(esg.environment_score),
            "ESG score",
        )?,
        social: optional_decimal_f64(
            &mut ctx,
            "esgScores.socialScore",
            Some(symbol.to_string()),
            from_raw(esg.social_score),
            "ESG score",
        )?,
        governance: optional_decimal_f64(
            &mut ctx,
            "esgScores.governanceScore",
            Some(symbol.to_string()),
            from_raw(esg.governance_score),
            "ESG score",
        )?,
    };

    // Collect involvement booleans as individual entries with simple categories.
    let mut involvement: Vec<EsgInvolvement> = Vec::new();
    let mut push_flag = |name: &str, val: Option<bool>| {
        let Some(val) = val else {
            return;
        };
        if val {
            involvement.push(EsgInvolvement {
                category: name.to_string(),
                score: None,
            });
        }
    };
    push_flag("adult", esg.adult);
    push_flag("alcoholic", esg.alcoholic);
    push_flag("animal_testing", esg.animal_testing);
    push_flag("catholic", esg.catholic);
    push_flag("controversial_weapons", esg.controversial_weapons);
    push_flag("small_arms", esg.small_arms);
    push_flag("fur_leather", esg.fur_leather);
    push_flag("gambling", esg.gambling);
    push_flag("gmo", esg.gmo);
    push_flag("military_contract", esg.military_contract);
    push_flag("nuclear", esg.nuclear);
    push_flag("palm_oil", esg.palm_oil);
    push_flag("pesticides", esg.pesticides);
    push_flag("thermal_coal", esg.thermal_coal);
    push_flag("tobacco", esg.tobacco);

    // Return scores together with involvement in a single summary
    Ok(ctx.finish(EsgSummary {
        scores: Some(scores),
        involvement,
    }))
}
