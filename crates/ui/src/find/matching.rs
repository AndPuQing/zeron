//! Linear, non-overlapping find matches with original UTF-8 byte offsets.

pub(crate) fn match_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let needle: Vec<char> = query.trim().to_lowercase().chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    // Keep the existing per-character lowercase comparison. Offsets always
    // refer to the original text, including characters whose case expands.
    let chars: Vec<(usize, char)> = text
        .char_indices()
        .map(|(offset, ch)| (offset, ch.to_lowercase().next().unwrap()))
        .collect();
    let mut prefix = vec![0; needle.len()];
    let mut matched = 0;
    for i in 1..needle.len() {
        while matched > 0 && needle[i] != needle[matched] {
            matched = prefix[matched - 1];
        }
        if needle[i] == needle[matched] {
            matched += 1;
        }
        prefix[i] = matched;
    }
    let mut ranges = Vec::new();
    matched = 0;
    for (i, &(_, ch)) in chars.iter().enumerate() {
        while matched > 0 && ch != needle[matched] {
            matched = prefix[matched - 1];
        }
        if ch == needle[matched] {
            matched += 1;
        }
        if matched == needle.len() {
            let start = chars[i + 1 - matched].0;
            let end = chars.get(i + 1).map_or(text.len(), |&(offset, _)| offset);
            ranges.push(start..end);
            matched = 0; // Existing find navigation counts non-overlapping matches.
        }
    }
    ranges
}

#[cfg(test)]
pub(crate) fn char_range(text: &str, query: &str) -> Option<(usize, usize)> {
    match_ranges(text, query)
        .first()
        .map(|range| (range.start, range.end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_case_boundaries_and_non_overlapping_semantics() {
        assert_eq!(char_range("naïve MAIN", "main"), Some((7, 11)));
        assert_eq!(match_ranges("aaaaa", "aa"), vec![0..2, 2..4]);
        assert_eq!(match_ranges("安卓🙂安卓", "安卓"), vec![0..6, 10..16]);
        assert_eq!(match_ranges("Éé İ", "é"), vec![0..2, 2..4]);
        assert_eq!(match_ranges("Éé İ", "i"), vec![5..7]);
        assert!(match_ranges("abc", "  ").is_empty());
        assert!(match_ranges("", "x").is_empty());
        assert!(match_ranges("aa", "aaa").is_empty());
        assert_eq!(match_ranges("ababaabababac", "ababac"), vec![7..13]);
    }

    // An independent naive oracle catches prefix fallback and boundary bugs.
    fn reference(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
        let needle: Vec<_> = query.trim().to_lowercase().chars().collect();
        if needle.is_empty() {
            return vec![];
        }
        let chars: Vec<_> = text.char_indices().collect();
        let mut result = vec![];
        let mut i = 0;
        while i + needle.len() <= chars.len() {
            if needle
                .iter()
                .enumerate()
                .all(|(j, ch)| chars[i + j].1.to_lowercase().next() == Some(*ch))
            {
                result.push(chars[i].0..chars.get(i + needle.len()).map_or(text.len(), |c| c.0));
                i += needle.len();
            } else {
                i += 1;
            }
        }
        result
    }

    #[test]
    fn matches_reference_for_mixed_unicode_and_repeated_prefixes() {
        let alphabet = ['a', 'A', 'b', 'é', 'É', '安', '🙂', 'İ', 'Σ', ' '];
        let mut seed = 7u64;
        for n in 0..2000 {
            let mut make = |len| -> String {
                (0..len)
                    .map(|_| {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        alphabet[(seed >> 32) as usize % alphabet.len()]
                    })
                    .collect()
            };
            let text = make(n % 60);
            let query = make(n % 12);
            assert_eq!(
                match_ranges(&text, &query),
                reference(&text, &query),
                "{text:?} / {query:?}"
            );
        }
    }

    #[test]
    fn long_dense_matches_and_long_near_misses() {
        let text = "a ".repeat(100_000);
        let hits = match_ranges(&text, "a");
        assert_eq!(hits.len(), 100_000);
        assert_eq!(hits.last(), Some(&(199_998..199_999)));
        let text = "a".repeat(200_000);
        assert!(match_ranges(&text, &("a".repeat(10_000) + "b")).is_empty());
    }
}
