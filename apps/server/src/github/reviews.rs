//! Writing to a pull request as the member: review comments, replies, threads, the review itself, merge and branch
//! delete. Pending comments use GitHub's own pending review, so a review started in Orbit continues on GitHub and back.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use super::pulls::{
    PendingReview, PullRef, ReviewComment, ReviewThread, comment_fields, review_comment, review_thread,
    thread_fields, threads,
};
use super::{GithubClient, GithubError};

/// GitHub node ids: letters, digits, `_`, `-` and `=`.
#[must_use]
pub fn valid_node_id(id: &str) -> bool {
    (1..=100).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'='))
}

/// What a write needs to know about the pull request.
pub struct Basics {
    pub id: String,
    pub head_sha: String,
    pub is_author: bool,
}

const BASICS_QUERY: &str = "query PullBasics($owner: String!, $repo: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $repo) { pullRequest(number: $number) { id headRefOid author { login } } }
}";

pub async fn basics(github: &GithubClient, token: &str, pull: &PullRef) -> Result<Basics, GithubError> {
    let data: Value = github.graphql(token, BASICS_QUERY, json!({ "owner": pull.owner, "repo": pull.repo, "number": pull.number })).await?;
    let pr = &data["repository"]["pullRequest"];
    let id = pr["id"].as_str().ok_or(GithubError::NotFound)?.to_owned();
    Ok(Basics {
        id,
        head_sha: pr["headRefOid"].as_str().unwrap_or_default().to_owned(),
        is_author: pr["author"]["login"].as_str().is_some() && pr["author"]["login"] == data["viewer"]["login"],
    })
}

const PENDING_QUERY: &str = "query PendingReview($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) { pullRequest(number: $number) {
    reviews(states: [PENDING], first: 20) { nodes { id viewerDidAuthor comments { totalCount } } }
  } }
}";

/// The member's own pending review, if they started one (in Orbit or on GitHub).
pub async fn pending_review(github: &GithubClient, token: &str, pull: &PullRef) -> Result<Option<PendingReview>, GithubError> {
    let data: Value = github.graphql(token, PENDING_QUERY, json!({ "owner": pull.owner, "repo": pull.repo, "number": pull.number })).await?;
    Ok(data["repository"]["pullRequest"]["reviews"]["nodes"]
        .as_array()
        .and_then(|nodes| nodes.iter().find(|node| node["viewerDidAuthor"] == true))
        .and_then(|node| {
            Some(PendingReview {
                id: node["id"].as_str()?.to_owned(),
                comment_count: node["comments"]["totalCount"].as_i64().unwrap_or(0),
            })
        }))
}

const ADD_REVIEW: &str = "mutation AddReview($pullId: ID!, $commit: GitObjectID!, $event: PullRequestReviewEvent, $body: String) {
  addPullRequestReview(input: { pullRequestId: $pullId, commitOID: $commit, event: $event, body: $body }) { pullRequestReview { id } }
}";

async fn add_review(github: &GithubClient, token: &str, basics: &Basics, event: Option<&str>, body: Option<&str>) -> Result<String, GithubError> {
    let data: Value = github
        .graphql(token, ADD_REVIEW, json!({ "pullId": basics.id, "commit": basics.head_sha, "event": event, "body": body }))
        .await?;
    data["addPullRequestReview"]["pullRequestReview"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or(GithubError::Unexpected(200))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = GithubNewReviewComment)]
pub struct NewReviewComment {
    pub path: String,
    /// `LEFT` (old file) or `RIGHT` (new file).
    pub side: String,
    pub line: i64,
    pub start_side: Option<String>,
    pub start_line: Option<i64>,
    /// Markdown.
    pub body: String,
    /// `pending` (in the member's review, seen when it is submitted) or `single` (posted now).
    pub mode: String,
    /// The head commit of the diff the member commented on. When the pull request has moved on, the comment is
    /// refused (409) instead of landing on another line.
    pub head_sha: Option<String>,
}

impl NewReviewComment {
    /// Sides are `LEFT`/`RIGHT`, lines positive, a range starts before it ends, the body is not empty.
    #[must_use]
    pub fn valid(&self) -> bool {
        let side = |value: &str| matches!(value, "LEFT" | "RIGHT");
        side(&self.side)
            && self.start_side.as_deref().is_none_or(side)
            && self.line > 0
            && self.start_line.is_none_or(|start| start > 0 && start <= self.line)
            && self.start_line.is_some() == self.start_side.is_some()
            && !self.body.trim().is_empty()
            && self.body.len() <= 65_536
            && !self.path.is_empty()
            && self.path.len() <= 4096
            && matches!(self.mode.as_str(), "pending" | "single")
            && self
                .head_sha
                .as_deref()
                .is_none_or(|sha| (1..=64).contains(&sha.len()) && sha.bytes().all(|c| c.is_ascii_alphanumeric()))
    }
}

