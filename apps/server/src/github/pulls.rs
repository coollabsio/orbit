//! Reading pull requests from GitHub with the member's token, in the shapes the Orbit API returns. Nothing here is
//! stored: every answer comes from GitHub and is that member's view.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use super::{GithubClient, GithubError};

/// GitHub's own limit for the files of one pull request.
const MAX_FILE_PAGES: u32 = 30;
const LIST_PAGE_SIZE: u32 = 25;
/// Larger files are not shown in Orbit (open them on GitHub).
const MAX_CONTENT_BYTES: usize = 1024 * 1024;

/// `owner/repo` and the number of one pull request; names are already checked.
#[derive(Clone, Debug)]
pub struct PullRef {
    pub owner: String,
    pub repo: String,
    pub number: i64,
}

impl PullRef {
    /// `owner/repo`, lower-case: the key Orbit's project connections use.
    #[must_use]
    pub fn repository(&self) -> String {
        format!("{}/{}", self.owner, self.repo).to_lowercase()
    }

    fn rest(&self, rest: &str) -> String {
        format!("repos/{}/{}/pulls/{}{rest}", self.owner, self.repo, self.number)
    }

    fn variables(&self) -> Value {
        json!({ "owner": self.owner, "repo": self.repo, "number": self.number })
    }
}

/// A GitHub owner or repository name: letters, digits, `-`, `_` and `.`, not `.` or `..`.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    (1..=100).contains(&name.len())
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}

/// A path inside the repository, for `contents`: no empty, `.` or `..` segments, no `?` or `#`.
#[must_use]
pub fn valid_file_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.contains(['?', '#', '\\', '\0'])
        && path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

#[derive(Serialize, ToSchema, Clone)]
#[schema(as = GithubActor)]
pub struct GithubActor {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubPullSummary)]
pub struct PullSummary {
    repository: String,
    number: i64,
    title: String,
    /// `open`, `draft`, `merged` or `closed`.
    state: String,
    author: GithubActor,
    /// Unix milliseconds.
    updated_at: i64,
    comments: i64,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubPullList)]
pub struct PullList {
    items: Vec<PullSummary>,
    /// Pass back as `cursor` for the next page; `null` on the last page.
    next_cursor: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubCheckRun)]
pub struct CheckRun {
    name: String,
    /// `success`, `failure`, `pending` or `neutral`.
    state: String,
    url: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubChecksSummary)]
pub struct ChecksSummary {
    /// `success`, `failure`, `pending` or `none`.
    state: String,
    runs: Vec<CheckRun>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubMergeInfo)]
pub struct MergeInfo {
    /// The merge methods the repository allows: `merge`, `squash`, `rebase`.
    methods: Vec<String>,
    /// `mergeable`, `conflicting` or `unknown` (GitHub still computes it).
    mergeable: String,
    /// Why merging is not possible now, for the person.
    blocked_reason: Option<String>,
    default_method: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubPullViewer)]
pub struct PullViewer {
    can_comment: bool,
    can_review: bool,
    can_merge: bool,
    is_author: bool,
}

#[derive(Serialize, ToSchema, Clone)]
#[schema(as = GithubPullTaskRef)]
pub struct PullTaskRef {
    pub task_id: String,
    pub title: String,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubPullDetail)]
pub struct PullDetail {
    repository: String,
    number: i64,
    /// GitHub's node id of the pull request.
    node_id: String,
    title: String,
    /// Markdown.
    body: String,
    /// `open`, `draft`, `merged` or `closed`.
    state: String,
    author: GithubActor,
    base_ref: String,
    head_ref: String,
    base_sha: String,
    head_sha: String,
    additions: i64,
    deletions: i64,
    changed_files: i64,
    html_url: String,
    checks: ChecksSummary,
    merge: MergeInfo,
    viewer: PullViewer,
    /// Orbit tasks linked to this pull request that the member can see.
    pub tasks: Vec<PullTaskRef>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubPullFile)]
