// SPDX-License-Identifier: MPL-2.0

use crate::model::ProviderCost;

enum SymbolKind {
    Mapped {
        iso: &'static str,
        symbol: &'static str,
        prefix: bool,
    },
    UnknownCodeSuffix(String),
}

#[must_use]
pub fn format_provider_cost(cost: &ProviderCost) -> (String, String) {
    let kind = classify_units(&cost.units);
    let used_s = fmt_amount(&kind, cost.used);
    let line = match cost.limit.filter(|l| *l > f64::EPSILON) {
        Some(lim) => {
            let lim_s = fmt_amount(&kind, lim);
            format!("{used_s} / {lim_s}")
        }
        None => format!("{used_s} spent"),
    };
    (line, iso_tooltip(&kind))
}

fn classify_units(raw: &str) -> SymbolKind {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return map_iso("USD");
    }
    let upper = trimmed.to_ascii_uppercase();
    let key = upper.as_str();
    let iso = match key {
        "$" | "US$" | "USD" => "USD",
        "EURO" | "EUR" | "€" => "EUR",
        "GBP" | "£" => "GBP",
        "JPY" | "¥" => "JPY",
        "RMB" | "CNY" => "CNY",
        "CAD" => "CAD",
        "AUD" => "AUD",
        "NZD" => "NZD",
        "SGD" => "SGD",
        "HKD" => "HKD",
        "MXN" => "MXN",
        "BRL" => "BRL",
        "INR" => "INR",
        "KRW" => "KRW",
        "CHF" => "CHF",
        "SEK" => "SEK",
        "NOK" => "NOK",
        "DKK" => "DKK",
        "PLN" => "PLN",
        "CZK" => "CZK",
        "HUF" => "HUF",
        "ILS" => "ILS",
        "TRY" => "TRY",
        "ZAR" => "ZAR",
        u if u.len() == 3 && u.chars().all(|c| c.is_ascii_alphabetic()) => {
            return SymbolKind::UnknownCodeSuffix(upper.clone());
        }
        _ => {
            return SymbolKind::UnknownCodeSuffix(upper);
        }
    };
    map_iso(iso)
}

const ISO_MAPPINGS: &[(&str, &str, bool)] = &[
    ("USD", "$", true),
    ("EUR", "€", false),
    ("GBP", "£", true),
    ("JPY", "¥", true),
    ("CNY", "¥", true),
    ("CAD", "CA$", true),
    ("AUD", "A$", true),
    ("NZD", "NZ$", true),
    ("SGD", "S$", true),
    ("HKD", "HK$", true),
    ("MXN", "MX$", true),
    ("BRL", "R$", true),
    ("INR", "₹", true),
    ("KRW", "₩", true),
    ("CHF", "CHF", false),
    ("SEK", "kr", false),
    ("NOK", "kr", false),
    ("DKK", "kr", false),
    ("PLN", "zł", false),
    ("CZK", "Kč", false),
    ("HUF", "Ft", false),
    ("ILS", "₪", true),
    ("TRY", "₺", false),
    ("ZAR", "R", true),
];

fn map_iso(canonical: &str) -> SymbolKind {
    if let Some(&(iso, symbol, prefix)) =
        ISO_MAPPINGS.iter().find(|(code, _, _)| *code == canonical)
    {
        return SymbolKind::Mapped {
            iso,
            symbol,
            prefix,
        };
    }
    SymbolKind::UnknownCodeSuffix(canonical.to_string())
}

fn fmt_amount(kind: &SymbolKind, value: f64) -> String {
    let n = format!("{value:.2}");
    match kind {
        SymbolKind::Mapped {
            symbol,
            prefix: true,
            ..
        } => format!("{symbol} {n}"),
        SymbolKind::Mapped {
            symbol,
            prefix: false,
            ..
        } => format!("{n} {symbol}"),
        SymbolKind::UnknownCodeSuffix(code) => format!("{n} {code}"),
    }
}

fn iso_tooltip(kind: &SymbolKind) -> String {
    match kind {
        SymbolKind::Mapped { iso, .. } => (*iso).to_string(),
        SymbolKind::UnknownCodeSuffix(code) => code.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProviderCost;

    #[test]
    fn eur_suffix_and_tooltip() {
        let cost = ProviderCost {
            used: 1.2,
            limit: Some(20.0),
            units: "EUR".into(),
        };
        let (line, iso) = format_provider_cost(&cost);
        assert_eq!(iso, "EUR");
        assert!(!line.starts_with('€'));
        assert!(line.contains("1.20 €"));
        assert!(line.contains("20.00 €"));
    }

    #[test]
    fn usd_prefix_tooltip() {
        let cost = ProviderCost {
            used: 3.5,
            limit: None,
            units: "USD".into(),
        };
        let (line, iso) = format_provider_cost(&cost);
        assert_eq!(iso, "USD");
        assert!(line.starts_with("$ "));
        assert!(line.ends_with(" spent"));
    }
}
