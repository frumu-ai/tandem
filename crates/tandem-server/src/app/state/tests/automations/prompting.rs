use super::*;

fn runtime_values(date: &str, time: &str, timestamp: &str) -> AutomationPromptRuntimeValues {
    let time_hms = if time.len() == 4 {
        format!("{time}00")
    } else {
        time.to_string()
    };
    let timestamp_filename = if time.len() == 4 {
        format!("{}_{}-{}-00", date, &time[..2], &time[2..])
    } else {
        format!("{}_{}", date, time)
    };
    AutomationPromptRuntimeValues {
        current_date: date.to_string(),
        current_time: time.to_string(),
        current_timestamp: timestamp.to_string(),
        current_date_compact: date.replace('-', ""),
        current_time_hms: time_hms,
        current_timestamp_filename: timestamp_filename,
    }
}


include!("prompting_parts/part01.rs");
include!("prompting_parts/part02.rs");
