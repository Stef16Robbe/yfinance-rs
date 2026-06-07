//! Helpers for inferring currencies from country information.

use std::{
    collections::{HashMap, hash_map::Entry},
    sync::LazyLock,
};

use paft::money::Currency;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Normalized country/country-alias → currency code pairs.
///
/// Keys must be uppercase and ASCII; values are ISO 4217 currency codes.
const COUNTRY_CURRENCY_RULES: &[(&str, &str)] = &[
    ("UNITED STATES", "USD"),
    ("UNITED STATES OF AMERICA", "USD"),
    ("U S", "USD"),
    ("U S A", "USD"),
    ("US", "USD"),
    ("USA", "USD"),
    ("CANADA", "CAD"),
    ("MEXICO", "MXN"),
    ("BRAZIL", "BRL"),
    ("ARGENTINA", "ARS"),
    ("CHILE", "CLP"),
    ("COLOMBIA", "COP"),
    ("PERU", "PEN"),
    ("URUGUAY", "UYU"),
    ("PARAGUAY", "PYG"),
    ("BOLIVIA", "BOB"),
    ("ECUADOR", "USD"),
    ("VENEZUELA", "VES"),
    ("COSTA RICA", "CRC"),
    ("GUATEMALA", "GTQ"),
    ("HONDURAS", "HNL"),
    ("NICARAGUA", "NIO"),
    ("PANAMA", "USD"),
    ("EL SALVADOR", "USD"),
    ("BELIZE", "BZD"),
    ("DOMINICAN REPUBLIC", "DOP"),
    ("JAMAICA", "JMD"),
    ("TRINIDAD AND TOBAGO", "TTD"),
    ("TRINIDAD", "TTD"),
    ("BARBADOS", "BBD"),
    ("BAHAMAS", "BSD"),
    ("BERMUDA", "BMD"),
    ("CAYMAN ISLANDS", "KYD"),
    ("CAYMAN", "KYD"),
    ("ARUBA", "AWG"),
    ("CURACAO", "ANG"),
    ("BRITISH VIRGIN ISLANDS", "USD"),
    ("PUERTO RICO", "USD"),
    ("DOMINICAN", "DOP"),
    ("UNITED KINGDOM", "GBP"),
    ("ENGLAND", "GBP"),
    ("SCOTLAND", "GBP"),
    ("WALES", "GBP"),
    ("NORTHERN IRELAND", "GBP"),
    ("EUROPEAN UNION", "EUR"),
    ("EURO AREA", "EUR"),
    ("IRELAND", "EUR"),
    ("FRANCE", "EUR"),
    ("GERMANY", "EUR"),
    ("ITALY", "EUR"),
    ("SPAIN", "EUR"),
    ("PORTUGAL", "EUR"),
    ("NETHERLANDS", "EUR"),
    ("BELGIUM", "EUR"),
    ("LUXEMBOURG", "EUR"),
    ("AUSTRIA", "EUR"),
    ("SWITZERLAND", "CHF"),
    ("SWEDEN", "SEK"),
    ("NORWAY", "NOK"),
    ("DENMARK", "DKK"),
    ("FINLAND", "EUR"),
    ("ICELAND", "ISK"),
    ("POLAND", "PLN"),
    ("CZECH REPUBLIC", "CZK"),
    ("CZECHIA", "CZK"),
    ("CZECH", "CZK"),
    ("HUNGARY", "HUF"),
    ("SLOVAKIA", "EUR"),
    ("SLOVENIA", "EUR"),
    ("CROATIA", "EUR"),
    ("ROMANIA", "RON"),
    ("BULGARIA", "BGN"),
    ("GREECE", "EUR"),
    ("CYPRUS", "EUR"),
    ("MALTA", "EUR"),
    ("ESTONIA", "EUR"),
    ("LATVIA", "EUR"),
    ("LITHUANIA", "EUR"),
    ("UKRAINE", "UAH"),
    ("BELARUS", "BYN"),
    ("RUSSIA", "RUB"),
    ("TURKEY", "TRY"),
    ("SERBIA", "RSD"),
    ("BOSNIA AND HERZEGOVINA", "BAM"),
    ("NORTH MACEDONIA", "MKD"),
    ("ALBANIA", "ALL"),
    ("MONTENEGRO", "EUR"),
    ("KOSOVO", "EUR"),
    ("ARMENIA", "AMD"),
    ("GEORGIA", "GEL"),
    ("AZERBAIJAN", "AZN"),
    ("KAZAKHSTAN", "KZT"),
    ("UZBEKISTAN", "UZS"),
    ("TURKMENISTAN", "TMT"),
    ("KYRGYZSTAN", "KGS"),
    ("TAJIKISTAN", "TJS"),
    ("CHINA", "CNY"),
    ("PEOPLES REPUBLIC OF CHINA", "CNY"),
    ("HONG KONG", "HKD"),
    ("MACAU", "MOP"),
    ("TAIWAN", "TWD"),
    ("KOREA", "KRW"),
    ("JAPAN", "JPY"),
    ("SOUTH KOREA", "KRW"),
    ("REPUBLIC OF KOREA", "KRW"),
    ("NORTH KOREA", "KPW"),
    ("INDIA", "INR"),
    ("PAKISTAN", "PKR"),
    ("BANGLADESH", "BDT"),
    ("SRI LANKA", "LKR"),
    ("NEPAL", "NPR"),
    ("BHUTAN", "BTN"),
    ("MALDIVES", "MVR"),
    ("MYANMAR", "MMK"),
    ("THAILAND", "THB"),
    ("VIETNAM", "VND"),
    ("LAOS", "LAK"),
    ("CAMBODIA", "KHR"),
    ("MALAYSIA", "MYR"),
    ("SINGAPORE", "SGD"),
    ("INDONESIA", "IDR"),
    ("PHILIPPINES", "PHP"),
    ("BRUNEI", "BND"),
    ("MONGOLIA", "MNT"),
    ("AUSTRALIA", "AUD"),
    ("NEW ZEALAND", "NZD"),
    ("FIJI", "FJD"),
    ("PAPUA NEW GUINEA", "PGK"),
    ("PAPUA", "PGK"),
    ("NEW CALEDONIA", "XPF"),
    ("FRENCH POLYNESIA", "XPF"),
    ("SAMOA", "WST"),
    ("TONGA", "TOP"),
    ("VANUATU", "VUV"),
    ("SOLOMON ISLANDS", "SBD"),
    ("SOLOMON", "SBD"),
    ("EAST TIMOR", "USD"),
    ("TIMOR-LESTE", "USD"),
    ("UNITED ARAB EMIRATES", "AED"),
    ("SAUDI ARABIA", "SAR"),
    ("QATAR", "QAR"),
    ("KUWAIT", "KWD"),
    ("BAHRAIN", "BHD"),
    ("OMAN", "OMR"),
    ("JORDAN", "JOD"),
    ("LEBANON", "LBP"),
    ("ISRAEL", "ILS"),
    ("PALESTINE", "ILS"),
    ("IRAQ", "IQD"),
    ("IRAN", "IRR"),
    ("AFGHANISTAN", "AFN"),
    ("SYRIA", "SYP"),
    ("YEMEN", "YER"),
    ("EGYPT", "EGP"),
    ("MOROCCO", "MAD"),
    ("ALGERIA", "DZD"),
    ("TUNISIA", "TND"),
    ("LIBYA", "LYD"),
    ("SUDAN", "SDG"),
    ("SOUTH SUDAN", "SSP"),
    ("NIGERIA", "NGN"),
    ("GHANA", "GHS"),
    ("COTE DIVOIRE", "XOF"),
    ("COTE D IVOIRE", "XOF"),
    ("COTE D'IVOIRE", "XOF"),
    ("IVORY COAST", "XOF"),
    ("SENEGAL", "XOF"),
    ("MALI", "XOF"),
    ("BENIN", "XOF"),
    ("BURKINA FASO", "XOF"),
    ("NIGER", "XOF"),
    ("TOGO", "XOF"),
    ("GUINEA-BISSAU", "XOF"),
    ("GUINEA BISSAU", "XOF"),
    ("CAMEROON", "XAF"),
    ("CHAD", "XAF"),
    ("CENTRAL AFRICAN REPUBLIC", "XAF"),
    ("DEMOCRATIC REPUBLIC OF THE CONGO", "CDF"),
    ("DEMOCRATIC REPUBLIC OF CONGO", "CDF"),
    ("CONGO KINSHASA", "CDF"),
    ("DR CONGO", "CDF"),
    ("REPUBLIC OF THE CONGO", "XAF"),
    ("CONGO BRAZZAVILLE", "XAF"),
    ("CONGO", "XAF"),
    ("GABON", "XAF"),
    ("EQUATORIAL GUINEA", "XAF"),
    ("GAMBIA", "GMD"),
    ("GUINEA", "GNF"),
    ("SIERRA LEONE", "SLE"),
    ("LIBERIA", "LRD"),
    ("ETHIOPIA", "ETB"),
    ("ERITREA", "ERN"),
    ("DJIBOUTI", "DJF"),
    ("KENYA", "KES"),
    ("UGANDA", "UGX"),
    ("TANZANIA", "TZS"),
    ("RWANDA", "RWF"),
    ("BURUNDI", "BIF"),
    ("SOMALIA", "SOS"),
    ("SEYCHELLES", "SCR"),
    ("MADAGASCAR", "MGA"),
    ("MAURITIUS", "MUR"),
    ("MOZAMBIQUE", "MZN"),
    ("ZIMBABWE", "ZWL"),
    ("ZAMBIA", "ZMW"),
    ("MALAWI", "MWK"),
    ("ANGOLA", "AOA"),
    ("NAMIBIA", "NAD"),
    ("BOTSWANA", "BWP"),
    ("SOUTH AFRICA", "ZAR"),
    ("LESOTHO", "LSL"),
    ("ESWATINI", "SZL"),
    ("SWAZILAND", "SZL"),
    ("COMOROS", "KMF"),
    ("MAURITANIA", "MRU"),
    ("SAO TOME AND PRINCIPE", "STN"),
    ("GRENADA", "XCD"),
    ("SAINT LUCIA", "XCD"),
    ("SAINT VINCENT AND THE GRENADINES", "XCD"),
    ("ANTIGUA AND BARBUDA", "XCD"),
    ("DOMINICA", "XCD"),
    ("SAINT KITTS AND NEVIS", "XCD"),
];

