#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteSize(pub u64);

pub fn round_up(value: u64, multiple: u64) -> u64 {
    if multiple == 0 || value == 0 {
        return value;
    }
    value.div_ceil(multiple) * multiple
}

pub fn format_capacity_binary(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.3} {}", round_to_3(value), UNITS[unit])
}

pub fn format_bytes_binary(bytes: u64) -> String {
    format_capacity_binary(bytes)
}

pub fn format_byte_rate_binary(bytes_per_second: f64) -> String {
    if !bytes_per_second.is_finite() || bytes_per_second < 0.0 {
        return "unavailable".to_string();
    }
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes_per_second < 1024.0 {
        return format!("{:.3} B/s", round_to_3(bytes_per_second));
    }
    let mut value = bytes_per_second;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{:.3} {}/s", round_to_3(value), UNITS[unit])
}

pub fn format_percent_3(numerator: u64, denominator: u64) -> String {
    if denominator == 0 {
        return "unavailable".to_string();
    }
    let value = (numerator as f64 * 100.0 / denominator as f64).clamp(0.0, 100.0);
    format!("{:.3}%", round_to_3(value))
}

pub fn format_percent(value: f64, clamp_to_100: bool) -> String {
    if !value.is_finite() {
        return "unavailable".to_string();
    }
    let value = if clamp_to_100 {
        value.clamp(0.0, 100.0)
    } else {
        value
    };
    format!("{:.3}%", round_to_3(value))
}

pub fn format_duration_seconds_3(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unavailable".to_string();
    }
    if seconds < 60.0 {
        return format!("{:.3} s", round_to_3(seconds));
    }
    let minutes = (seconds / 60.0).floor() as u64;
    let remaining = seconds - (minutes * 60) as f64;
    format!("{minutes}m {:.3}s", round_to_3(remaining))
}

pub fn format_duration_seconds(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unavailable".to_string();
    }
    if seconds < 60.0 {
        return format!("{:.3} s", round_to_3(seconds));
    }
    let minutes = (seconds / 60.0).floor() as u64;
    let remaining = seconds - (minutes * 60) as f64;
    format!("{minutes}m {:.3}s", round_to_3(remaining))
}

pub fn format_exact_byte_count(bytes: u64) -> String {
    format_capacity_binary(bytes)
}

pub fn format_bytes_decimal_exact(bytes: u64) -> String {
    format_exact_byte_count(bytes)
}

pub fn format_percent_3_half_away(value: f64, clamp_to_100: bool) -> String {
    format_percent(value, clamp_to_100)
}

pub fn format_binary_capacity_3_half_away(bytes: u64) -> String {
    format_capacity_binary(bytes)
}

pub fn format_binary_rate_3_half_away(bytes_per_second: f64) -> String {
    format_byte_rate_binary(bytes_per_second)
}

pub fn format_load_3_half_away(value: f64) -> String {
    if value.is_finite() {
        format!("{:.3}", round_to_3(value))
    } else {
        "unavailable".to_string()
    }
}

pub fn format_temperature_3_half_away(value: f64) -> String {
    if value.is_finite() {
        format!("{:.3} C", round_to_3(value))
    } else {
        "unavailable".to_string()
    }
}

fn round_to_3(value: f64) -> f64 {
    round_half_away(value * 1000.0) / 1000.0
}

fn round_half_away(value: f64) -> f64 {
    if value.is_sign_negative() {
        (value - 0.5).ceil()
    } else {
        (value + 0.5).floor()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_formatter_matches_dashboard_rules() {
        assert_eq!(format_capacity_binary(0), "0 B");
        assert_eq!(format_capacity_binary(512), "512 B");
        assert_eq!(format_capacity_binary(1024), "1.000 KiB");
        assert_eq!(format_capacity_binary(1536), "1.500 KiB");
        assert_eq!(format_capacity_binary(1024 * 1024), "1.000 MiB");
    }

    #[test]
    fn percent_and_duration_formatters_match_design_precision() {
        assert_eq!(format_percent_3(34_0375, 100_0000), "34.038%");
        assert_eq!(format_percent(34.0375, true), "34.038%");
        assert_eq!(format_percent(100.1, true), "100.000%");
        assert_eq!(format_duration_seconds_3(1.2345), "1.235 s");
        assert_eq!(format_duration_seconds(28.7125), "28.713 s");
        assert_eq!(format_duration_seconds(61.25), "1m 1.250s");
    }

    #[test]
    fn exact_byte_count_uses_design_byte_units() {
        assert_eq!(format_exact_byte_count(123), "123 B");
        assert_eq!(format_exact_byte_count(1024), "1.000 KiB");
    }
}
