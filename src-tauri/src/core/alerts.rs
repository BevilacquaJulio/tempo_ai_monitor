use crate::{
    core::history,
    providers::{self, Observation},
};
use chrono::{DateTime, Datelike, Local, Weekday};
use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub fn notify(
    app: &AppHandle,
    db: &rusqlite::Connection,
    provider: &str,
    display_name: &str,
    observation: &Observation,
    now: i64,
    thresholds: &[u8],
    portuguese: bool,
) {
    if providers::observation_is_stale(observation.at, now) {
        return;
    }
    for window in &observation.quotas {
        if window["status"] != "available" {
            continue;
        }
        let (Some(used), Some(id), Some(reset)) = (
            window["usedPercent"].as_f64(),
            window["id"].as_str(),
            window["resetAt"]
                .as_str()
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                .map(|time| time.timestamp()),
        ) else {
            continue;
        };
        if reset <= now {
            continue;
        }
        let mut should_notify = false;
        for level in thresholds.iter().copied().filter(|threshold| used >= f64::from(*threshold)) {
            should_notify |= history::reserve_alert(db, provider, id, reset, level).unwrap_or(false);
        }
        if should_notify {
            let (title, body) = usage_alert(display_name, window, used, reset, now, portuguese);
            let _ = app.notification().builder().title(title).body(body).show();
        }
    }
}

fn weekday_short(weekday: Weekday, portuguese: bool) -> &'static str {
    match (portuguese, weekday) {
        (true, Weekday::Sun) => "dom",
        (true, Weekday::Mon) => "seg",
        (true, Weekday::Tue) => "ter",
        (true, Weekday::Wed) => "qua",
        (true, Weekday::Thu) => "qui",
        (true, Weekday::Fri) => "sex",
        (true, Weekday::Sat) => "sáb",
        (false, Weekday::Sun) => "Sun",
        (false, Weekday::Mon) => "Mon",
        (false, Weekday::Tue) => "Tue",
        (false, Weekday::Wed) => "Wed",
        (false, Weekday::Thu) => "Thu",
        (false, Weekday::Fri) => "Fri",
        (false, Weekday::Sat) => "Sat",
    }
}

fn renewal_sentence(reset: i64, now: i64, portuguese: bool) -> String {
    let (Some(at), Some(current)) = (
        DateTime::from_timestamp(reset, 0).map(|t| t.with_timezone(&Local)),
        DateTime::from_timestamp(now, 0).map(|t| t.with_timezone(&Local)),
    ) else {
        return String::new();
    };
    let day_diff = (at.date_naive() - current.date_naive()).num_days();
    if portuguese {
        let time = at.format("%H:%M");
        if day_diff <= 0 {
            format!(" Renova hoje às {time}.")
        } else if day_diff <= 6 {
            let day = weekday_short(at.weekday(), true);
            format!(" Renova em {day} às {time}.")
        } else {
            let date = at.format("%d/%m");
            format!(" Renova em {date} às {time}.")
        }
    } else {
        let time = at.format("%I:%M %p");
        if day_diff <= 0 {
            format!(" Renews today at {time}.")
        } else if day_diff <= 6 {
            let day = weekday_short(at.weekday(), false);
            format!(" Renews {day} at {time}.")
        } else {
            let date = at.format("%m/%d");
            format!(" Renews on {date} at {time}.")
        }
    }
}

pub fn usage_alert(name: &str, window: &Value, used: f64, reset: i64, now: i64, portuguese: bool) -> (String, String) {
    let label = match (portuguese, window["type"].as_str()) {
        (true, Some("five_hour")) => "Limite de 5 horas",
        (true, Some("weekly")) => "Limite semanal",
        (true, _) => "Limite adicional",
        (false, Some("five_hour")) => "5-hour limit",
        (false, Some("weekly")) => "Weekly limit",
        (false, _) => "Additional limit",
    };
    let scope = window["scope"].as_str().map(|scope| format!(" ({scope})")).unwrap_or_default();
    let status = if portuguese {
        if used >= 100.0 {
            "Limite atingido"
        } else if used >= 95.0 {
            "Seu uso está quase no fim"
        } else if used >= 90.0 {
            "Resta pouco uso"
        } else {
            "Você está perto do limite"
        }
    } else if used >= 100.0 {
        "Limit reached"
    } else if used >= 95.0 {
        "About to run out"
    } else if used >= 90.0 {
        "Very little left"
    } else {
        "Getting close to the limit"
    };
    let next_step = match (portuguese, used >= 100.0) {
        (true, true) => "Aguarde a renovação ou confira outra ferramenta no TempoAI.",
        (true, false) => "Abra o TempoAI para ver quanto ainda dá para usar.",
        (false, true) => "Wait for it to renew or check another tool in TempoAI.",
        (false, false) => "Open TempoAI to see how much you can still use.",
    };
    let renewal = renewal_sentence(reset, now, portuguese);
    (format!("{name}: {status}"), format!("{label}{scope}.{renewal} {next_step}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_alerts_explain_the_limit_and_use_date_or_weekday_by_horizon() {
        let window = json!({"type":"weekly", "scope":"Claude Opus"});
        let now = 1_800_000_000;
        let in_7_days = now + 7 * 86_400;
        let (title, body) = usage_alert("Claude Code", &window, 100.0, in_7_days, now, true);
        assert_eq!(title, "Claude Code: Limite atingido");
        assert!(body.contains("Renova em ") && body.contains("/"));
        assert!(body.contains("Aguarde a renovação ou confira outra ferramenta"));

        let in_3_days = now + 3 * 86_400;
        let (_, body_week) = usage_alert("Claude Code", &window, 95.0, in_3_days, now, true);
        assert!(body_week.contains("Renova em ") && !body_week.contains("/"));

        let (title_en, body_en) = usage_alert("Claude Code", &window, 95.0, in_7_days, now, false);
        assert_eq!(title_en, "Claude Code: About to run out");
        assert!(body_en.contains("Open TempoAI to see how much you can still use."));
    }
}