/// Precomputed exact lookup table using normalized `COUNTRY_CURRENCY_RULES`.
static COUNTRY_TO_CURRENCY: LazyLock<HashMap<String, Currency>> = LazyLock::new(|| {
    let mut map = HashMap::with_capacity(COUNTRY_CURRENCY_RULES.len());
    for (country, code) in COUNTRY_CURRENCY_RULES {
        let country = normalize_country_rule_key(country);
        let currency = parse_currency_code(code);

        match map.entry(country) {
            Entry::Vacant(entry) => {
                entry.insert(currency);
            }
            Entry::Occupied(entry) => {
                assert!(
                    entry.get() == &currency,
                    "conflicting currency rules normalize to the same country key: {:?}",
                    entry.key()
                );
            }
        }
    }
    map
});

/// Precomputed fuzzy lookup table using normalized `COUNTRY_CURRENCY_RULES`.
static FUZZY_COUNTRY_TO_CURRENCY: LazyLock<Vec<(String, Currency)>> = LazyLock::new(|| {
    let mut rules = COUNTRY_CURRENCY_RULES
        .iter()
        .map(|(country, code)| {
            (
                normalize_country_rule_key(country),
                parse_currency_code(code),
            )
        })
        .collect::<Vec<_>>();

    rules.sort_unstable_by(|(left, _), (right, _)| {
        right.len().cmp(&left.len()).then_with(|| left.cmp(right))
    });

    rules
});

