//! Durations as documents write them: `500ms`, `120s`, `2m`, `1h`.

use std::time::Duration;

pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| format!("`{text}` needs a unit: ms, s, m or h"))?;
    let (digits, unit) = text.split_at(split);
    if digits.is_empty() {
        return Err(format!("`{text}` does not start with a number"));
    }
    let n: u64 = digits
        .parse()
        .map_err(|_| format!("`{digits}` is too large"))?;
    let millis = match unit {
        "ms" => n,
        "s" => n.checked_mul(1000).ok_or("the duration is too long")?,
        "m" => n.checked_mul(60_000).ok_or("the duration is too long")?,
        "h" => n.checked_mul(3_600_000).ok_or("the duration is too long")?,
        other => {
            return Err(format!(
                "`{other}` is not a unit of time; use ms, s, m or h"
            ))
        }
    };
    Ok(Duration::from_millis(millis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_are_read() {
        assert_eq!(parse_duration("500ms"), Ok(Duration::from_millis(500)));
        assert_eq!(parse_duration("120s"), Ok(Duration::from_secs(120)));
        assert_eq!(parse_duration("2m"), Ok(Duration::from_secs(120)));
        assert_eq!(parse_duration("1h"), Ok(Duration::from_secs(3600)));
        assert_eq!(parse_duration(" 15s "), Ok(Duration::from_secs(15)));
        assert_eq!(parse_duration("0s"), Ok(Duration::ZERO));
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(parse_duration("120").unwrap_err().contains("needs a unit"));
        assert!(parse_duration("s")
            .unwrap_err()
            .contains("start with a number"));
        assert!(parse_duration("5 days").unwrap_err().contains("not a unit"));
        assert!(parse_duration("").is_err());
        assert!(parse_duration("99999999999999999999s")
            .unwrap_err()
            .contains("too large"));
        assert!(parse_duration("99999999999999999h")
            .unwrap_err()
            .contains("too long"));
    }
}
