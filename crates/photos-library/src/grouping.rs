use chrono::{DateTime, Datelike, NaiveDate, Utc};

use crate::catalog::Photo;

/// Calendar-granularity for browsing a collection by capture date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Month,
    Year,
}

/// One calendar period and the indices of its photos in the input collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoGroup {
    pub label: String,
    pub start: i64,
    pub end: i64,
    pub indices: Vec<usize>,
}

/// Groups selected photos newest-first using UTC calendar boundaries.
/// Invalid timestamps or indices are omitted; input order is retained within groups.
#[must_use]
pub fn groups(photos: &[Photo], indices: &[usize], period: Period) -> Vec<PhotoGroup> {
    let mut grouped = std::collections::BTreeMap::new();
    for &index in indices {
        let Some(photo) = photos.get(index) else {
            continue;
        };
        let Some(date) = DateTime::<Utc>::from_timestamp(photo.captured_at, 0) else {
            continue;
        };
        let month = if period == Period::Month {
            date.month()
        } else {
            1
        };
        let Some(start) =
            NaiveDate::from_ymd_opt(date.year(), month, 1).and_then(|d| d.and_hms_opt(0, 0, 0))
        else {
            continue;
        };
        let next = if period == Period::Year || month == 12 {
            date.year()
                .checked_add(1)
                .and_then(|y| NaiveDate::from_ymd_opt(y, 1, 1))
        } else {
            NaiveDate::from_ymd_opt(date.year(), month + 1, 1)
        };
        let Some(end) = next.and_then(|d| d.and_hms_opt(0, 0, 0)) else {
            continue;
        };
        let group = grouped
            .entry(start.and_utc().timestamp())
            .or_insert_with(|| PhotoGroup {
                label: start
                    .format(if period == Period::Year {
                        "%Y"
                    } else {
                        "%B %Y"
                    })
                    .to_string(),
                start: start.and_utc().timestamp(),
                end: end.and_utc().timestamp(),
                indices: Vec::new(),
            });
        group.indices.push(index);
    }
    grouped.into_values().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::PhotoId;

    fn photo(date: &str) -> Photo {
        Photo {
            id: PhotoId(date.into()),
            path: date.into(),
            name: date.into(),
            width: 1,
            height: 1,
            bytes: 1,
            modified_ns: 0,
            captured_at: DateTime::parse_from_rfc3339(date)
                .expect("date")
                .timestamp(),
            camera: None,
            has_location: false,
            format: "png".into(),
        }
    }

    #[test]
    fn grouping_month_and_year_boundaries_respect_visible_selection() {
        let photos = [
            photo("2025-12-31T23:59:59Z"),
            photo("2026-01-01T00:00:00Z"),
            photo("2025-12-01T00:00:00Z"),
        ];
        let result = groups(&photos, &[0, 1, 2], Period::Month);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].label, "January 2026");
        assert_eq!(result[1].indices, [0, 2]);
        assert_eq!(result[1].end, result[0].start);

        let photos = [
            photo("2026-02-12T10:00:00Z"),
            photo("2026-09-20T10:00:00Z"),
            photo("2025-10-09T10:00:00Z"),
        ];
        let result = groups(&photos, &[0, 1], Period::Year);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "2026");
        assert_eq!(result[0].indices, [0, 1]);
    }
}