const ADD_THREAD: &str = concat!(
    "mutation AddThread($reviewId: ID!, $path: String!, $line: Int!, $side: DiffSide!, $startLine: Int, $startSide: DiffSide, $body: String!) {\n",
    "  addPullRequestReviewThread(input: { pullRequestReviewId: $reviewId, path: $path, line: $line, side: $side, startLine: $startLine, startSide: $startSide, body: $body, subjectType: LINE }) {\n",
    "    thread { ",
    thread_fields!(),
    " }\n  }\n}"
);

pub async fn add_comment(github: &GithubClient, token: &str, pull: &PullRef, comment: &NewReviewComment) -> Result<ReviewThread, GithubError> {
    let basics = basics(github, token, pull).await?;
    if comment.head_sha.as_deref().is_some_and(|seen| seen != basics.head_sha) {
        return Err(GithubError::Conflict("New commits were pushed.".to_owned()));
    }
    if comment.mode == "single" {
        let mut body = json!({ "commit_id": basics.head_sha, "path": comment.path, "body": comment.body, "line": comment.line, "side": comment.side });
        if let (Some(start), Some(side)) = (comment.start_line, &comment.start_side) {
            body["start_line"] = json!(start);
            body["start_side"] = json!(side);
        }
        let created: Value = github
            .rest(token, Method::POST, &format!("repos/{}/{}/pulls/{}/comments", pull.owner, pull.repo, pull.number), Some(&body))
            .await?;
        let id = created["id"].as_i64().ok_or(GithubError::Unexpected(201))?;
        // REST answers with the comment; the API answers with its thread.
        return threads(github, token, pull)
            .await?
            .into_iter()
            .find(|thread| thread.comments.iter().any(|item| item.database_id == id))
            .ok_or(GithubError::Unexpected(201));
    }
    let review = match pending_review(github, token, pull).await? {
        Some(review) => review.id,
        None => add_review(github, token, &basics, None, None).await?,
    };
    let data: Value = github
        .graphql(
            token,
            ADD_THREAD,
            json!({
                "reviewId": review, "path": comment.path, "line": comment.line, "side": comment.side,
                "startLine": comment.start_line, "startSide": comment.start_side, "body": comment.body
            }),
        )
        .await?;
    let thread = &data["addPullRequestReviewThread"]["thread"];
    if thread.is_null() {
        return Err(GithubError::Unprocessable("GitHub could not add the comment on this line.".to_owned()));
    }
    Ok(review_thread(thread))
}

const ADD_REPLY: &str = concat!(
    "mutation AddReply($threadId: ID!, $body: String!, $reviewId: ID) {\n",
    "  addPullRequestReviewThreadReply(input: { pullRequestReviewThreadId: $threadId, body: $body, pullRequestReviewId: $reviewId }) {\n",
    "    comment { ",
    comment_fields!(),
    " }\n  }\n}"
);

/// A reply; with a pending review it goes into that review, as on GitHub.
pub async fn reply(github: &GithubClient, token: &str, pull: &PullRef, thread: &str, body: &str) -> Result<ReviewComment, GithubError> {
    let review = pending_review(github, token, pull).await?.map(|review| review.id);
    let data: Value = github
        .graphql(token, ADD_REPLY, json!({ "threadId": thread, "body": body, "reviewId": review }))
        .await?;
    let comment = &data["addPullRequestReviewThreadReply"]["comment"];
    if comment.is_null() {
        return Err(GithubError::NotFound);
    }
    Ok(review_comment(comment))
}

const UPDATE_COMMENT: &str = concat!(
    "mutation UpdateComment($id: ID!, $body: String!) {\n",
    "  updatePullRequestReviewComment(input: { pullRequestReviewCommentId: $id, body: $body }) { pullRequestReviewComment { ",
    comment_fields!(),
    " } }\n}"
);

pub async fn update_comment(github: &GithubClient, token: &str, id: &str, body: &str) -> Result<ReviewComment, GithubError> {
    let data: Value = github.graphql(token, UPDATE_COMMENT, json!({ "id": id, "body": body })).await?;
    let comment = &data["updatePullRequestReviewComment"]["pullRequestReviewComment"];
    if comment.is_null() {
        return Err(GithubError::NotFound);
    }
    Ok(review_comment(comment))
}

pub async fn delete_comment(github: &GithubClient, token: &str, id: &str) -> Result<(), GithubError> {
    let _: Value = github
        .graphql(token, "mutation DeleteComment($id: ID!) { deletePullRequestReviewComment(input: { id: $id }) { pullRequestReview { id } } }", json!({ "id": id }))
        .await?;
    Ok(())
}

const RESOLVE: &str = concat!("mutation ResolveThread($id: ID!) { resolveReviewThread(input: { threadId: $id }) { thread { ", thread_fields!(), " } } }");
const UNRESOLVE: &str = concat!("mutation UnresolveThread($id: ID!) { unresolveReviewThread(input: { threadId: $id }) { thread { ", thread_fields!(), " } } }");

