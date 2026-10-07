//! Sizes: how a document writes one (`below = "4GiB"`), how the window shows one, and what a model's
//! parameter count says about the bytes it takes.

/// `4GiB`, `15 GB`, `500MB`, `1.5GiB`, `4096`: a number, then a unit or none (bytes). `KB`, `MB`, `GB`, `TB`
/// count in thousands, `KiB`, `MiB`, `GiB`, `TiB` in 1024s, as they say; `K`, `M`, `G`, `T` alone are the
/// powers of 1024, which is what the window has always called a gigabyte. `None` for anything else.
pub fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    if number.is_empty() || number.matches('.').count() > 1 || number.starts_with('.') {
        return None;
    }
    let number: f64 = number.parse().ok()?;
    let factor: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "k" | "kib" => 1024.0,
        "m" | "mib" => 1024.0 * 1024.0,
        "g" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    let bytes = number * factor;
    (bytes.is_finite() && bytes < u64::MAX as f64).then_some(bytes as u64)
}

/// `2.5 GB`: the units in 1024s, one decimal, as the window has always written a size.
pub fn format_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = n as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// How many parameters a size tag of the Ollama library names: `0.6b` is 600 million, `22m` 22 million,
/// `8x7b` (a mixture of eight experts of seven billion) 56 billion, `e2b` the 2 billion its name says.
/// `None` for what is not a count.
pub fn parse_parameters(tag: &str) -> Option<f64> {
    let lower = tag.trim().to_ascii_lowercase();
    // a lone letter before the number (`e2b`, `a3b`) belongs to the name, not to the count
    let mut chars = lower.chars();
    let tag = match (chars.next(), chars.next()) {
        (Some(letter), Some(digit)) if letter.is_ascii_lowercase() && digit.is_ascii_digit() => {
            &lower[1..]
        }
        _ => &lower[..],
    };
    let factor = match tag.chars().last()? {
        'k' => 1e3,
        'm' => 1e6,
        'b' => 1e9,
        't' => 1e12,
        _ => return None,
    };
    let body = &tag[..tag.len() - 1];
    let (count, experts) = match body.split_once('x') {
        Some((experts, each)) => (each, experts.parse::<f64>().ok()?),
        None => (body, 1.0),
    };
    if count.is_empty()
        || !count.chars().all(|c| c.is_ascii_digit() || c == '.')
        || count.matches('.').count() > 1
        || count.starts_with('.')
        || count.ends_with('.')
    {
        return None;
    }
    let parameters = count.parse::<f64>().ok()? * experts * factor;
    (parameters.is_finite() && parameters > 0.0).then_some(parameters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_written_the_way_documents_write_them() {
        for (text, bytes) in [
            ("4GiB", Some(4 * 1024 * 1024 * 1024)),
            ("15 GiB", Some(15 * 1024 * 1024 * 1024)),
            ("1.5GiB", Some(1_610_612_736)),
            ("500MB", Some(500_000_000)),
            ("4GB", Some(4_000_000_000)),
            ("4G", Some(4 * 1024 * 1024 * 1024)),
            ("2KiB", Some(2048)),
            ("4096", Some(4096)),
            ("12 B", Some(12)),
            (" 7mib ", Some(7 * 1024 * 1024)),
            ("", None),
            ("GiB", None),
            ("4 elephants", None),
            ("1.2.3GB", None),
            (".5GB", None),
            ("-4GB", None),
            ("99999999999999999999TiB", None),
        ] {
            assert_eq!(parse_size(text), bytes, "{text:?}");
        }
    }

    #[test]
    fn a_size_is_shown_as_the_window_always_showed_one() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(2_489_757_856), "2.3 GB");
        assert_eq!(format_bytes(851_251_104), "811.8 MB");
        assert_eq!(format_bytes(u64::MAX), "16777216.0 TB");
    }

    #[test]
    fn a_size_tag_is_a_count_of_parameters() {
        for (tag, expected) in [
            ("0.6b", Some(6e8)),
            ("1.7b", Some(1.7e9)),
            ("8b", Some(8e9)),
            ("235b", Some(2.35e11)),
            ("22m", Some(2.2e7)),
            ("135m", Some(1.35e8)),
            ("8x7b", Some(5.6e10)),
            ("e2b", Some(2e9)),
            ("e4b", Some(4e9)),
            ("1t", Some(1e12)),
            ("500k", Some(5e5)),
            ("latest", None),
            ("q4_k_m", None),
            ("b", None),
            ("", None),
            ("1.2.3b", None),
            ("7", None),
            ("0b", None),
        ] {
            assert_eq!(parse_parameters(tag), expected, "{tag:?}");
        }
    }
}
