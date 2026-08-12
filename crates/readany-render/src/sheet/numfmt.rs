pub(crate) fn format_number(value: f64, code: &str, date_1904: bool) -> String {
    if !value.is_finite() || code.eq_ignore_ascii_case("general") {
        return general(value);
    }
    let (section, use_absolute) = select_section(value, code);
    let section = strip_modifiers(section);
    if looks_like_date(&section) {
        return format_date(value, &section, date_1904);
    }
    if looks_like_fraction(&section) {
        return format_fraction(value, &section, use_absolute);
    }
    format_numeric(value, &section, use_absolute)
}

pub(crate) fn format_text(value: &str, code: &str) -> String {
    let sections = split_sections(code);
    sections
        .get(3)
        .map(|section| render_literals(section, Some(value)))
        .unwrap_or_else(|| value.to_owned())
}

fn general(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    let plain = format!("{value:.10}");
    plain.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn select_section(value: f64, code: &str) -> (&str, bool) {
    let sections = split_sections(code);
    for section in &sections {
        if let Some(condition) = condition(section) {
            if condition.matches(value) {
                return (section, false);
            }
        }
    }
    if value > 0.0 {
        (sections.first().copied().unwrap_or(code), false)
    } else if value < 0.0 {
        if let Some(section) = sections.get(1) {
            (section, true)
        } else {
            (sections.first().copied().unwrap_or(code), false)
        }
    } else {
        (
            sections
                .get(2)
                .or_else(|| sections.first())
                .copied()
                .unwrap_or(code),
            false,
        )
    }
}

fn split_sections(code: &str) -> Vec<&str> {
    let mut sections = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in code.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        } else if ch == ';' && !quoted {
            sections.push(&code[start..index]);
            start = index + 1;
        }
    }
    sections.push(&code[start..]);
    sections
}

#[derive(Clone, Copy)]
enum Comparison {
    Less,
    LessEqual,
    Equal,
    NotEqual,
    GreaterEqual,
    Greater,
}

struct Condition {
    comparison: Comparison,
    operand: f64,
}

impl Condition {
    fn matches(&self, value: f64) -> bool {
        match self.comparison {
            Comparison::Less => value < self.operand,
            Comparison::LessEqual => value <= self.operand,
            Comparison::Equal => value == self.operand,
            Comparison::NotEqual => value != self.operand,
            Comparison::GreaterEqual => value >= self.operand,
            Comparison::Greater => value > self.operand,
        }
    }
}

fn condition(section: &str) -> Option<Condition> {
    for token in bracket_tokens(section) {
        for (operator, comparison) in [
            ("<=", Comparison::LessEqual),
            (">=", Comparison::GreaterEqual),
            ("<>", Comparison::NotEqual),
            ("<", Comparison::Less),
            ("=", Comparison::Equal),
            (">", Comparison::Greater),
        ] {
            if let Some(value) = token.strip_prefix(operator) {
                return Some(Condition {
                    comparison,
                    operand: value.parse().ok()?,
                });
            }
        }
    }
    None
}

fn bracket_tokens(section: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut offset = 0;
    while let Some(start) = section[offset..].find('[') {
        let start = offset + start;
        let Some(end) = section[start + 1..].find(']') else {
            break;
        };
        let end = start + 1 + end;
        result.push(&section[start + 1..end]);
        offset = end + 1;
    }
    result
}

fn strip_modifiers(section: &str) -> String {
    let mut output = String::new();
    let mut offset = 0;
    while let Some(start) = section[offset..].find('[') {
        let start = offset + start;
        output.push_str(&section[offset..start]);
        let Some(end) = section[start + 1..].find(']') else {
            output.push_str(&section[start..]);
            return output;
        };
        let end = start + 1 + end;
        let token = &section[start + 1..end];
        if matches!(token.to_ascii_lowercase().as_str(), "h" | "m" | "s") {
            output.push_str(&section[start..=end]);
        }
        offset = end + 1;
    }
    output.push_str(&section[offset..]);
    output
}