fn parse_currency_code(code: &str) -> Currency {
    code.parse()
        .unwrap_or_else(|_| panic!("invalid currency code {code} in country currency table"))
}

fn normalize_country_rule_key(country: &str) -> String {
    normalize_country(country)
        .unwrap_or_else(|| panic!("country currency rule key normalizes to empty: {country:?}"))
}

/// Normalize a country string to an uppercase ASCII key.
fn normalize_country(country: &str) -> Option<String> {
    let trimmed = country.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut buf = String::with_capacity(trimmed.len());
    for ch in trimmed.nfkd().filter(|ch| !is_combining_mark(*ch)) {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch);
        } else if is_country_word_separator(ch) || ch.is_alphanumeric() {
            buf.push(' ');
        }
    }

    let normalized = buf
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

const fn is_country_word_separator(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_ascii_punctuation()
        || matches!(
            ch,
            '\u{00A0}'
                | '\u{2010}'
                | '\u{2011}'
                | '\u{2012}'
                | '\u{2013}'
                | '\u{2014}'
                | '\u{2015}'
                | '\u{2018}'
                | '\u{2019}'
                | '\u{201C}'
                | '\u{201D}'
                | '\u{2212}'
        )
}

/// Attempt to infer a currency from a country string.
///
/// Returns `None` if the country string is empty or cannot be matched.
pub fn currency_for_country(country: &str) -> Option<Currency> {
    let normalized = normalize_country(country)?;

    if let Some(currency) = COUNTRY_TO_CURRENCY.get(normalized.as_str()) {
        return Some(currency.clone());
    }

    heuristic_currency_match(&normalized)
}