pub struct PullFile {
    path: String,
    previous_path: Option<String>,
    /// GitHub's status: `added`, `removed`, `modified`, `renamed`, `copied`, `changed` or `unchanged`.
    status: String,
    additions: i64,
    deletions: i64,
    /// The unified diff hunks; `null` when GitHub does not send them (binary or very large files).
    patch: Option<String>,
    viewed: bool,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubFileContents)]
pub struct FileContents {
    /// `null` for binary files and files over 1 MiB.
    text: Option<String>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubReviewComment)]
pub struct ReviewComment {
    /// GitHub's node id.
    pub id: String,
    pub database_id: i64,
    pub author: GithubActor,
    /// Markdown.
    pub body: String,
    /// Unix milliseconds.
    pub created_at: i64,
    /// In the member's pending review (not visible to others yet).
    pub pending: bool,
    pub can_edit: bool,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubReviewThread)]
pub struct ReviewThread {
    /// GitHub's node id.
    pub id: String,
    pub path: String,
    /// `LEFT` (the old file) or `RIGHT` (the new file).
    pub side: String,
    /// `null` when the thread is outdated and no longer on a line.
    pub line: Option<i64>,
    pub start_line: Option<i64>,
    pub start_side: Option<String>,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub can_resolve: bool,
    pub can_reply: bool,
    pub comments: Vec<ReviewComment>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubPendingReview)]
pub struct PendingReview {
    pub id: String,
    pub comment_count: i64,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubPullThreads)]
pub struct PullThreads {
    pub threads: Vec<ReviewThread>,
    pub pending_review: Option<PendingReview>,
}

#[derive(Serialize, ToSchema)]
#[schema(as = GithubPullTimelineItem)]
pub struct PullTimelineItem {
    /// `comment` or `review`.
    kind: String,
    author: GithubActor,
    body: String,
    created_at: i64,
    /// For reviews: `approved`, `changes_requested` or `commented`.
    review_state: Option<String>,
}

/// GitHub ISO 8601 time → Unix milliseconds (0 when missing or unreadable).
fn millis(value: &Value) -> i64 {
    value
        .as_str()
        .and_then(|text| text.parse::<jiff::Timestamp>().ok())
        .map_or(0, jiff::Timestamp::as_millisecond)
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn int(value: &Value) -> i64 {
    value.as_i64().unwrap_or_default()
}

/// A GraphQL `author` (`null` for deleted accounts: "ghost", as GitHub shows).
fn actor(value: &Value) -> GithubActor {
    GithubActor {
        login: value["login"].as_str().unwrap_or("ghost").to_owned(),
        avatar_url: value["avatarUrl"]
            .as_str()
            .or_else(|| value["avatar_url"].as_str())
            .unwrap_or_default()
            .to_owned(),
    }
}

fn pull_state(state: &str, draft: bool, merged: bool) -> String {
    match (state.to_ascii_lowercase().as_str(), merged, draft) {
        (_, true, _) => "merged",
        ("closed", false, _) => "closed",
        (_, false, true) => "draft",
        _ => "open",
    }
    .to_owned()
}

/// The search filters of the project PR list.
#[derive(Clone, Copy, Debug, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PullFilter {
    Open,
    Review,
    Mine,
    Merged,
    Closed,
}

/// GitHub search query for a project's pull requests.
fn search_query(repository: &str, label: Option<&str>, filter: PullFilter, text: &str) -> String {
    let mut parts = vec!["is:pr".to_owned(), format!("repo:{repository}")];
    if let Some(label) = label.filter(|label| !label.is_empty()) {
        parts.push(format!("label:\"{}\"", label.replace('"', "")));
    }
    parts.push(
        match filter {
            PullFilter::Open => "is:open",
            PullFilter::Review => "is:open review-requested:@me",
            PullFilter::Mine => "is:open author:@me",
            PullFilter::Merged => "is:merged",
            PullFilter::Closed => "is:closed is:unmerged",
        }
        .to_owned(),
    );
    let text = text.trim();
    if !text.is_empty() {
        parts.push(text.chars().take(200).collect());
    }
    parts.join(" ")
}

