//! Binary byte units shared by native UI logs and the CLI.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit > 0 && (value * 10.0).round() / 10.0 >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn byte_readouts_choose_units_without_rounding_small_tables_to_zero() {
        let cases = [
            (0, "0 B"),
            (7, "7 B"),
            (1023, "1023 B"),
            (1024, "1.0 KiB"),
            (1536, "1.5 KiB"),
            (10_240, "10.0 KiB"),
            (1_048_575, "1.0 MiB"),
            (1_048_576, "1.0 MiB"),
            (853_221_600, "813.7 MiB"),
            (1 << 30, "1.0 GiB"),
        ];
        for (bytes, expected) in cases {
            assert_eq!(super::format_bytes(bytes), expected);
        }
    }
}