fn looks_like_fraction(code: &str) -> bool {
    code.contains('/')
        && code.split_once('/').is_some_and(|(left, right)| {
            left.contains(['?', '#', '0']) && right.contains(['?', '#', '0'])
        })
}

fn format_fraction(value: f64, code: &str, use_absolute: bool) -> String {
    let magnitude = if use_absolute { value.abs() } else { value };
    let negative = magnitude < 0.0;
    let magnitude = magnitude.abs();
    let whole = magnitude.floor() as u64;
    let denominator_digits = code
        .split_once('/')
        .map(|(_, right)| {
            right
                .chars()
                .take_while(|character| matches!(character, '?' | '#' | '0'))
                .count()
        })
        .unwrap_or(1)
        .clamp(1, 3);
    let maximum = 10_u64.pow(denominator_digits as u32) - 1;
    let (mut numerator, mut denominator) = best_fraction(magnitude.fract(), maximum);
    let mut whole = whole;
    if numerator == denominator {
        whole += 1;
        numerator = 0;
        denominator = 1;
    }
    let mixed = code
        .split_once('/')
        .map(|(left, _)| left.contains(' '))
        .unwrap_or(false);
    let mut output = if numerator == 0 {
        whole.to_string()
    } else if mixed && whole > 0 {
        format!("{whole} {numerator}/{denominator}")
    } else if mixed {
        format!("{numerator}/{denominator}")
    } else {
        format!("{}/{denominator}", whole * denominator + numerator)
    };
    if negative && !use_absolute {
        output.insert(0, '-');
    }
    output
}

fn best_fraction(value: f64, maximum: u64) -> (u64, u64) {
    let mut best = (0, 1);
    let mut error = value;
    for denominator in 1..=maximum {
        let numerator = (value * denominator as f64).round() as u64;
        let candidate = (value - numerator as f64 / denominator as f64).abs();
        if candidate < error {
            best = (numerator, denominator);
            error = candidate;
        }
    }
    let divisor = gcd(best.0, best.1);
    (best.0 / divisor, best.1 / divisor)
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left.max(1)
}

fn format_numeric(value: f64, code: &str, use_absolute: bool) -> String {
    let percent_count = unquoted_count(code, '%');
    let scale_commas = trailing_scale_commas(code);
    let mut magnitude = if use_absolute { value.abs() } else { value };
    magnitude *= 100_f64.powi(percent_count as i32);
    magnitude /= 1000_f64.powi(scale_commas as i32);
    let pattern_start = code.find(['0', '#', '?']);
    let pattern_end = code.rfind(['0', '#', '?']).map(|index| index + 1);
    let Some((start, end)) = pattern_start.zip(pattern_end) else {
        return render_literals(code, None);
    };
    let pattern = &code[start..end];
    let decimals = pattern
        .split_once('.')
        .map(|(_, fractional)| {
            fractional
                .chars()
                .filter(|character| matches!(character, '0' | '#' | '?'))
                .count()
        })
        .unwrap_or(0);
    let scientific = code.contains("E+") || code.contains("E-");
    let rounded = excel_round(magnitude.abs(), decimals);
    let mut number = if scientific {
        format!("{:.*E}", decimals, rounded)
    } else {
        format!("{:.*}", decimals, rounded)
    };
    if pattern
        .split('.')
        .next()
        .is_some_and(|integer| integer.contains(','))
        && !scientific
    {
        number = thousands(&number);
    }
    if magnitude < 0.0 {
        number.insert(0, '-');
    }
    let prefix = render_literals(&code[..start], None);
    let suffix = render_literals(&code[end..], None);
    format!("{prefix}{number}{suffix}")
}

fn excel_round(value: f64, decimals: usize) -> f64 {
    let factor = 10_f64.powi(decimals.min(15) as i32);
    (value * factor).round() / factor
}

fn trailing_scale_commas(code: &str) -> usize {
    let end = code
        .rfind(['0', '#', '?'])
        .map(|index| index + 1)
        .unwrap_or(0);
    code[end..]
        .chars()
        .take_while(|character| *character == ',')
        .count()
}