fn heuristic_currency_match(normalized: &str) -> Option<Currency> {
    FUZZY_COUNTRY_TO_CURRENCY
        .iter()
        .find_map(|(country, currency)| {
            contains_country_key(normalized, country).then(|| currency.clone())
        })
}

fn contains_country_key(normalized: &str, key: &str) -> bool {
    normalized.match_indices(key).any(|(start, _)| {
        let end = start + key.len();
        let bytes = normalized.as_bytes();
        is_word_boundary(bytes, start) && is_word_boundary(bytes, end)
    })
}

fn is_word_boundary(bytes: &[u8], index: usize) -> bool {
    index == 0 || index == bytes.len() || bytes[index - 1] == b' ' || bytes[index] == b' '
}

#[cfg(test)]
mod tests {
    use super::{
        COUNTRY_CURRENCY_RULES, COUNTRY_TO_CURRENCY, FUZZY_COUNTRY_TO_CURRENCY,
        currency_for_country, normalize_country,
    };
    use paft::money::Currency;
    use std::collections::HashMap;

    fn currency_code(country: &str) -> Option<String> {
        currency_for_country(country).map(|currency| currency.to_string())
    }

    #[test]
    fn country_currency_rules_are_well_formed() {
        for &(country, code) in COUNTRY_CURRENCY_RULES {
            assert!(!country.is_empty(), "empty country currency rule key");
            assert_eq!(
                country.trim(),
                country,
                "country currency rule key has surrounding whitespace: {country:?}"
            );
            assert!(
                country.is_ascii(),
                "country currency rule key must be ASCII: {country:?}"
            );
            assert!(
                !country.bytes().any(|byte| byte.is_ascii_lowercase()),
                "country currency rule key must be uppercase: {country:?}"
            );
            assert!(
                code.parse::<Currency>().is_ok(),
                "invalid currency code {code} for country currency rule {country}"
            );
        }

        let mut exact_keys = HashMap::new();

        for &(country, code) in COUNTRY_CURRENCY_RULES {
            let normalized = normalize_country(country)
                .unwrap_or_else(|| panic!("country currency rule key normalizes empty: {country}"));
            let currency = code
                .parse::<Currency>()
                .expect("country currency rule code was already validated");

            if let Some(previous) = exact_keys.insert(normalized.clone(), currency.clone()) {
                assert!(
                    previous == currency,
                    "country currency rule {country:?} normalizes to conflicting key {normalized:?}"
                );
            }
        }

        assert_eq!(COUNTRY_TO_CURRENCY.len(), exact_keys.len());
        assert_eq!(
            FUZZY_COUNTRY_TO_CURRENCY.len(),
            COUNTRY_CURRENCY_RULES.len()
        );

        for &(country, code) in COUNTRY_CURRENCY_RULES {
            let normalized = normalize_country(country)
                .unwrap_or_else(|| panic!("country currency rule key normalizes empty: {country}"));
            let currency = code
                .parse::<Currency>()
                .expect("country currency rule code was already validated");

            assert_eq!(
                COUNTRY_TO_CURRENCY.get(normalized.as_str()),
                Some(&currency)
            );
            assert!(
                FUZZY_COUNTRY_TO_CURRENCY
                    .iter()
                    .any(|(rule_country, rule_currency)| {
                        rule_country == &normalized && rule_currency == &currency
                    }),
                "fuzzy country currency table is missing rule {country:?} -> {code}"
            );
        }
    }