pub async fn list(
    github: &GithubClient,
    token: &str,
    repository: &str,
    label: Option<&str>,
    filter: PullFilter,
    search: &str,
    page: u32,
) -> Result<PullList, GithubError> {
    let mut url = url::Url::parse("https://x/search/issues").expect("static URL");
    url.query_pairs_mut()
        .append_pair("q", &search_query(repository, label, filter, search))
        .append_pair("sort", "updated")
        .append_pair("order", "desc")
        .append_pair("per_page", &LIST_PAGE_SIZE.to_string())
        .append_pair("page", &page.to_string());
    let path = format!("search/issues?{}", url.query().unwrap_or_default());
    let reply: Value = github.rest(token, Method::GET, &path, None).await?;
    let items: Vec<PullSummary> = reply["items"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|item| PullSummary {
            repository: repository.to_owned(),
            number: int(&item["number"]),
            title: text(&item["title"]),
            state: pull_state(
                item["state"].as_str().unwrap_or("open"),
                item["draft"].as_bool().unwrap_or(false),
                !item["pull_request"]["merged_at"].is_null(),
            ),
            author: actor(&item["user"]),
            updated_at: millis(&item["updated_at"]),
            comments: int(&item["comments"]),
        })
        .collect();
    let total = reply["total_count"].as_i64().unwrap_or(0);
    // GitHub search returns at most 1,000 results.
    let seen = i64::from(page * LIST_PAGE_SIZE);
    let more = seen < total.min(1000) && items.len() == LIST_PAGE_SIZE as usize;
    Ok(PullList {
        items,
        next_cursor: more.then(|| (page + 1).to_string()),
    })
}

const DETAIL_QUERY: &str = "query PullDetail($owner: String!, $repo: String!, $number: Int!) {
  viewer { login }
  repository(owner: $owner, name: $repo) {
    mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed viewerDefaultMergeMethod viewerPermission
    pullRequest(number: $number) {
      id number title body state isDraft merged locked url additions deletions changedFiles
      baseRefName headRefName baseRefOid headRefOid mergeable mergeStateStatus
      author { login avatarUrl }
      commits(last: 1) { nodes { commit { statusCheckRollup { state contexts(first: 100) { nodes {
        __typename
        ... on CheckRun { name conclusion status detailsUrl }
        ... on StatusContext { context state targetUrl }
      } } } } } }
    }
  }
}";

fn check_state(value: &str) -> &'static str {
    match value.to_ascii_uppercase().as_str() {
        "SUCCESS" => "success",
        "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => "failure",
        "NEUTRAL" | "SKIPPED" | "STALE" => "neutral",
        _ => "pending",
    }
}

fn checks(rollup: &Value) -> ChecksSummary {
    let runs: Vec<CheckRun> = rollup["contexts"]["nodes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|node| {
            if node["__typename"] == "CheckRun" {
                let state = if node["status"] == "COMPLETED" {
                    check_state(node["conclusion"].as_str().unwrap_or("NEUTRAL"))
                } else {
                    "pending"
                };
                CheckRun {
                    name: text(&node["name"]),
                    state: state.to_owned(),
                    url: node["detailsUrl"].as_str().map(str::to_owned),
                }
            } else {
                CheckRun {
                    name: text(&node["context"]),
                    state: check_state(node["state"].as_str().unwrap_or("PENDING")).to_owned(),
                    url: node["targetUrl"].as_str().map(str::to_owned),
                }
            }
        })
        .collect();
    let state = if rollup.is_null() {
        "none"
    } else if runs.iter().any(|run| run.state == "failure") {
        "failure"
    } else if runs.iter().any(|run| run.state == "pending") {
        "pending"
    } else {
        check_state(rollup["state"].as_str().unwrap_or("SUCCESS"))
    };
    ChecksSummary {
        state: state.to_owned(),
        runs,
    }
}

