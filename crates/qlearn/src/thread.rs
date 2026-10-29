//! The threads of a page. See section 6 of the design.
//!
//! A thread lives in the page file of its page, beside the quizzes, so a
//! push of the page keeps it. Its comments are kept in Markdown, as their
//! author wrote them: the page gets the HTML of the server, and the agent
//! gets the Markdown back.

use serde::{Deserialize, Serialize};

use crate::model::{Author, BlockSource, Comment, PageKind, Thread};

/// A thread as the store keeps it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredThread {
    /// See [`id`].
    pub id: String,
    /// The block as the agent wrote it when the thread opened, or None for
    /// a thread about the whole page. A later push may take the block
    /// away, and the agent must still read what the learner asked about.
    pub block: Option<BlockSource>,
    pub quote: Option<String>,
    pub comments: Vec<StoredComment>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredComment {
    pub author: Author,
    /// Markdown.
    pub body: String,
    /// RFC 3339.
    pub at: String,
}

impl StoredThread {
    /// The thread of the page `page`, each body passed through `body`: the
    /// HTML of the server for the page, the Markdown as it is for the agent.
    pub fn view(&self, page: &str, body: impl Fn(&str) -> String) -> Thread {
        Thread {
            id: self.id.clone(),
            page: page.to_owned(),
            block: self.block.as_ref().map(|block| block.id.clone()),
            quote: self.quote.clone(),
            comments: self
                .comments
                .iter()
                .map(|comment| Comment {
                    author: comment.author,
                    body: body(&comment.body),
                    at: comment.at.clone(),
                })
                .collect(),
        }
    }

    /// The rank of the thread in its page, from 1.
    fn rank(&self) -> usize {
        split_id(&self.id).map_or(0, |(_, n)| n)
    }
}

/// The id of the thread `n` of a page: `<page>-<n>`, such as `3-2`.
///
/// A page id holds no hyphen, so the id names its page, and a route finds
/// the thread with no scan of the course. Threads are never removed, so
/// the rank stays unique in its page.
pub fn id(page: &str, n: usize) -> String {
    format!("{page}-{n}")
}

/// The page and the rank that a thread id names, or None when it names no
/// thread.
pub fn split_id(id: &str) -> Option<(&str, usize)> {
    let (page, n) = id.rsplit_once('-')?;
    PageKind::of_id(page)?;
    // `+1` and `01` parse too, and name no thread.
    let rank = n
        .parse::<usize>()
        .ok()
        .filter(|rank| rank.to_string() == n)?;
    (rank > 0).then_some((page, rank))
}

/// The id of the next thread of a page that holds `threads`.
pub fn next_id(page: &str, threads: &[StoredThread]) -> String {
    let last = threads.iter().map(StoredThread::rank).max().unwrap_or(0);
    id(page, last + 1)
}

/// The body of a comment as the store keeps it, or the reason to refuse it.
pub fn body(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("the body is empty. Write the comment in Markdown".to_owned());
    }
    Ok(text.to_owned())
}

/// The quote of a new thread as the store keeps it. An empty selection is
/// no quote.
pub fn quote(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thread_id_names_its_page_and_its_rank() {
        assert_eq!(id("7a", 2), "7a-2");
        for (id, page, rank) in [("1-1", "1", 1), ("7a-12", "7a", 12), ("c1-3", "c1", 3)] {
            assert_eq!(split_id(id), Some((page, rank)), "{id}");
        }
        for id in [
            "", "1", "1-", "-1", "1-0", "1-01", "1-+1", "x-1", "1-1-1", "../1-1",
        ] {
            assert_eq!(split_id(id), None, "{id:?}");
        }
    }

    fn thread(id: &str) -> StoredThread {
        StoredThread {
            id: id.to_owned(),
            block: None,
            quote: None,
            comments: Vec::new(),
        }
    }

    #[test]
    fn the_next_thread_takes_the_rank_after_the_last() {
        assert_eq!(next_id("3", &[]), "3-1");
        assert_eq!(next_id("3", &[thread("3-1"), thread("3-4")]), "3-5");
    }

    #[test]
    fn an_empty_body_is_refused_and_an_empty_quote_is_none() {
        assert!(body(" \n\t").is_err());
        assert_eq!(
            body("  Why a *random* wait?\n").unwrap(),
            "Why a *random* wait?"
        );
        assert_eq!(quote(Some("  \n")), None);
        assert_eq!(quote(None), None);
        assert_eq!(quote(Some(" rand() ")).as_deref(), Some("rand()"));
    }
}