pub async fn set_resolved(github: &GithubClient, token: &str, thread: &str, resolved: bool) -> Result<ReviewThread, GithubError> {
    let (query, field) = if resolved { (RESOLVE, "resolveReviewThread") } else { (UNRESOLVE, "unresolveReviewThread") };
    let data: Value = github.graphql(token, query, json!({ "id": thread })).await?;
    let node = &data[field]["thread"];
    if node.is_null() {
        return Err(GithubError::NotFound);
    }
    Ok(review_thread(node))
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = GithubSubmitReview)]
pub struct SubmitReview {
    /// `COMMENT`, `APPROVE` or `REQUEST_CHANGES`.
    pub event: String,
    /// The review summary (markdown); may be empty for an approval.
    pub body: String,
}

/// Why a review cannot be submitted, before GitHub is asked.
pub enum ReviewRefusal {
    OwnPull,
    Empty,
}

/// Submits the member's pending review, or makes a new review with the event when there is none.
pub async fn submit_review(github: &GithubClient, token: &str, pull: &PullRef, review: &SubmitReview) -> Result<Result<(), ReviewRefusal>, GithubError> {
    let basics = basics(github, token, pull).await?;
    if basics.is_author && review.event != "COMMENT" {
        return Ok(Err(ReviewRefusal::OwnPull));
    }
    let pending = pending_review(github, token, pull).await?;
    if review.event != "APPROVE" && review.body.trim().is_empty() && pending.as_ref().is_none_or(|item| item.comment_count == 0) {
        return Ok(Err(ReviewRefusal::Empty));
    }
    match pending {
        Some(pending) => {
            let _: Value = github
                .graphql(
                    token,
                    "mutation SubmitReview($reviewId: ID!, $event: PullRequestReviewEvent!, $body: String) { submitPullRequestReview(input: { pullRequestReviewId: $reviewId, event: $event, body: $body }) { pullRequestReview { id } } }",
                    json!({ "reviewId": pending.id, "event": review.event, "body": review.body }),
                )
                .await?;
        }
        None => {
            add_review(github, token, &basics, Some(&review.event), Some(&review.body)).await?;
        }
    }
    Ok(Ok(()))
}

/// Deletes the member's pending review and its comments. `false` when there is none.
pub async fn discard_review(github: &GithubClient, token: &str, pull: &PullRef) -> Result<bool, GithubError> {
    let Some(pending) = pending_review(github, token, pull).await? else { return Ok(false) };
    let _: Value = github
        .graphql(token, "mutation DeleteReview($id: ID!) { deletePullRequestReview(input: { pullRequestReviewId: $id }) { pullRequestReview { id } } }", json!({ "id": pending.id }))
        .await?;
    Ok(true)
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = GithubMergePull)]
pub struct MergePull {
    /// `merge`, `squash` or `rebase`.
    pub method: String,
    pub commit_title: Option<String>,
    pub commit_message: Option<String>,
    /// The head commit the member saw; GitHub refuses the merge when new commits came in.
    pub expected_head_sha: String,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubMergeResult)]
pub struct MergeResult {
    /// The merge commit.
    pub sha: String,
}

pub async fn merge(github: &GithubClient, token: &str, pull: &PullRef, merge: &MergePull) -> Result<MergeResult, GithubError> {
    let mut body = json!({ "merge_method": merge.method, "sha": merge.expected_head_sha });
    if let Some(title) = merge.commit_title.as_deref().filter(|title| !title.trim().is_empty()) {
        body["commit_title"] = json!(title);
    }
    if let Some(message) = &merge.commit_message {
        body["commit_message"] = json!(message);
    }
    let reply: Value = github
        .rest(token, Method::PUT, &format!("repos/{}/{}/pulls/{}/merge", pull.owner, pull.repo, pull.number), Some(&body))
        .await?;
    Ok(MergeResult { sha: reply["sha"].as_str().unwrap_or_default().to_owned() })
}

/// Deletes the head branch of a merged pull request whose branch is in the same repository. `false` when that is not so.
pub async fn delete_branch(github: &GithubClient, token: &str, pull: &PullRef) -> Result<bool, GithubError> {
    let reply: Value = github
        .rest(token, Method::GET, &format!("repos/{}/{}/pulls/{}", pull.owner, pull.repo, pull.number), None)
        .await?;
    let same_repo = reply["head"]["repo"]["full_name"].as_str().map(str::to_lowercase) == reply["base"]["repo"]["full_name"].as_str().map(str::to_lowercase);
    let branch = reply["head"]["ref"].as_str().unwrap_or_default();
    if reply["merged"] != true || !same_repo || branch.is_empty() {
        return Ok(false);
    }
    let encoded: Vec<String> = branch
        .split('/')
        .map(|segment| url::form_urlencoded::byte_serialize(segment.as_bytes()).collect::<String>().replace('+', "%20"))
        .collect();
    github
        .rest_empty(token, Method::DELETE, &format!("repos/{}/{}/git/refs/heads/{}", pull.owner, pull.repo, encoded.join("/")), None)
        .await?;
    Ok(true)
}
