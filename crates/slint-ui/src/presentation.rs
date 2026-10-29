//! Presentation mapping: JSON/runtime values to bounded, localized Slint rows.
//! This module intentionally performs no wallet or network operations.

use crate::format::short_address;
use crate::{i18n, is_slatepack_armor, sanitize};
use chrono::TimeZone;

pub fn contact_rows(value: &serde_json::Value) -> Vec<crate::ContactRow> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let address = item["address"].as_str()?;
                    Some(crate::ContactRow {
                        address: address.into(),
                        short_address: short_address(address).into(),
                        name: sanitize::single_line(item["name"].as_str().unwrap_or("—"), 80)
                            .into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn message_rows(value: &serde_json::Value) -> Vec<crate::MessageRow> {
    value
        .as_array()
        .map(|records| {
            records
                .iter()
                .map(|record| {
                    let incoming = record["direction"].as_str() == Some("incoming");
                    let text = record["kind"]["text"]
                        .as_str()
                        .or_else(|| record["kind"]["kind"].as_str())
                        .map(i18n::tr)
                        .unwrap_or_else(|| i18n::tr("нетекстовое сообщение"));
                    let text = sanitize::message(&text, 8192);
                    let when = record["created_at"]
                        .as_u64()
                        .and_then(|seconds| chrono::Local.timestamp_opt(seconds as i64, 0).single())
                        .map(|at| at.format("%d.%m %H:%M").to_string())
                        .unwrap_or_default();
                    let slatepack = is_slatepack_armor(&text);
                    crate::MessageRow {
                        direction: i18n::tr(if incoming {
                            "входящее"
                        } else {
                            "исходящее"
                        })
                        .into(),
                        text: text.into(),
                        when: when.into(),
                        incoming,
                        slatepack,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn blocked_rows(value: &serde_json::Value) -> Vec<slint::SharedString> {
    value["addresses"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(slint::SharedString::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn blocked_text(value: &serde_json::Value) -> String {
    let addresses = value["addresses"].as_array();
    match addresses {
        Some(items) if !items.is_empty() => i18n::tr(&format!(
            "заблокированы: {}",
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        _ => i18n::tr("заблокированных адресов нет"),
    }
}

pub fn history_text(value: &serde_json::Value) -> String {
    value
        .as_array()
        .map(|records| {
            records
                .iter()
                .map(|record| {
                    let direction = record["direction"].as_str().unwrap_or("?");
                    let kind = record["kind"]["text"]
                        .as_str()
                        .or_else(|| record["kind"].as_str())
                        .map(i18n::tr)
                        .unwrap_or_else(|| i18n::tr("сообщение"));
                    sanitize::log_line(&format!("{direction} · {kind}"))
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

pub fn swap_rows(board: &flint_runtime::SwapBoard) -> Vec<crate::SwapRow> {
    board
        .offers
        .iter()
        .map(|offer| crate::SwapRow {
            id: offer.id.clone().into(),
            pair: format!("{} → {}", offer.give, offer.get).into(),
            rate: format!("{} {} за 1 {}", offer.rate, offer.get, offer.give).into(),
            bounds: format!("{}..{}", offer.min, offer.max).into(),
            stamps: format!("stamps {} · burn {}", offer.stamps, offer.burn).into(),
        })
        .collect()
}
