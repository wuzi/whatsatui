//! Offline Unicode emoji search. Shortcodes are a picker aid, not wire content.
pub fn search(query: &str) -> Vec<&'static emojis::Emoji> {
    let query = query.trim().trim_matches(':').to_lowercase();
    let words = query.replace(['_', '-'], " ");
    let mut found = emojis::iter()
        .filter_map(|emoji| {
            let name = emoji.name().to_lowercase();
            let score = if emoji.as_str() == query
                || emoji.shortcodes().any(|s| s == query)
                || name == words
            {
                0
            } else if emoji.shortcodes().any(|s| s.starts_with(&query)) || name.starts_with(&words)
            {
                1
            } else if emoji.shortcodes().any(|s| s.contains(&query)) || name.contains(&words) {
                2
            } else {
                return None;
            };
            Some((score, emoji))
        })
        .collect::<Vec<_>>();
    found.sort_by_key(|(score, _)| *score);
    found.into_iter().take(80).map(|(_, emoji)| emoji).collect()
}