    #[test]
    fn exact_country_lookup_uses_country_currency_table() {
        assert_eq!(currency_code("Italy").as_deref(), Some("EUR"));
        assert_eq!(currency_code("United States").as_deref(), Some("USD"));
        assert_eq!(currency_code("Cote d'Ivoire").as_deref(), Some("XOF"));
        assert_eq!(currency_code("Timor-Leste").as_deref(), Some("USD"));
    }

    #[test]
    fn country_normalization_strips_unicode_diacritics() {
        assert_eq!(
            normalize_country("Côte d’Ivoire").as_deref(),
            Some("COTE D IVOIRE")
        );
        assert_eq!(currency_code("Curaçao").as_deref(), Some("ANG"));
        assert_eq!(
            currency_code("São Tomé and Príncipe").as_deref(),
            Some("STN")
        );
        assert_eq!(
            normalize_country("Łódź, Poland").as_deref(),
            Some("ODZ POLAND")
        );
        assert_eq!(currency_code("Łódź, Poland").as_deref(), Some("PLN"));
    }

    #[test]
    fn fuzzy_country_lookup_uses_country_currency_table() {
        assert_eq!(
            currency_code("Issuer incorporated in the United Kingdom").as_deref(),
            Some("GBP")
        );
        assert_eq!(currency_code("Euro Area").as_deref(), Some("EUR"));
        assert_eq!(currency_code("Dominican").as_deref(), Some("DOP"));
        assert_eq!(
            currency_code("Republic of South Korea").as_deref(),
            Some("KRW")
        );
        assert_eq!(
            currency_code("Issuer incorporated in Timor-Leste").as_deref(),
            Some("USD")
        );
    }

    #[test]
    fn fuzzy_country_lookup_prefers_specific_word_bounded_matches() {
        assert_eq!(
            currency_code("North Korea exchange").as_deref(),
            Some("KPW")
        );
        assert_eq!(
            currency_code("Republic of South Sudan").as_deref(),
            Some("SSP")
        );
        assert_eq!(currency_code("Nigeria").as_deref(), Some("NGN"));
        assert_eq!(currency_code("Somalia").as_deref(), Some("SOS"));
        assert_eq!(
            currency_code("Democratic Republic of the Congo").as_deref(),
            Some("CDF")
        );
    }
}
