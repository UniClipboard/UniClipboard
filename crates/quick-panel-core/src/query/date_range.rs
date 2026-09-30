//! Recognises a date range written in the search box, such as `9.1-9.15`, `上周` or `3d`.
//!
//! The parser is a pure function of the text and of "today", so it is tested with a fixed date.
//! Ranges are half-open (`start` inclusive, `end` exclusive) in local calendar time. The daemon
//! wants both ends inclusive and in milliseconds; [`DateRange::bounds_ms`] converts.

use std::ops::Range;

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};

/// Separators between the two ends of a range. `-` is tried last because dates use it as well.
const SEPARATORS: [&str; 7] = ["..", "~", "～", "–", "—", "到", "至"];
/// How many words a range may span, e.g. `9月1日 到 15日`.
const MAX_WORDS: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DateRange {
    /// First instant of the range; `None` starts at the earliest entry.
    pub start: Option<NaiveDateTime>,
    /// First instant after the range; `None` ends at the end of today.
    pub end: Option<NaiveDateTime>,
    /// Text shown on the filter chip, e.g. "9月1日 – 15日".
    pub label: String,
}

/// A range found inside a longer text, with the byte span it was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub range: DateRange,
    pub span: Range<usize>,
}

impl DateRange {
    /// Inclusive `[from, to]` in milliseconds since the epoch, as the daemon expects. The daemon
    /// requires both ends, so an open start becomes 0 and an open end the end of `today`.
    pub fn bounds_ms(&self, today: NaiveDate) -> (i64, i64) {
        self.bounds_in(&Local, today)
    }

    fn bounds_in<Tz: TimeZone>(&self, zone: &Tz, today: NaiveDate) -> (i64, i64) {
        let millis = |time: NaiveDateTime| {
            zone.from_local_datetime(&time)
                .earliest()
                .or_else(|| zone.from_local_datetime(&time).latest())
                .map_or_else(
                    || time.and_utc().timestamp_millis(),
                    |t| t.timestamp_millis(),
                )
        };
        let from = self.start.map_or(0, millis);
        let end = self
            .end
            .unwrap_or_else(|| midnight(today + Duration::days(1)));
        (from.max(0), (millis(end) - 1).max(0))
    }
}

