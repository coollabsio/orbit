//! Mentions in task descriptions and comments. The text has `@Name` or `@handle` (the part of
//! the email before the `@`), as the web app's markdown renderer reads it. The server reads the
//! text itself, so every client and every edit follows one rule.

use orbit_platform::Id;
use sqlx::{Row, Sqlite, Transaction};

/// A text that names a member: the lower-case name or handle, and the member when exactly one
/// has it.
type Label = (String, Option<Id>);

/// After a mention, and before its `@`: the end of the text, white space, or punctuation.
fn is_boundary(char: Option<char>) -> bool {
    char.is_none_or(|char| char.is_whitespace() || ".,!?;:()[]{}\"'`".contains(char))
}

/// The members that `text` mentions, each one time, in the order of the text. Fenced code
/// blocks and inline code are not read. A label that more than one member has names nobody.
fn find_mentions(text: &str, labels: &[Label]) -> Vec<Id> {
    let mut found = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let lower = line.to_lowercase();
        let mut in_code = false;
        let mut previous = None;
        for (at, char) in lower.char_indices() {
            if char == '`' {
                in_code = !in_code;
            } else if char == '@' && !in_code && is_boundary(previous) {
                let rest = &lower[at + 1..];
                // The longest label wins: "@Ada Lovelace" is not "@Ada".
                let label = labels
                    .iter()
                    .filter(|(label, _)| {
                        rest.starts_with(label.as_str())
                            && is_boundary(rest[label.len()..].chars().next())
                    })
                    .max_by_key(|(label, _)| label.len());
                if let Some((_, Some(user_id))) = label
                    && !found.contains(user_id)
                {
                    found.push(*user_id);
                }
            }
            previous = Some(char);
        }
    }
    found
}

/// The active members of the workspace that `text` mentions.
pub(super) async fn mentioned_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    text: &str,
) -> Result<Vec<Id>, sqlx::Error> {
    if !text.contains('@') {
        return Ok(Vec::new());
    }
    let members = sqlx::query(
        "SELECT users.id, users.display_name, users.email FROM memberships \
         JOIN users ON users.id = memberships.user_id \
         WHERE memberships.workspace_id = ? AND users.suspended_at IS NULL",
    )
    .bind(workspace_id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    let mut labels: Vec<Label> = Vec::new();
    for member in &members {
        let Ok(id) = member.get::<String, _>("id").parse::<Id>() else {
            continue;
        };
        let name: String = member.get("display_name");
        let email: String = member.get("email");
        let handle = email.split('@').next().unwrap_or_default();
        for label in [name.trim().to_lowercase(), handle.to_lowercase()] {
            if label.is_empty() {
                continue;
            }
            match labels.iter_mut().find(|(known, _)| *known == label) {
                // The name of one member and the handle of the same member can be equal.
                Some((_, owner)) if *owner != Some(id) => *owner = None,
                Some(_) => {}
                None => labels.push((label, Some(id))),
            }
        }
    }
    Ok(find_mentions(text, &labels))
}

/// The members that `after` mentions and `before` did not: an edit notifies only them.
pub(super) async fn newly_mentioned_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Id,
    before: &str,
    after: &str,
) -> Result<Vec<Id>, sqlx::Error> {
    let now = mentioned_in_tx(tx, workspace_id, after).await?;
    if now.is_empty() {
        return Ok(now);
    }
    let earlier = mentioned_in_tx(tx, workspace_id, before).await?;
    Ok(now.into_iter().filter(|id| !earlier.contains(id)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_are_read_by_name_or_handle_outside_code() {
        let [ada, grace, twin]: [Id; 3] = std::array::from_fn(|_| Id::new_v7());
        let labels = vec![
            ("ada lovelace".to_owned(), Some(ada)),
            ("ada".to_owned(), Some(ada)),
            ("grace".to_owned(), Some(grace)),
            ("sam".to_owned(), None),
            ("sam.one".to_owned(), Some(twin)),
        ];
        let find = |text: &str| find_mentions(text, &labels);
        assert_eq!(find("Thanks @Ada Lovelace, and @GRACE!"), [ada, grace]);
        assert_eq!(find("(@grace) @grace @ada."), [grace, ada]);
        // Not a mention: inside a word, an unknown name, a longer word, a name of two members.
        assert!(find("mail a@grace.com, @nobody, @gracefully, @sam").is_empty());
        assert_eq!(find("@sam.one"), [twin]);
        // Code is not read.
        assert!(find("`@ada` and\n```\n@grace\n```\n~~~\n@ada\n~~~").is_empty());
        assert_eq!(find("`code` @ada\n```\n@grace\n```\n@grace"), [ada, grace]);
        assert!(find("").is_empty());
    }
}
