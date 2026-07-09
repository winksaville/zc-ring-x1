//! Number formatting helpers for the band-table renderer.

/// Format an integer with thousands separators, e.g.
/// `12345` → `"12,345"`.
pub fn fmt_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format a float with `decimals` fractional digits and thousands
/// separators on the integer part.
pub fn fmt_commas_f64(n: f64, decimals: usize) -> String {
    let s = format!("{n:.decimals$}");
    let (sign, body) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s.as_str()),
    };
    let (int_part, frac_part) = match body.find('.') {
        Some(i) => (&body[..i], &body[i..]),
        None => (body, ""),
    };
    let int_num: u64 = int_part.parse().unwrap_or(0); // OK: int part of a formatted float; overflow beyond u64 formats as 0
    format!("{sign}{}{frac_part}", fmt_commas(int_num))
}