fn blocked_reason(status: &str, draft: bool) -> Option<String> {
    let reason = match status {
        _ if draft => "This pull request is a draft.",
        "DIRTY" => "This branch has conflicts.",
        "BLOCKED" => "Branch protection blocks merging.",
        "BEHIND" => "This branch is out of date with the base branch.",
        "UNKNOWN" => "GitHub is still checking whether this can be merged.",
        "DRAFT" => "This pull request is a draft.",
        _ => return None,
    };
    Some(reason.to_owned())
}

pub async fn detail(github: &GithubClient, token: &str, pull: &PullRef) -> Result<PullDetail, GithubError> {
    let data: Value = github.graphql(token, DETAIL_QUERY, pull.variables()).await?;
    let repo = &data["repository"];
    let pr = &repo["pullRequest"];
    if pr.is_null() {
        return Err(GithubError::NotFound);
    }
    let draft = pr["isDraft"].as_bool().unwrap_or(false);
    let merged = pr["merged"].as_bool().unwrap_or(false);
    let state = pull_state(pr["state"].as_str().unwrap_or("OPEN"), draft, merged);
    let open = state == "open" || state == "draft";
    let methods: Vec<String> = [
        ("merge", "mergeCommitAllowed"),
        ("squash", "squashMergeAllowed"),
        ("rebase", "rebaseMergeAllowed"),
    ]
    .into_iter()
    .filter(|(_, field)| repo[*field].as_bool().unwrap_or(false))
    .map(|(method, _)| method.to_owned())
    .collect();
    let permission = repo["viewerPermission"].as_str().unwrap_or("READ");
    let can_write = matches!(permission, "WRITE" | "MAINTAIN" | "ADMIN");
    let locked = pr["locked"].as_bool().unwrap_or(false);
    let author = actor(&pr["author"]);
    let is_author = data["viewer"]["login"].as_str() == Some(author.login.as_str());
    let mergeable = match pr["mergeable"].as_str().unwrap_or("UNKNOWN") {
        "MERGEABLE" => "mergeable",
        "CONFLICTING" => "conflicting",
        _ => "unknown",
    };
    Ok(PullDetail {
        repository: pull.repository(),
        number: int(&pr["number"]),
        node_id: text(&pr["id"]),
        title: text(&pr["title"]),
        body: text(&pr["body"]),
        state,
        author,
        base_ref: text(&pr["baseRefName"]),
        head_ref: text(&pr["headRefName"]),
        base_sha: text(&pr["baseRefOid"]),
        head_sha: text(&pr["headRefOid"]),
        additions: int(&pr["additions"]),
        deletions: int(&pr["deletions"]),
        changed_files: int(&pr["changedFiles"]),
        html_url: text(&pr["url"]),
        checks: checks(&pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]),
        merge: MergeInfo {
            methods,
            mergeable: mergeable.to_owned(),
            blocked_reason: if open {
                blocked_reason(pr["mergeStateStatus"].as_str().unwrap_or(""), draft)
            } else {
                None
            },
            default_method: repo["viewerDefaultMergeMethod"]
                .as_str()
                .map(str::to_ascii_lowercase),
        },
        viewer: PullViewer {
            can_comment: !locked || can_write,
            can_review: (!locked || can_write) && !is_author && open,
            can_merge: can_write && open,
            is_author,
        },
        tasks: Vec::new(),
    })
}

const FILES_QUERY: &str = "query PullFiles($owner: String!, $repo: String!, $number: Int!, $after: String) {
  repository(owner: $owner, name: $repo) { pullRequest(number: $number) {
    files(first: 100, after: $after) { nodes { path viewerViewedState } pageInfo { hasNextPage endCursor } }
  } }
}";