fn unquoted_count(code: &str, needle: char) -> usize {
    let mut quoted = false;
    let mut escaped = false;
    code.chars()
        .filter(|character| {
            if escaped {
                escaped = false;
                false
            } else if *character == '\\' {
                escaped = true;
                false
            } else if *character == '"' {
                quoted = !quoted;
                false
            } else {
                !quoted && *character == needle
            }
        })
        .count()
}

fn thousands(value: &str) -> String {
    let (integer, decimal) = value.split_once('.').unwrap_or((value, ""));
    let mut reversed = String::new();
    for (index, character) in integer.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            reversed.push(',');
        }
        reversed.push(character);
    }
    let mut output: String = reversed.chars().rev().collect();
    if !decimal.is_empty() {
        output.push('.');
        output.push_str(decimal);
    }
    output
}

fn looks_like_date(code: &str) -> bool {
    let lower = code.to_ascii_lowercase();
    ["yy", "dd", "hh", "ss", "am/pm", "[h]", "[m]", "[s]"]
        .iter()
        .any(|token| lower.contains(token))
}

fn format_date(value: f64, code: &str, date_1904: bool) -> String {
    let serial_days = value.floor() as i64;
    let (year, month, day) = if !date_1904 && serial_days == 60 {
        (1900, 2, 29)
    } else {
        let adjusted = if date_1904 {
            serial_days
        } else if serial_days > 60 {
            serial_days - 1
        } else {
            serial_days
        };
        civil_from_days(adjusted + if date_1904 { -24_107 } else { -25_568 })
    };
    let total_seconds = (value.fract().abs() * 86_400.0).round() as u64;
    let hour = (total_seconds / 3_600 % 24) as u32;
    let minute = (total_seconds / 60 % 60) as u32;
    let second = (total_seconds % 60) as u32;
    let weekday = ((serial_days + if date_1904 { 5 } else { 6 }).rem_euclid(7)) as usize;
    render_date_code(
        code,
        DateParts {
            year,
            month,
            day,
            hour,
            minute,
            second,
            weekday,
            elapsed_seconds: (value.abs() * 86_400.0).round() as u64,
        },
    )
}

struct DateParts {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    weekday: usize,
    elapsed_seconds: u64,
}

fn render_date_code(code: &str, parts: DateParts) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const WEEKDAYS: [&str; 7] = [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ];
    let lower = code.to_ascii_lowercase();
    let twelve_hour = lower.contains("am/pm");
    let mut output = String::new();
    let mut offset = 0;
    let mut previous_hour = false;
    while offset < code.len() {
        let rest = &code[offset..];
        if let Some(tail) = rest.strip_prefix('"') {
            if let Some(end) = tail.find('"') {
                output.push_str(&tail[..end]);
                offset += end + 2;
                continue;
            }
        }
        if let Some(tail) = rest.strip_prefix('\\') {
            if let Some(character) = tail.chars().next() {
                output.push(character);
                offset += 1 + character.len_utf8();
                continue;
            }
        }
        if rest.starts_with('_') || rest.starts_with('*') {
            if let Some(character) = rest[1..].chars().next() {
                if rest.starts_with('_') {
                    output.push(' ');
                }
                offset += 1 + character.len_utf8();
                continue;
            }
        }
        let lower_rest = &lower[offset..];
        let token = [
            "am/pm", "dddd", "mmmm", "ddd", "mmm", "yyyy", "[h]", "[m]", "[s]", "yy", "dd", "hh",
            "mm", "ss", "d", "h", "m", "s",
        ]
        .into_iter()
        .find(|token| lower_rest.starts_with(token));
        if let Some(token) = token {
            let minute_token = matches!(token, "m" | "mm")
                && (previous_hour
                    || lower_rest[token.len()..]
                        .trim_start_matches(|character: char| !character.is_ascii_alphabetic())
                        .starts_with('s'));
            let text = match token {
                "am/pm" => if parts.hour < 12 { "AM" } else { "PM" }.into(),
                "yyyy" => format!("{:04}", parts.year),
                "yy" => format!("{:02}", parts.year.rem_euclid(100)),
                "mmmm" => MONTHS[(parts.month - 1) as usize].into(),
                "mmm" => MONTHS[(parts.month - 1) as usize][..3].into(),
                "dddd" => WEEKDAYS[parts.weekday].into(),
                "ddd" => WEEKDAYS[parts.weekday][..3].into(),
                "dd" => format!("{:02}", parts.day),
                "d" => parts.day.to_string(),
                "hh" => format!(
                    "{:02}",
                    if twelve_hour {
                        twelve(parts.hour)
                    } else {
                        parts.hour
                    }
                ),
                "h" => if twelve_hour {
                    twelve(parts.hour)
                } else {
                    parts.hour
                }
                .to_string(),
                "mm" if minute_token => format!("{:02}", parts.minute),
                "m" if minute_token => parts.minute.to_string(),
                "mm" => format!("{:02}", parts.month),
                "m" => parts.month.to_string(),
                "ss" => format!("{:02}", parts.second),
                "s" => parts.second.to_string(),
                "[h]" => (parts.elapsed_seconds / 3_600).to_string(),
                "[m]" => (parts.elapsed_seconds / 60).to_string(),
                "[s]" => parts.elapsed_seconds.to_string(),
                _ => String::new(),
            };
            output.push_str(&text);
            previous_hour = matches!(token, "h" | "hh");
            offset += token.len();
            continue;
        }
        let character = rest.chars().next().unwrap_or_default();
        output.push(character);
        if character.is_ascii_alphabetic() {
            previous_hour = false;
        }
        offset += character.len_utf8();
    }
    output
}