/// The byte span of every whitespace-separated word.
pub fn words(text: &str) -> Vec<Range<usize>> {
    let mut words = Vec::new();
    let mut start = None;
    for (index, ch) in text.char_indices() {
        match (ch.is_whitespace(), start) {
            (false, None) => start = Some(index),
            (true, Some(begin)) => {
                words.push(begin..index);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(begin) = start {
        words.push(begin..text.len());
    }
    words
}

/// The longest run of words in `text` that reads as a date range; the leftmost wins a tie.
pub fn find(text: &str, today: NaiveDate) -> Option<Found> {
    let words = words(text);
    for length in (1..=words.len().min(MAX_WORDS)).rev() {
        for first in 0..=words.len() - length {
            let span = words[first].start..words[first + length - 1].end;
            if let Some(range) = parse(&text[span.clone()], today) {
                return Some(Found { range, span });
            }
        }
    }
    None
}

/// Reads the whole of `text` as a date range.
pub fn parse(text: &str, today: NaiveDate) -> Option<DateRange> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    keyword(text, today)
        .or_else(|| open_ended(text, today))
        .or_else(|| single(text, today))
        .or_else(|| split_range(text, today))
}

fn midnight(date: NaiveDate) -> NaiveDateTime {
    date.and_time(NaiveTime::MIN)
}

fn range(
    start: Option<NaiveDateTime>,
    end: Option<NaiveDateTime>,
    label: String,
) -> Option<DateRange> {
    if let (Some(start), Some(end)) = (start, end) {
        if end <= start {
            return None;
        }
    }
    Some(DateRange { start, end, label })
}

fn days(start: NaiveDate, end_exclusive: NaiveDate, label: &str) -> Option<DateRange> {
    range(
        Some(midnight(start)),
        Some(midnight(end_exclusive)),
        label.into(),
    )
}

fn week_start(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

fn month_start(year: i32, month: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, 1)
}

fn next_month(date: NaiveDate) -> NaiveDate {
    if date.month() == 12 {
        NaiveDate::from_ymd_opt(date.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(date.year(), date.month() + 1, 1)
    }
    .unwrap_or(date)
}

/// Words with a fixed meaning: 今天, 上周, 本月, 8月, 最近3天, 3d.
fn keyword(text: &str, today: NaiveDate) -> Option<DateRange> {
    match text {
        "今天" => return days(today, today + Duration::days(1), "今天"),
        "昨天" => return days(today - Duration::days(1), today, "昨天"),
        "前天" => return days(today - Duration::days(2), today - Duration::days(1), "前天"),
        "本周" | "这周" => {
            let start = week_start(today);
            return days(start, start + Duration::days(7), "本周");
        }
        "上周" => {
            let start = week_start(today) - Duration::days(7);
            return days(start, start + Duration::days(7), "上周");
        }
        "本月" | "这个月" => {
            let start = month_start(today.year(), today.month())?;
            return days(start, next_month(start), "本月");
        }
        "上月" | "上个月" => {
            let this = month_start(today.year(), today.month())?;
            let start = month_start(
                this.year() - i32::from(this.month() == 1),
                (this.month() + 10) % 12 + 1,
            )?;
            return days(start, this, "上月");
        }
        _ => {}
    }
    if let Some(count) = recent_days(text) {
        let start = today - Duration::days(count - 1);
        return days(start, today + Duration::days(1), &format!("最近{count}天"));
    }
    let (numbers, tail) = numbers_and_tail(text)?;
    if tail != "月" {
        return None;
    }
    let (year, month) = match numbers.as_slice() {
        [(month, _)] => (None, *month),
        [(year, 4), (month, _)] => (Some(*year as i32), *month),
        _ => return None,
    };
    let year = year.unwrap_or_else(|| today.year() - i32::from(month > today.month()));
    let start = month_start(year, month)?;
    let label = if year == today.year() {
        format!("{month}月")
    } else {
        format!("{year}年{month}月")
    };
    days(start, next_month(start), &label)
}

/// `最近3天`, `近3天`, `过去3天`, `3d`.
fn recent_days(text: &str) -> Option<i64> {
    let digits = |s: &str| {
        (!s.is_empty() && s.len() <= 4 && s.chars().all(|c| c.is_ascii_digit()))
            .then(|| s.parse::<i64>().ok())
            .flatten()
            .filter(|n| *n > 0)
    };
    for prefix in ["最近", "过去", "近"] {
        if let Some(count) = text
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix('天'))
            .and_then(digits)
        {
            return Some(count);
        }
    }
    let lowered = text.to_ascii_lowercase();
    lowered.strip_suffix('d').and_then(digits)
}

/// `9.1以后`, `9.1以前`: one end, the other open.
fn open_ended(text: &str, today: NaiveDate) -> Option<DateRange> {
    for (suffix, after) in [
        ("以后", true),
        ("之后", true),
        ("以前", false),
        ("之前", false),
    ] {
        if let Some(rest) = text.strip_suffix(suffix) {
            return bounded(&parse_piece(rest, false)?, after, today);
        }
    }
    None
}

fn single(text: &str, today: NaiveDate) -> Option<DateRange> {
    let piece = parse_piece(text, false)?;
    let (start, _) = resolve_start(&piece, today)?;
    let end = piece_end(&piece, start);
    range(Some(start), Some(end), label_range(start, end, today))
}

fn split_range(text: &str, today: NaiveDate) -> Option<DateRange> {
    let candidates = SEPARATORS
        .iter()
        .flat_map(|separator| text.match_indices(separator))
        .chain(text.match_indices('-'));
    for (index, separator) in candidates {
        let left = text[..index].trim();
        let right = text[index + separator.len()..].trim();
        let found = match (left.is_empty(), right.is_empty()) {
            (true, true) => None,
            // `9.1-` runs up to today and `-9.1` starts at the earliest entry.
            (false, true) => {
                parse_piece(left, false).and_then(|piece| bounded(&piece, true, today))
            }
            (true, false) => {
                parse_piece(right, false).and_then(|piece| bounded(&piece, false, today))
            }
            (false, false) => both_ends(left, right, today),
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn both_ends(left: &str, right: &str, today: NaiveDate) -> Option<DateRange> {
    let left = parse_piece(left, false)?;
    let right = parse_piece(right, true)?;
    let (start, week) = resolve_start(&left, today)?;
    let end = resolve_end(&right, start, week, today)?;
    range(Some(start), Some(end), label_range(start, end, today))
}

/// A range with a single known end: from it to today, or from the earliest entry up to it.
fn bounded(piece: &Piece, from: bool, today: NaiveDate) -> Option<DateRange> {
    let (start, _) = resolve_start(piece, today)?;
    if from {
        let label = format!("{}起", day_label(start.date(), today.year()));
        range(Some(start), None, label)
    } else {
        let end = piece_end(piece, start);
        let label = format!(
            "{}前",
            day_label((end - Duration::seconds(1)).date(), today.year())
        );
        range(None, Some(end), label)
    }
}

/// One end of a range as typed.
#[derive(Clone, Debug)]
enum Piece {
    Date {
        year: Option<i32>,
        month: u32,
        day: u32,
        time: Option<NaiveTime>,
    },
    /// `15日`: a day of the month, which only makes sense as the end of a range.
    Day(u32),
    Time(NaiveTime),
    /// `上周一`, or `周三` as the end of a range. `week` counts weeks back from this one.
    Weekday {
        week: Option<i64>,
        day: Weekday,
    },
}

fn parse_piece(text: &str, as_end: bool) -> Option<Piece> {
    let text = text.trim();
    if let Some(piece) = weekday_piece(text, as_end) {
        return Some(piece);
    }
    let (date, time) = split_time(text);
    if date.is_empty() {
        return time.map(Piece::Time).filter(|_| as_end);
    }
    let (numbers, tail) = numbers_and_tail(date)?;
    let separators: Vec<char> = separators_between(date);
    let time_allowed =
        |piece: Piece| (time.is_none() || !matches!(piece, Piece::Day(_))).then_some(piece);
    match (numbers.as_slice(), separators.as_slice()) {
        ([(year, 4), (month, _), (day, _)], [a, b])
            if is_date_separator(*a)
                && is_date_separator(*b)
                && matches!(tail, "" | "日" | "号") =>
        {
            let year = i32::try_from(*year)
                .ok()
                .filter(|y| (1970..=2200).contains(y))?;
            valid(Some(year), *month, *day, time)
        }
        ([(month, m_len), (day, _)], [a])
            if *m_len <= 2 && is_date_separator(*a) && matches!(tail, "" | "日" | "号") =>
        {
            valid(None, *month, *day, time)
        }
        ([(day, _)], []) if matches!(tail, "日" | "号") || (as_end && tail.is_empty()) => (1
            ..=31)
            .contains(day)
            .then_some(Piece::Day(*day))
            .and_then(time_allowed),
        _ => None,
    }
}

fn valid(year: Option<i32>, month: u32, day: u32, time: Option<NaiveTime>) -> Option<Piece> {
    // Feb 29 is only checked against a real year when the year is resolved.
    let probe = year.unwrap_or(2024);
    NaiveDate::from_ymd_opt(probe, month, day)?;
    Some(Piece::Date {
        year,
        month,
        day,
        time,
    })
}

fn is_date_separator(ch: char) -> bool {
    matches!(ch, '-' | '/' | '.' | '年' | '月')
}

/// Splits a trailing `14:00` off the text.
fn split_time(text: &str) -> (&str, Option<NaiveTime>) {
    let Some(colon) = text.rfind(':') else {
        return (text, None);
    };
    let head = &text[..colon];
    let start = head
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |index| {
            index + head[index..].chars().next().map_or(1, char::len_utf8)
        });
    let hour = &head[start..];
    let minute = &text[colon + 1..];
    let time = (hour.len() <= 2 && minute.len() == 2)
        .then(|| NaiveTime::from_hms_opt(hour.parse().ok()?, minute.parse().ok()?, 0))
        .flatten();
    match time {
        Some(time) => (text[..start].trim(), Some(time)),
        None => (text, None),
    }
}

/// Digit groups with their lengths, and whatever follows the last group.
fn numbers_and_tail(text: &str) -> Option<(Vec<(u32, usize)>, &str)> {
    let mut numbers = Vec::new();
    let mut rest = text;
    loop {
        let length = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if length == 0 || length > 4 {
            return None;
        }
        numbers.push((rest[..length].parse().ok()?, length));
        rest = &rest[length..];
        let mut chars = rest.chars();
        match chars.next() {
            Some(ch)
                if is_date_separator(ch)
                    && chars.next().is_some_and(|next| next.is_ascii_digit()) =>
            {
                rest = &rest[ch.len_utf8()..];
            }
            _ => return Some((numbers, rest)),
        }
    }
}

fn separators_between(text: &str) -> Vec<char> {
    let mut separators = Vec::new();
    let mut previous_digit = false;
    let chars: Vec<char> = text.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        if ch.is_ascii_digit() {
            previous_digit = true;
            continue;
        }
        if previous_digit && chars.get(index + 1).is_some_and(char::is_ascii_digit) {
            separators.push(*ch);
        }
        previous_digit = false;
    }
    separators
}

/// `上周一`, `本周三`; a bare `周三` only as the end of a range.
fn weekday_piece(text: &str, as_end: bool) -> Option<Piece> {
    let (week, rest) = if let Some(rest) = text
        .strip_prefix("上周")
        .or_else(|| text.strip_prefix("上星期"))
    {
        (Some(1), rest)
    } else if let Some(rest) = text
        .strip_prefix("本周")
        .or_else(|| text.strip_prefix("这周"))
        .or_else(|| text.strip_prefix("本星期"))
    {
        (Some(0), rest)
    } else if as_end {
        (
            None,
            text.strip_prefix("周")
                .or_else(|| text.strip_prefix("星期"))?,
        )
    } else {
        return None;
    };
    let rest = if week.is_some() {
        rest.strip_prefix("周")
            .or_else(|| rest.strip_prefix("星期"))
            .unwrap_or(rest)
    } else {
        rest
    };
    let mut chars = rest.chars();
    let day = match (chars.next()?, chars.next()) {
        ('一', None) => Weekday::Mon,
        ('二', None) => Weekday::Tue,
        ('三', None) => Weekday::Wed,
        ('四', None) => Weekday::Thu,
        ('五', None) => Weekday::Fri,
        ('六', None) => Weekday::Sat,
        ('日' | '天', None) => Weekday::Sun,
        _ => return None,
    };
    Some(Piece::Weekday { week, day })
}

/// The start of a range, and for weekdays the Monday of the week it was named in.
fn resolve_start(piece: &Piece, today: NaiveDate) -> Option<(NaiveDateTime, Option<NaiveDate>)> {
    match piece {
        Piece::Date {
            year,
            month,
            day,
            time,
        } => {
            let date = match year {
                Some(year) => NaiveDate::from_ymd_opt(*year, *month, *day)?,
                None => latest_past(*month, *day, today)?,
            };
            Some((date.and_time(time.unwrap_or(NaiveTime::MIN)), None))
        }
        Piece::Weekday {
            week: Some(back),
            day,
        } => {
            let monday = week_start(today) - Duration::days(7 * back);
            let date = monday + Duration::days(i64::from(day.num_days_from_monday()));
            Some((midnight(date), Some(monday)))
        }
        _ => None,
    }
}

/// Where a piece ends when it is the only one: the end of its day, or a minute after its time.
fn piece_end(piece: &Piece, start: NaiveDateTime) -> NaiveDateTime {
    match piece {
        Piece::Date { time: Some(_), .. } => start + Duration::minutes(1),
        _ => midnight(start.date() + Duration::days(1)),
    }
}

/// The end of a range, given its start. A year-less end is the first matching date on or after the
/// start, so `12.20-1.5` crosses into the next year.
fn resolve_end(
    piece: &Piece,
    start: NaiveDateTime,
    week: Option<NaiveDate>,
    today: NaiveDate,
) -> Option<NaiveDateTime> {
    let day_end = |date: NaiveDate| midnight(date + Duration::days(1));
    match piece {
        Piece::Date {
            year,
            month,
            day,
            time,
        } => {
            let date = match year {
                Some(year) => NaiveDate::from_ymd_opt(*year, *month, *day)?,
                None => {
                    let found = first_on_or_after(start.date(), |d| {
                        d.month() == *month && d.day() == *day
                    })?;
                    // Crossing into the next year can only mean a date that has already happened.
                    if found.year() > start.date().year() && found > today {
                        return None;
                    }
                    found
                }
            };
            Some(time.map_or_else(|| day_end(date), |time| date.and_time(time)))
        }
        Piece::Day(day) => first_on_or_after(start.date(), |d| d.day() == *day).map(day_end),
        Piece::Time(time) => {
            let end = start.date().and_time(*time);
            (start.time() != NaiveTime::MIN).then_some(end)
        }
        Piece::Weekday {
            week: reference,
            day,
        } => {
            let monday = match (reference, week) {
                (Some(back), _) => week_start(today) - Duration::days(7 * back),
                (None, Some(monday)) => monday,
                (None, None) => week_start(start.date()),
            };
            Some(day_end(
                monday + Duration::days(i64::from(day.num_days_from_monday())),
            ))
        }
    }
}

/// The latest date on or before `today` with this month and day.
fn latest_past(month: u32, day: u32, today: NaiveDate) -> Option<NaiveDate> {
    (0..=8)
        .filter_map(|back| NaiveDate::from_ymd_opt(today.year() - back, month, day))
        .find(|date| *date <= today)
}

fn first_on_or_after(from: NaiveDate, matches: impl Fn(NaiveDate) -> bool) -> Option<NaiveDate> {
    (0..=366 * 9)
        .map(|offset| from + Duration::days(offset))
        .find(|date| matches(*date))
}

fn day_label(date: NaiveDate, reference_year: i32) -> String {
    if date.year() == reference_year {
        format!("{}月{}日", date.month(), date.day())
    } else {
        format!("{}年{}月{}日", date.year(), date.month(), date.day())
    }
}

fn label_range(start: NaiveDateTime, end: NaiveDateTime, today: NaiveDate) -> String {
    let with_time = |time: NaiveDateTime| time.time() != NaiveTime::MIN;
    let clock = |time: NaiveDateTime| time.format("%H:%M").to_string();
    if with_time(start) || with_time(end) {
        let first = day_label(start.date(), today.year());
        if start.date() == end.date() {
            return format!("{first} {} – {}", clock(start), clock(end));
        }
        let last = day_label(end.date(), start.year());
        return format!("{first} {} – {last} {}", clock(start), clock(end));
    }
    let first_day = start.date();
    let last_day = end.date() - Duration::days(1);
    if first_day == last_day {
        return day_label(first_day, today.year());
    }
    let right = if first_day.year() == last_day.year() && first_day.month() == last_day.month() {
        format!("{}日", last_day.day())
    } else {
        day_label(last_day, first_day.year())
    };
    format!("{} – {right}", day_label(first_day, today.year()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // A Tuesday. The week is Monday 9/28 to Sunday 10/4; last week is 9/21 to 9/27.
    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDateTime {
        at(y, m, d, 0, 0)
    }

    /// Parses `text` and checks the range (the end is exclusive) and the label.
    #[track_caller]
    fn check(text: &str, start: Option<NaiveDateTime>, end: Option<NaiveDateTime>, label: &str) {
        let found = parse(text, today()).unwrap_or_else(|| panic!("{text:?} should parse"));
        assert_eq!(
            (found.start, found.end, found.label.as_str()),
            (start, end, label),
            "{text:?}"
        );
    }

    #[test]
    fn a_single_date_covers_that_day() {
        check(
            "9.12",
            Some(day(2026, 9, 12)),
            Some(day(2026, 9, 13)),
            "9月12日",
        );
        check(
            "9/12",
            Some(day(2026, 9, 12)),
            Some(day(2026, 9, 13)),
            "9月12日",
        );
        check(
            "9月12日",
            Some(day(2026, 9, 12)),
            Some(day(2026, 9, 13)),
            "9月12日",
        );
        check(
            "2025-12-20",
            Some(day(2025, 12, 20)),
            Some(day(2025, 12, 21)),
            "2025年12月20日",
        );
    }

    #[test]
    fn a_missing_year_means_the_most_recent_past_date() {
        // Today is 9/29, so 10.1 has not happened this year.
        check(
            "10.1",
            Some(day(2025, 10, 1)),
            Some(day(2025, 10, 2)),
            "2025年10月1日",
        );
        check(
            "9.29",
            Some(day(2026, 9, 29)),
            Some(day(2026, 9, 30)),
            "9月29日",
        );
        // Feb 29 falls back to the last leap year.
        check(
            "2.29",
            Some(day(2024, 2, 29)),
            Some(day(2024, 3, 1)),
            "2024年2月29日",
        );
    }

    #[test]
    fn ranges_accept_every_separator() {
        let (start, end) = (Some(day(2026, 9, 1)), Some(day(2026, 9, 16)));
        for text in [
            "9.1-9.15",
            "9/1~9/15",
            "9.1～9.15",
            "9.1 – 9.15",
            "9.1..9.15",
            "9.1到9.15",
            "9.1至9.15",
            "9.1 - 9.15",
        ] {
            check(text, start, end, "9月1日 – 15日");
        }
    }

    #[test]
    fn the_end_may_leave_out_what_the_start_already_says() {
        check(
            "9月1日到15日",
            Some(day(2026, 9, 1)),
            Some(day(2026, 9, 16)),
            "9月1日 – 15日",
        );
        check(
            "9.1-15",
            Some(day(2026, 9, 1)),
            Some(day(2026, 9, 16)),
            "9月1日 – 15日",
        );
        check(
            "9月1日 到 10月2日",
            Some(day(2026, 9, 1)),
            Some(day(2026, 10, 3)),
            "9月1日 – 10月2日",
        );
    }

    #[test]
    fn a_range_can_cross_the_new_year() {
        check(
            "2025-12-20..01-05",
            Some(day(2025, 12, 20)),
            Some(day(2026, 1, 6)),
            "2025年12月20日 – 2026年1月5日",
        );
        check(
            "12.20-1.5",
            Some(day(2025, 12, 20)),
            Some(day(2026, 1, 6)),
            "2025年12月20日 – 2026年1月5日",
        );
    }

    #[test]
    fn an_end_before_the_start_is_not_a_range() {
        // The only later 9.1 is next year, which has not happened.
        assert!(parse("9.15-9.1", today()).is_none());
        assert!(parse("2026-09-15..2026-09-01", today()).is_none());
    }

    #[test]
    fn a_range_may_end_after_today_within_the_same_year() {
        check(
            "9.1-9.30",
            Some(day(2026, 9, 1)),
            Some(day(2026, 10, 1)),
            "9月1日 – 30日",
        );
    }

    #[test]
    fn one_open_end_runs_to_today_or_from_the_earliest_entry() {
        check("9.1-", Some(day(2026, 9, 1)), None, "9月1日起");
        check("9.1以后", Some(day(2026, 9, 1)), None, "9月1日起");
        check("-9.1", None, Some(day(2026, 9, 2)), "9月1日前");
        check("9.1以前", None, Some(day(2026, 9, 2)), "9月1日前");
        check("9.1之后", Some(day(2026, 9, 1)), None, "9月1日起");
    }

    #[test]
    fn months_and_years_are_read_as_whole_months() {
        check("8月", Some(day(2026, 8, 1)), Some(day(2026, 9, 1)), "8月");
        check("9月", Some(day(2026, 9, 1)), Some(day(2026, 10, 1)), "9月");
        check(
            "12月",
            Some(day(2025, 12, 1)),
            Some(day(2026, 1, 1)),
            "2025年12月",
        );
        check(
            "2025年12月",
            Some(day(2025, 12, 1)),
            Some(day(2026, 1, 1)),
            "2025年12月",
        );
    }

    #[test]
    fn weeks_start_on_monday() {
        check(
            "本周",
            Some(day(2026, 9, 28)),
            Some(day(2026, 10, 5)),
            "本周",
        );
        check(
            "上周",
            Some(day(2026, 9, 21)),
            Some(day(2026, 9, 28)),
            "上周",
        );
        check(
            "上周一到周三",
            Some(day(2026, 9, 21)),
            Some(day(2026, 9, 24)),
            "9月21日 – 23日",
        );
        check(
            "上周五",
            Some(day(2026, 9, 25)),
            Some(day(2026, 9, 26)),
            "9月25日",
        );
        check(
            "上周日",
            Some(day(2026, 9, 27)),
            Some(day(2026, 9, 28)),
            "9月27日",
        );
        check(
            "本周一-周二",
            Some(day(2026, 9, 28)),
            Some(day(2026, 9, 30)),
            "9月28日 – 29日",
        );
    }

    #[test]
    fn day_and_month_words() {
        check(
            "今天",
            Some(day(2026, 9, 29)),
            Some(day(2026, 9, 30)),
            "今天",
        );
        check(
            "昨天",
            Some(day(2026, 9, 28)),
            Some(day(2026, 9, 29)),
            "昨天",
        );
        check(
            "本月",
            Some(day(2026, 9, 1)),
            Some(day(2026, 10, 1)),
            "本月",
        );
        check("上月", Some(day(2026, 8, 1)), Some(day(2026, 9, 1)), "上月");
    }

    #[test]
    fn last_month_in_january_is_december_of_the_previous_year() {
        let january = NaiveDate::from_ymd_opt(2026, 1, 15).unwrap();
        let found = parse("上月", january).unwrap();
        assert_eq!(found.start, Some(day(2025, 12, 1)));
        assert_eq!(found.end, Some(day(2026, 1, 1)));
    }

    #[test]
    fn recent_days_include_today() {
        check(
            "最近3天",
            Some(day(2026, 9, 27)),
            Some(day(2026, 9, 30)),
            "最近3天",
        );
        check(
            "近7天",
            Some(day(2026, 9, 23)),
            Some(day(2026, 9, 30)),
            "最近7天",
        );
        check(
            "3d",
            Some(day(2026, 9, 27)),
            Some(day(2026, 9, 30)),
            "最近3天",
        );
        check(
            "30D",
            Some(day(2026, 8, 31)),
            Some(day(2026, 9, 30)),
            "最近30天",
        );
    }

    #[test]
    fn times_of_day_narrow_a_day() {
        check(
            "9.12 14:00-16:00",
            Some(at(2026, 9, 12, 14, 0)),
            Some(at(2026, 9, 12, 16, 0)),
            "9月12日 14:00 – 16:00",
        );
        check(
            "9.12 14:00",
            Some(at(2026, 9, 12, 14, 0)),
            Some(at(2026, 9, 12, 14, 1)),
            "9月12日 14:00 – 14:01",
        );
        check(
            "9.12 09:30~9.13 08:00",
            Some(at(2026, 9, 12, 9, 30)),
            Some(at(2026, 9, 13, 8, 0)),
            "9月12日 09:30 – 9月13日 08:00",
        );
    }

    #[test]
    fn a_time_needs_a_date_before_it() {
        assert!(parse("14:00", today()).is_none());
        assert!(parse("14:00-16:00", today()).is_none());
        assert!(parse("9.12 16:00-14:00", today()).is_none());
    }

    #[test]
    fn plain_words_and_numbers_are_not_dates() {
        for text in [
            "",
            " ",
            "hello",
            "2026",
            "15",
            "3",
            "13.13",
            "2.30",
            "1.2.3",
            "d",
            "0d",
            "最近0天",
            "13月",
            "https://example.com",
            "a-b",
            "-",
            "..",
            "上周八",
            "周三",
        ] {
            assert!(parse(text, today()).is_none(), "{text:?} must not parse");
        }
    }

    #[test]
    fn find_takes_the_longest_run_of_words_and_reports_its_span() {
        let text = "设计 9.12 14:00-16:00 草稿";
        let found = find(text, today()).unwrap();
        assert_eq!(&text[found.span.clone()], "9.12 14:00-16:00");
        assert_eq!(found.range.label, "9月12日 14:00 – 16:00");

        let text = "9月1日 到 15日 报告";
        let found = find(text, today()).unwrap();
        assert_eq!(&text[found.span], "9月1日 到 15日");
    }

    #[test]
    fn find_offsets_stay_valid_after_multibyte_text() {
        let text = "设计稿 上周 报告";
        let found = find(text, today()).unwrap();
        assert_eq!(&text[found.span], "上周");
        assert!(find("设计稿 报告", today()).is_none());
        assert!(find("", today()).is_none());
    }

    #[test]
    fn bounds_are_inclusive_and_never_negative() {
        let zone = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let ms = |t: NaiveDateTime| zone.from_local_datetime(&t).unwrap().timestamp_millis();
        let bounds = |text: &str| parse(text, today()).unwrap().bounds_in(&zone, today());
        assert_eq!(
            bounds("9.12"),
            (ms(day(2026, 9, 12)), ms(day(2026, 9, 13)) - 1)
        );
        // An open end reaches the end of today; an open start the epoch.
        assert_eq!(
            bounds("9.1-"),
            (ms(day(2026, 9, 1)), ms(day(2026, 9, 30)) - 1)
        );
        assert_eq!(bounds("-9.1"), (0, ms(day(2026, 9, 2)) - 1));
        // Before 1970 the daemon would refuse a negative value.
        assert_eq!(bounds("1970-01-01"), (0, ms(day(1970, 1, 2)) - 1));
    }
}
