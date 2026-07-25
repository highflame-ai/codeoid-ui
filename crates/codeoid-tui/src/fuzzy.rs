//! Tiny fuzzy subsequence matcher, ported from pi-tui's `fuzzy.ts` scoring.
//!
//! [`score`] returns `None` when the query isn't a subsequence of the
//! candidate, and otherwise a number where **lower is better**. The weights
//! mirror the reference implementation:
//!
//! * exact match (case-insensitive) → large bonus
//! * consecutive matched characters → bonus
//! * gaps between matches → penalty
//! * match at a word boundary (start, after punctuation, or camelCase) → bonus
//! * later match positions → tiny penalty (prefer earlier hits)
//!
//! Matching is ASCII-case-insensitive; non-ASCII compares by exact codepoint
//! (fine for command names and file paths, which is all we feed it).

/// Bonus applied when the whole candidate equals the query (ignoring case).
const EXACT_BONUS: f64 = -100.0;
/// Bonus per matched character that sits on a word boundary.
const BOUNDARY_BONUS: f64 = -10.0;
/// Bonus when a matched character immediately follows the previous match.
const CONSECUTIVE_BONUS: f64 = -5.0;
/// Penalty per skipped character between two matches.
const GAP_PENALTY: f64 = 2.0;
/// Penalty per index of a match's position (nudges earlier matches up).
const POSITION_PENALTY: f64 = 0.1;

/// Score a fuzzy match of `needle` against `haystack`. `None` if `needle`
/// is not a subsequence of `haystack`; otherwise lower is a better match.
#[must_use]
pub fn score(needle: &str, haystack: &str) -> Option<f64> {
    if needle.is_empty() {
        return Some(0.0);
    }
    if haystack.eq_ignore_ascii_case(needle) {
        return Some(EXACT_BONUS);
    }

    let hay: Vec<char> = haystack.chars().collect();
    let mut total = 0.0;
    let mut h = 0usize;
    let mut last_match: Option<usize> = None;

    for nc in needle.chars() {
        let target = nc.to_ascii_lowercase();
        let idx = loop {
            if h >= hay.len() {
                return None;
            }
            if hay[h].to_ascii_lowercase() == target {
                break h;
            }
            h += 1;
        };

        if is_boundary(&hay, idx) {
            total += BOUNDARY_BONUS;
        }
        match last_match {
            Some(prev) if prev + 1 == idx => total += CONSECUTIVE_BONUS,
            Some(prev) => total += GAP_PENALTY * saturating_gap(prev, idx),
            None => {}
        }
        total += POSITION_PENALTY * idx_as_f64(idx);

        last_match = Some(idx);
        h = idx + 1;
    }

    Some(total)
}

fn is_boundary(hay: &[char], idx: usize) -> bool {
    if idx == 0 {
        return true;
    }
    let prev = hay[idx - 1];
    !prev.is_alphanumeric() || (prev.is_ascii_lowercase() && hay[idx].is_ascii_uppercase())
}

fn saturating_gap(prev: usize, idx: usize) -> f64 {
    idx_as_f64(idx.saturating_sub(prev).saturating_sub(1))
}

// Indices in candidate strings are tiny (command names, path segments), so
// the lossy-cast lint doesn't apply in practice — isolate it here.
#[allow(clippy::cast_precision_loss)]
fn idx_as_f64(n: usize) -> f64 {
    n as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_needle_matches_anything() {
        assert_eq!(score("", "anything"), Some(0.0));
    }

    #[test]
    fn non_subsequence_returns_none() {
        assert!(score("xyz", "export").is_none());
        assert!(score("zzz", "").is_none());
    }

    #[test]
    fn exact_match_wins_big() {
        let exact = score("help", "help").unwrap();
        let fuzzy = score("hlp", "help").unwrap();
        assert!(exact < fuzzy);
        assert!((exact - EXACT_BONUS).abs() < f64::EPSILON);
    }

    #[test]
    fn exact_is_case_insensitive() {
        assert_eq!(score("HELP", "help"), Some(EXACT_BONUS));
    }

    #[test]
    fn prefix_beats_scattered() {
        // "mo" as a prefix of "model" should rank better (lower) than the
        // same chars scattered through "autonomous".
        let prefix = score("mo", "model").unwrap();
        let scattered = score("mo", "autonomous").unwrap();
        assert!(prefix < scattered, "prefix {prefix} scattered {scattered}");
    }

    #[test]
    fn word_boundary_rewarded() {
        // Matching the start of a segment after a separator beats a match
        // buried mid-word.
        let boundary = score("nb", "new-branch").unwrap(); // n(start) b(after '-')
        let buried = score("ew", "new").unwrap(); // e,w mid-word
        assert!(boundary < buried);
    }

    #[test]
    fn consecutive_beats_gapped() {
        let consecutive = score("ex", "export").unwrap();
        let gapped = score("et", "export").unwrap(); // e...t with a gap
        assert!(consecutive < gapped);
    }
}