pub async fn files(github: &GithubClient, token: &str, pull: &PullRef) -> Result<Vec<PullFile>, GithubError> {
    let mut viewed = std::collections::HashSet::new();
    let mut after = Value::Null;
    for _ in 0..MAX_FILE_PAGES {
        let mut variables = pull.variables();
        variables["after"] = after.clone();
        let data: Value = github.graphql(token, FILES_QUERY, variables).await?;
        let files = &data["repository"]["pullRequest"]["files"];
        for node in files["nodes"].as_array().map(Vec::as_slice).unwrap_or_default() {
            if node["viewerViewedState"] == "VIEWED" {
                viewed.insert(text(&node["path"]));
            }
        }
        if files["pageInfo"]["hasNextPage"] != true {
            break;
        }
        after = files["pageInfo"]["endCursor"].clone();
    }
    let mut result = Vec::new();
    for page in 1..=MAX_FILE_PAGES {
        let reply: Vec<Value> = github
            .rest(token, Method::GET, &pull.rest(&format!("/files?per_page=100&page={page}")), None)
            .await?;
        let count = reply.len();
        result.extend(reply.into_iter().map(|file| {
            let path = text(&file["filename"]);
            PullFile {
                viewed: viewed.contains(&path),
                path,
                previous_path: file["previous_filename"].as_str().map(str::to_owned),
                status: text(&file["status"]),
                additions: int(&file["additions"]),
                deletions: int(&file["deletions"]),
                patch: file["patch"].as_str().map(str::to_owned),
            }
        }));
        if count < 100 {
            break;
        }
    }
    Ok(result)
}

/// A file at a commit, as text. Not a GitHub error when the file is binary or too large: `text` is `null`.
pub async fn contents(
    github: &GithubClient,
    token: &str,
    pull: &PullRef,
    path: &str,
    sha: &str,
) -> Result<FileContents, GithubError> {
    let encoded: Vec<String> = path
        .split('/')
        .map(|segment| url::form_urlencoded::byte_serialize(segment.as_bytes()).collect::<String>().replace('+', "%20"))
        .collect();
    let reply: Value = github
        .rest(
            token,
            Method::GET,
            &format!("repos/{}/{}/contents/{}?ref={sha}", pull.owner, pull.repo, encoded.join("/")),
            None,
        )
        .await?;
    let size = usize::try_from(reply["size"].as_u64().unwrap_or(u64::MAX)).unwrap_or(usize::MAX);
    if reply["type"] != "file" || reply["encoding"] != "base64" || size > MAX_CONTENT_BYTES {
        return Ok(FileContents { text: None });
    }
    let raw: String = text(&reply["content"]).split_whitespace().collect();
    let bytes = STANDARD.decode(raw).unwrap_or_default();
    // A NUL byte marks a binary file, as git does.
    let text = String::from_utf8(bytes).ok().filter(|text| !text.contains('\0'));
    Ok(FileContents { text })
}

const THREADS_QUERY: &str = "query PullThreads($owner: String!, $repo: String!, $number: Int!, $after: String) {
  repository(owner: $owner, name: $repo) { pullRequest(number: $number) {
    reviewThreads(first: 100, after: $after) {
      nodes {
        id path line startLine diffSide startDiffSide isResolved isOutdated viewerCanResolve viewerCanReply
        comments(first: 100) { nodes { id databaseId body createdAt state viewerCanUpdate author { login avatarUrl } } }
      }
      pageInfo { hasNextPage endCursor }
    }
  } }
}";

