use chrono::{Local, LocalResult, TimeZone};

pub fn get_local_timestring(time: i64) -> String {
    let datetime = match Local.timestamp_millis_opt(time) {
        LocalResult::Single(dt) => dt,
        _ => Local::now(),
    };
    datetime.format("%H:%M:%S").to_string()
}
