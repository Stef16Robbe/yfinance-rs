//! Helpers for inferring currencies from country information.

use std::{collections::HashMap, sync::LazyLock};

use paft::money::Currency;

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

/// Precomputed exact lookup table using `COUNTRY_CURRENCY_RULES`.
static COUNTRY_TO_CURRENCY: LazyLock<HashMap<&'static str, Currency>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for (country, code) in COUNTRY_CURRENCY_RULES {
        map.insert(*country, parse_currency_code(code));
    }
    map
});

/// Precomputed fuzzy lookup table using `COUNTRY_CURRENCY_RULES`.
static FUZZY_COUNTRY_TO_CURRENCY: LazyLock<Vec<(&'static str, Currency)>> = LazyLock::new(|| {
    let mut rules = COUNTRY_CURRENCY_RULES
        .iter()
        .map(|(country, code)| (*country, parse_currency_code(code)))
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

/// Normalize a country string to an uppercase ASCII key.
fn normalize_country(country: &str) -> Option<String> {
    let trimmed = country.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut buf = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        match ch {
            'A'..='Z' | '0'..='9' => buf.push(ch),
            'a'..='z' => buf.push(ch.to_ascii_uppercase()),
            ' ' | '\t' | '\n' | '\r' | '\'' | '`' | '"' => buf.push(' '),
            '-' | '_' | '/' | ',' | '.' | ';' | ':' | '&' | '(' | ')' | '[' | ']' | '{' | '}' => {
                buf.push(' ');
            }
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' => {
                buf.push('A');
            }
            'ç' | 'Ç' => buf.push('C'),
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => buf.push('E'),
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => buf.push('I'),
            'ñ' | 'Ñ' => buf.push('N'),
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' => buf.push('O'),
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => buf.push('U'),
            'ý' | 'ÿ' | 'Ý' => buf.push('Y'),
            _ => {
                // Ignore other symbols to keep normalization simple.
            }
        }
    }

    let normalized = buf
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
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
    use super::currency_for_country;

    fn currency_code(country: &str) -> Option<String> {
        currency_for_country(country).map(|currency| currency.to_string())
    }

    #[test]
    fn exact_country_lookup_uses_country_currency_table() {
        assert_eq!(currency_code("Italy").as_deref(), Some("EUR"));
        assert_eq!(currency_code("United States").as_deref(), Some("USD"));
        assert_eq!(currency_code("Cote d'Ivoire").as_deref(), Some("XOF"));
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