pub async fn threads(github: &GithubClient, token: &str, pull: &PullRef) -> Result<Vec<ReviewThread>, GithubError> {
    let mut threads = Vec::new();
    let mut after = Value::Null;
    for _ in 0..MAX_FILE_PAGES {
        let mut variables = pull.variables();
        variables["after"] = after.clone();
        let data: Value = github.graphql(token, THREADS_QUERY, variables).await?;
        let page = &data["repository"]["pullRequest"]["reviewThreads"];
        for node in page["nodes"].as_array().map(Vec::as_slice).unwrap_or_default() {
            threads.push(ReviewThread {
                id: text(&node["id"]),
                path: text(&node["path"]),
                side: node["diffSide"].as_str().unwrap_or("RIGHT").to_owned(),
                line: node["line"].as_i64(),
                start_line: node["startLine"].as_i64(),
                start_side: node["startDiffSide"].as_str().map(str::to_owned),
                is_resolved: node["isResolved"].as_bool().unwrap_or(false),
                is_outdated: node["isOutdated"].as_bool().unwrap_or(false),
                can_resolve: node["viewerCanResolve"].as_bool().unwrap_or(false),
                can_reply: node["viewerCanReply"].as_bool().unwrap_or(false),
                comments: node["comments"]["nodes"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .map(review_comment)
                    .collect(),
            });
        }
        if page["pageInfo"]["hasNextPage"] != true {
            break;
        }
        after = page["pageInfo"]["endCursor"].clone();
    }
    Ok(threads)
}

/// One GraphQL `PullRequestReviewComment`.
pub fn review_comment(node: &Value) -> ReviewComment {
    ReviewComment {
        id: text(&node["id"]),
        database_id: int(&node["databaseId"]),
        author: actor(&node["author"]),
        body: text(&node["body"]),
        created_at: millis(&node["createdAt"]),
        pending: node["state"] == "PENDING",
        can_edit: node["viewerCanUpdate"].as_bool().unwrap_or(false),
    }
}

const CONVERSATION_QUERY: &str = "query PullConversation($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) { pullRequest(number: $number) {
    comments(last: 100) { nodes { body createdAt author { login avatarUrl } } }
    reviews(last: 100, states: [APPROVED, CHANGES_REQUESTED, COMMENTED]) { nodes { body state submittedAt author { login avatarUrl } } }
  } }
}";

pub async fn conversation(
    github: &GithubClient,
    token: &str,
    pull: &PullRef,
) -> Result<Vec<PullTimelineItem>, GithubError> {
    let data: Value = github.graphql(token, CONVERSATION_QUERY, pull.variables()).await?;
    let pr = &data["repository"]["pullRequest"];
    let list = |value: &Value| value["nodes"].as_array().cloned().unwrap_or_default();
    let mut items: Vec<PullTimelineItem> = list(&pr["comments"])
        .iter()
        .map(|node| PullTimelineItem {
            kind: "comment".to_owned(),
            author: actor(&node["author"]),
            body: text(&node["body"]),
            created_at: millis(&node["createdAt"]),
            review_state: None,
        })
        .collect();
    items.extend(
        list(&pr["reviews"])
            .iter()
            // A review with inline comments only and no summary adds nothing to the conversation.
            .filter(|node| node["state"] != "COMMENTED" || !text(&node["body"]).trim().is_empty())
            .map(|node| PullTimelineItem {
                kind: "review".to_owned(),
                author: actor(&node["author"]),
                body: text(&node["body"]),
                created_at: millis(&node["submittedAt"]),
                review_state: node["state"].as_str().map(str::to_ascii_lowercase),
            }),
    );
    items.sort_by_key(|item| item.created_at);
    Ok(items)
}

const PULL_ID_QUERY: &str = "query PullId($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) { pullRequest(number: $number) { id } }
}";

/// GitHub's node id of the pull request.
pub async fn node_id(github: &GithubClient, token: &str, pull: &PullRef) -> Result<String, GithubError> {
    let data: Value = github.graphql(token, PULL_ID_QUERY, pull.variables()).await?;
    data["repository"]["pullRequest"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or(GithubError::NotFound)
}

pub async fn set_viewed(
    github: &GithubClient,
    token: &str,
    pull: &PullRef,
    path: &str,
    viewed: bool,
) -> Result<(), GithubError> {
    let id = node_id(github, token, pull).await?;
    let query = if viewed {
        "mutation MarkViewed($pullId: ID!, $path: String!) { markFileAsViewed(input: { pullRequestId: $pullId, path: $path }) { clientMutationId } }"
    } else {
        "mutation UnmarkViewed($pullId: ID!, $path: String!) { unmarkFileAsViewed(input: { pullRequestId: $pullId, path: $path }) { clientMutationId } }"
    };
    let _: Value = github
        .graphql(token, query, json!({ "pullId": id, "path": path }))
        .await?;
    Ok(())
}