fn twelve(hour: u32) -> u32 {
    if hour % 12 == 0 { 12 } else { hour % 12 }
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    ((y + i64::from(month <= 2)) as i32, month as u32, day as u32)
}

fn render_literals(code: &str, text: Option<&str>) -> String {
    let mut output = String::new();
    let mut characters = code.chars().peekable();
    let mut quoted = false;
    while let Some(character) = characters.next() {
        match character {
            '"' => quoted = !quoted,
            '\\' => {
                if let Some(next) = characters.next() {
                    output.push(next);
                }
            }
            '_' => {
                characters.next();
                output.push(' ');
            }
            '*' => {
                characters.next();
            }
            '@' if !quoted => output.push_str(text.unwrap_or("")),
            '%' if !quoted => output.push('%'),
            '[' if !quoted => {
                for next in characters.by_ref() {
                    if next == ']' {
                        break;
                    }
                }
            }
            value if quoted || !matches!(value, '0' | '#' | '?' | ',' | '.') => output.push(value),
            _ => {}
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn committed_reference_cases_match_excel_and_libreoffice() {
        for (value, code, expected) in [
            (12345.678, "#,##0.00", "12,345.68"),
            (-1234.0, "#,##0;[Red](#,##0)", "(1,234)"),
            (0.256, "0.0%", "25.6%"),
            (1_250_000.0, "0.0,,\"M\"", "1.3M"),
            (1.25, "# ?/?", "1 1/4"),
            (0.333, "?/?", "1/3"),
            (60.0, "yyyy-mm-dd", "1900-02-29"),
            (
                61.5,
                "ddd, mmm d, yyyy h:mm AM/PM",
                "Thu, Mar 1, 1900 12:00 PM",
            ),
            (1.5, "[h]:mm:ss", "36:00:00"),
            (150.0, "[>100]\"high\";[Red]\"low\"", "high"),
        ] {
            assert_eq!(format_number(value, code, false), expected, "{code}");
        }
        assert_eq!(format_text("claim", "0;0;0;\"text: \"@"), "text: claim");
    }

    proptest! {
        #[test]
        fn every_finite_value_formats_without_panicking(value in -1e12_f64..1e12_f64) {
            for code in ["General", "0", "0.00", "#,##0", "0%", "# ?/?", "yyyy-mm-dd", "[h]:mm:ss"] {
                let formatted = format_number(value, code, false);
                prop_assert!(!formatted.is_empty());
            }
        }
    }
}
