use chrono::{Local, LocalResult, TimeZone};

pub fn get_local_timestring(time: i64) -> String {
    let datetime = match Local.timestamp_millis_opt(time) {
        LocalResult::Single(dt) => dt,
        _ => Local::now(),
    };
    datetime.format("%H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_timestamp_returns_formatted_time() {
        let result = get_local_timestring(0);
        assert_eq!(result.len(), 8, "should produce HH:MM:SS format");
        assert!(result.contains(':'), "should contain colon separators");
    }

    #[test]
    fn invalid_timestamp_does_not_panic() {
        let result = get_local_timestring(i64::MAX);
        assert_eq!(result.len(), 8, "should fall back to current time");
    }

    #[test]
    fn extreme_negative_timestamp_does_not_panic() {
        let result = get_local_timestring(i64::MIN);
        assert_eq!(result.len(), 8, "should fall back to current time");
    }
}
