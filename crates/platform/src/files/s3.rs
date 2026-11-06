//! A small client for S3-compatible object storage (AWS S3, Cloudflare R2, MinIO, MaxIO, ...): only the
//! requests that attachment and backup storage need. Requests are signed with AWS Signature V4 and sent with
//! reqwest over rustls with ring, so no AWS SDK (and no aws-lc build) is needed.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use hmac::{Hmac, Mac};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use reqwest::{Method, StatusCode, Url};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::BlobReader;

/// Payload hash for requests whose body is not hashed (allowed by S3 over HTTPS and by S3-compatible servers).
const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";
const PAGE_SIZE: &str = "1000";
/// The most of an error, probe or copy response that is read.
const SMALL_BODY_BYTES: usize = 64 * 1024;
/// The most of an object read with [`S3Bucket::get_bytes`] (backup manifests).
const OBJECT_BODY_BYTES: usize = 16 * 1024 * 1024;
/// The most of one list page (1000 keys of up to 1 KiB each, with their XML).
const LIST_BODY_BYTES: usize = 8 * 1024 * 1024;
/// The whole of a request without a file body, also when the server sends slowly.
const SMALL_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Reading an error body of a file transfer.
const ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(10);

/// A bucket and the key prefix under which Orbit stores its objects.
#[derive(Clone, Eq, PartialEq)]
pub struct S3Config {
    /// `https://s3.eu-central-1.amazonaws.com`, `https://<account>.r2.cloudflarestorage.com`, `http://minio:9000`.
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    /// Key prefix without leading or trailing `/`; empty for the bucket root.
    pub prefix: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    /// `endpoint/bucket/key` URLs instead of `bucket.endpoint/key` (MinIO and most self-hosted servers).
    pub path_style: bool,
}

impl std::fmt::Debug for S3Config {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("S3Config")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .field("access_key_id", &self.access_key_id)
            .field("path_style", &self.path_style)
            .finish_non_exhaustive()
    }
}

impl S3Config {
    /// Whether both configs store their objects under the same keys: the same bucket and prefix on the same
    /// endpoint host. Two names for one server (`localhost` and `127.0.0.1`) are not found.
    #[must_use]
    pub fn same_location(&self, other: &Self) -> bool {
        self.location() == other.location()
    }

    /// Whether the two configs can address the same objects: the same bucket on the same endpoint host, and one
    /// prefix equal to or inside the other.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        fn inside(inner: &str, outer: &str) -> bool {
            outer.is_empty()
                || inner
                    .strip_prefix(outer)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        }
        let ((server, bucket, left), (other_server, other_bucket, right)) =
            (self.location(), other.location());
        server == other_server
            && bucket == other_bucket
            && (inside(left, right) || inside(right, left))
    }

    /// The endpoint's `host[:port]` (lowercase, without a default port), the bucket and the prefix.
    fn location(&self) -> (String, &str, &str) {
        let endpoint = self.endpoint.trim();
        let server = Url::parse(endpoint)
            .ok()
            .and_then(|url| {
                let host = url.host_str()?.to_owned();
                Some(match url.port() {
                    Some(port) => format!("{host}:{port}"),
                    None => host,
                })
            })
            .unwrap_or_else(|| endpoint.to_ascii_lowercase());
        (server, self.bucket.trim(), self.prefix.trim_matches('/'))
    }
}

#[derive(Debug, Error)]
pub enum S3Error {
    #[error("invalid object storage settings: {0}")]
    Config(String),
    #[error("object storage request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("object storage answered {status}: {code} {message}")]
    Status {
        status: u16,
        code: String,
        message: String,
    },
    #[error("object storage file I/O failed at {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("object storage returned an invalid response: {0}")]
    Response(String),
}

impl S3Error {
    /// A short cause for an API response: no endpoint or server text beyond the S3 error code.
    /// Log the full error.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            Self::Config(detail) => detail.clone(),
            Self::Request(error) if error.is_timeout() => {
                "the storage endpoint did not answer in time".to_owned()
            }
            Self::Request(error) if error.is_connect() => {
                "the connection to the storage endpoint failed".to_owned()
            }
            Self::Request(_) => "the request to the storage endpoint failed".to_owned(),
            Self::Status { status, code, .. } => {
                let code = Some(code.as_str())
                    .filter(|code| {
                        !code.is_empty()
                            && code.len() <= 64
                            && code.chars().all(|c| c.is_ascii_alphanumeric())
                    })
                    .map(|code| format!(" {code}"))
                    .unwrap_or_default();
                format!("the S3 request was refused ({status}{code})")
            }
            Self::Io { .. } => "a local file could not be read or written".to_owned(),
            Self::Response(_) => "object storage returned an invalid response".to_owned(),
        }
    }
}

/// An object found by [`S3Bucket::list`]. `key` is relative to the configured prefix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S3Object {
    pub key: String,
    pub last_modified_millis: i64,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct S3Bucket {
    config: S3Config,
    scheme: String,
    /// `host[:port]` of the endpoint.
    authority: String,
    client: reqwest::Client,
}

struct Target {
    url: String,
    host: String,
    path: String,
}

impl S3Bucket {
    pub fn new(config: S3Config) -> Result<Self, S3Error> {
        Self::build(config, false)
    }

    /// A bucket that a workspace member picked: only `https` on a public internet address, so the server cannot
    /// be made to reach its own network (SSRF).
    pub fn new_public(config: S3Config) -> Result<Self, S3Error> {
        Self::build(config, true)
    }

    fn build(mut config: S3Config, public_only: bool) -> Result<Self, S3Error> {
        config.prefix = config.prefix.trim_matches('/').to_owned();
        if config.region.trim().is_empty() {
            config.region = "us-east-1".to_owned();
        }
        let endpoint = Url::parse(config.endpoint.trim())
            .map_err(|_| S3Error::Config("endpoint must be a URL".to_owned()))?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.query().is_some()
            || !matches!(endpoint.path(), "" | "/")
            || !endpoint.username().is_empty()
        {
            return Err(S3Error::Config(
                "endpoint must be an http(s) URL without a path".to_owned(),
            ));
        }
        let host = endpoint
            .host_str()
            .ok_or_else(|| S3Error::Config("endpoint has no host".to_owned()))?;
        if public_only {
            let public = match endpoint.host() {
                Some(url::Host::Ipv4(ip)) => crate::is_public_ip(ip.into()),
                Some(url::Host::Ipv6(ip)) => crate::is_public_ip(ip.into()),
                _ => true,
            };
            if endpoint.scheme() != "https" || !public {
                return Err(S3Error::Config(
                    "endpoint must be an https URL on the public internet".to_owned(),
                ));
            }
        }
        let authority = match endpoint.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        if config.bucket.is_empty()
            || !config
                .bucket
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.'))
        {
            return Err(S3Error::Config(
                "bucket must use lowercase letters, digits, '-' and '.'".to_owned(),
            ));
        }
        let mut client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none());
        if public_only {
            // A proxy from the environment would resolve the host itself, past the public-address filter.
            client = client
                .dns_resolver(std::sync::Arc::new(crate::PublicResolver))
                .no_proxy();
        }
        let client = client.build()?;
        Ok(Self {
            scheme: endpoint.scheme().to_owned(),
            authority,
            config,
            client,
        })
    }

    #[must_use]
    pub fn config(&self) -> &S3Config {
        &self.config
    }

    /// Writes, reads back and deletes a small object: proves the endpoint, credentials and permissions work.
    pub async fn probe(&self) -> Result<(), S3Error> {
        let key = format!(".orbit-probe/{}", Uuid::now_v7());
        let body = b"orbit storage check".to_vec();
        self.put_bytes(&key, body.clone()).await?;
        let read = self.get_capped(&key, SMALL_BODY_BYTES).await?;
        self.delete(&key).await?;
        if read.as_deref() != Some(body.as_slice()) {
            return Err(S3Error::Response(
                "the test object read back differently".to_owned(),
            ));
        }
        Ok(())
    }

    pub async fn put_bytes(&self, key: &str, body: Vec<u8>) -> Result<(), S3Error> {
        let length = body.len();
        let response = self
            .request(Method::PUT, key, &[], &[])
            .header(CONTENT_LENGTH, length)
            .header(CONTENT_TYPE, "application/octet-stream")
            .timeout(SMALL_REQUEST_TIMEOUT)
            .body(body)
            .send()
            .await?;
        expect_success(response).await.map(drop)
    }

    /// Streams a local file into one object (a single PUT: S3 accepts up to 5 GiB).
    pub async fn put_file(&self, key: &str, path: &Path) -> Result<(), S3Error> {
        let io = |source| S3Error::Io {
            path: path.to_owned(),
            source,
        };
        let file = tokio::fs::File::open(path).await.map_err(io)?;
        let length = file.metadata().await.map_err(io)?.len();
        let response = self
            .request(Method::PUT, key, &[], &[])
            .header(CONTENT_LENGTH, length)
            .header(CONTENT_TYPE, "application/octet-stream")
            .body(reqwest::Body::wrap_stream(
                tokio_util::io::ReaderStream::new(file),
            ))
            .send()
            .await?;
        expect_success(response).await.map(drop)
    }

    /// The object as a stream, or `None` when it does not exist.
    pub async fn get(&self, key: &str) -> Result<Option<BlobReader>, S3Error> {
        let response = self.request(Method::GET, key, &[], &[]).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = expect_success(response).await?;
        let stream = response.bytes_stream().map_err(std::io::Error::other);
        Ok(Some(
            Box::pin(tokio_util::io::StreamReader::new(stream)) as BlobReader
        ))
    }

    /// A small object (at most 16 MiB) read into memory, or `None` when it does not exist.
    pub async fn get_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, S3Error> {
        self.get_capped(key, OBJECT_BODY_BYTES).await
    }

    async fn get_capped(&self, key: &str, limit: usize) -> Result<Option<Vec<u8>>, S3Error> {
        let response = self
            .request(Method::GET, key, &[], &[])
            .timeout(SMALL_REQUEST_TIMEOUT)
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = expect_success(response).await?;
        Ok(Some(read_capped(response, limit).await?.ok_or_else(
            || S3Error::Response(format!("object {key} is larger than {limit} bytes")),
        )?))
    }

    /// Downloads the object into `path` (created or truncated). Returns `false` when the object does not exist.
    pub async fn get_to_file(&self, key: &str, path: &Path) -> Result<bool, S3Error> {
        let response = self.request(Method::GET, key, &[], &[]).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        let mut response = expect_success(response).await?;
        let io = |source| S3Error::Io {
            path: path.to_owned(),
            source,
        };
        let mut file = tokio::fs::File::create(path).await.map_err(io)?;
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await.map_err(io)?;
        }
        file.sync_all().await.map_err(io)?;
        Ok(true)
    }

    /// The object's last-modified time in Unix milliseconds, or `None` when it does not exist.
    pub async fn last_modified(&self, key: &str) -> Result<Option<i64>, S3Error> {
        let response = self
            .request(Method::HEAD, key, &[], &[])
            .timeout(SMALL_REQUEST_TIMEOUT)
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = expect_success(response).await?;
        let value = response
            .headers()
            .get(reqwest::header::LAST_MODIFIED)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| S3Error::Response("HEAD without Last-Modified".to_owned()))?;
        DateTime::parse_from_rfc2822(value)
            .map(|time| Some(time.timestamp_millis()))
            .map_err(|_| S3Error::Response(format!("invalid Last-Modified {value}")))
    }

    /// Copies the object onto itself, which sets its last-modified time to now. Returns `false` when it does
    /// not exist.
    pub async fn touch(&self, key: &str) -> Result<bool, S3Error> {
        let source = format!(
            "/{}/{}",
            self.config.bucket,
            uri_encode(&self.full_key(key), false)
        );
        let response = self
            .request(
                Method::PUT,
                key,
                &[],
                &[
                    ("x-amz-copy-source", source),
                    ("x-amz-metadata-directive", "REPLACE".to_owned()),
                ],
            )
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, 0)
            .timeout(SMALL_REQUEST_TIMEOUT)
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        let body = read_text(expect_success(response).await?, SMALL_BODY_BYTES).await?;
        // S3 can report a failed copy with 200 and an <Error> body.
        if body.contains("<Error>") {
            return match xml_value(&body, "Code").as_deref() {
                Some("NoSuchKey") => Ok(false),
                code => Err(S3Error::Status {
                    status: 200,
                    code: code.unwrap_or_default().to_owned(),
                    message: xml_value(&body, "Message").unwrap_or_default(),
                }),
            };
        }
        Ok(true)
    }

    /// Deletes the object. A missing object is not an error.
    pub async fn delete(&self, key: &str) -> Result<(), S3Error> {
        let response = self
            .request(Method::DELETE, key, &[], &[])
            .timeout(SMALL_REQUEST_TIMEOUT)
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        expect_success(response).await.map(drop)
    }

    /// Every object whose key starts with `prefix`, sorted by key.
    pub async fn list(&self, prefix: &str) -> Result<Vec<S3Object>, S3Error> {
        self.list_up_to(prefix, usize::MAX).await
    }

    /// Whether at least one object's key starts with `prefix`.
    pub async fn any(&self, prefix: &str) -> Result<bool, S3Error> {
        Ok(!self.list_up_to(prefix, 1).await?.is_empty())
    }

    async fn list_up_to(&self, prefix: &str, limit: usize) -> Result<Vec<S3Object>, S3Error> {
        let full_prefix = self.full_key(prefix);
        let strip = self.full_key("");
        let mut objects = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut query = vec![
                ("list-type", "2".to_owned()),
                ("max-keys", PAGE_SIZE.to_owned()),
                ("prefix", full_prefix.clone()),
            ];
            if let Some(token) = &token {
                query.push(("continuation-token", token.clone()));
            }
            let response = self
                .request(Method::GET, "", &query, &[])
                .timeout(SMALL_REQUEST_TIMEOUT)
                .send()
                .await?;
            let body = read_text(expect_success(response).await?, LIST_BODY_BYTES).await?;
            for contents in xml_elements(&body, "Contents") {
                let key = xml_value(contents, "Key")
                    .ok_or_else(|| S3Error::Response("listed object without a key".to_owned()))?;
                let last_modified = xml_value(contents, "LastModified")
                    .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
                    .ok_or_else(|| S3Error::Response(format!("invalid LastModified for {key}")))?;
                objects.push(S3Object {
                    key: key.strip_prefix(&strip).unwrap_or(&key).to_owned(),
                    last_modified_millis: last_modified.timestamp_millis(),
                    size: xml_value(contents, "Size")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                });
                if objects.len() >= limit {
                    return Ok(objects);
                }
            }
            token = match xml_value(&body, "IsTruncated").as_deref() {
                Some("true") => {
                    Some(xml_value(&body, "NextContinuationToken").ok_or_else(|| {
                        S3Error::Response("truncated list without a continuation token".to_owned())
                    })?)
                }
                _ => None,
            };
            if token.is_none() {
                objects.sort_by(|left, right| left.key.cmp(&right.key));
                return Ok(objects);
            }
        }
    }

    fn full_key(&self, key: &str) -> String {
        if self.config.prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{}/{key}", self.config.prefix)
        }
    }

    /// The URL of `key` (an empty key addresses the bucket itself).
    fn target(&self, key: &str) -> Target {
        let object = if key.is_empty() {
            String::new()
        } else {
            uri_encode(&self.full_key(key), true)
        };
        let (host, path) = if self.config.path_style {
            let path = if object.is_empty() {
                format!("/{}", self.config.bucket)
            } else {
                format!("/{}/{object}", self.config.bucket)
            };
            (self.authority.clone(), path)
        } else {
            (
                format!("{}.{}", self.config.bucket, self.authority),
                format!("/{object}"),
            )
        };
        Target {
            url: format!("{}://{host}{path}", self.scheme),
            host,
            path,
        }
    }

    fn request(
        &self,
        method: Method,
        key: &str,
        query: &[(&str, String)],
        headers: &[(&str, String)],
    ) -> reqwest::RequestBuilder {
        let target = self.target(key);
        let mut query: Vec<(String, String)> = query
            .iter()
            .map(|(name, value)| (uri_encode(name, false), uri_encode(value, false)))
            .collect();
        query.sort();
        let query = query
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("&");
        let url = if query.is_empty() {
            target.url
        } else {
            format!("{}?{query}", target.url)
        };
        let mut signed: Vec<(String, String)> = headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect();
        signed.push(("host".to_owned(), target.host));
        let authorization = sign(
            &SigningKey {
                access_key_id: &self.config.access_key_id,
                secret_access_key: &self.config.secret_access_key,
                region: &self.config.region,
            },
            method.as_str(),
            &target.path,
            &query,
            &mut signed,
            UNSIGNED_PAYLOAD,
            Utc::now(),
        );
        let mut builder = self.client.request(method, url);
        for (name, value) in signed {
            if name != "host" {
                builder = builder.header(name, value);
            }
        }
        builder.header(reqwest::header::AUTHORIZATION, authorization)
    }
}

async fn expect_success(response: reqwest::Response) -> Result<reqwest::Response, S3Error> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status().as_u16();
    // The start of the body is enough for its code and message; a file transfer has no request
    // timeout, so the read has its own.
    let body = tokio::time::timeout(ERROR_BODY_TIMEOUT, read_capped(response, SMALL_BODY_BYTES))
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
        .map(|body| String::from_utf8_lossy(&body).into_owned())
        .unwrap_or_default();
    Err(S3Error::Status {
        status,
        code: xml_value(&body, "Code").unwrap_or_default(),
        message: xml_value(&body, "Message").unwrap_or_default(),
    })
}

/// The body, or `None` when it is longer than `limit` (the rest is not read).
async fn read_capped(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Option<Vec<u8>>, S3Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Ok(None);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > limit {
            return Ok(None);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Some(body))
}

async fn read_text(response: reqwest::Response, limit: usize) -> Result<String, S3Error> {
    let body = read_capped(response, limit)
        .await?
        .ok_or_else(|| S3Error::Response(format!("response is larger than {limit} bytes")))?;
    String::from_utf8(body).map_err(|_| S3Error::Response("response is not UTF-8".to_owned()))
}

struct SigningKey<'a> {
    access_key_id: &'a str,
    secret_access_key: &'a str,
    region: &'a str,
}

/// AWS Signature V4 for S3. Adds `x-amz-date` and `x-amz-content-sha256` to `headers` (which must contain
/// `host`) and returns the `Authorization` value. `query` is already encoded and sorted.
fn sign(
    key: &SigningKey<'_>,
    method: &str,
    path: &str,
    query: &str,
    headers: &mut Vec<(String, String)>,
    payload_hash: &str,
    now: DateTime<Utc>,
) -> String {
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date = now.format("%Y%m%d").to_string();
    headers.push(("x-amz-date".to_owned(), amz_date.clone()));
    headers.push(("x-amz-content-sha256".to_owned(), payload_hash.to_owned()));
    for header in headers.iter_mut() {
        header.0 = header.0.to_ascii_lowercase();
    }
    headers.sort();
    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{}\n", value.trim()))
        .collect();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_request =
        format!("{method}\n{path}\n{query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}");
    let scope = format!("{date}/{}/s3/aws4_request", key.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{:x}",
        Sha256::digest(canonical_request.as_bytes())
    );
    let mut signing_key = hmac(
        format!("AWS4{}", key.secret_access_key).as_bytes(),
        date.as_bytes(),
    );
    for part in [key.region, "s3", "aws4_request"] {
        signing_key = hmac(&signing_key, part.as_bytes());
    }
    let signature: String = hmac(&signing_key, string_to_sign.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        key.access_key_id
    )
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// S3's URI encoding: every byte except `A-Z a-z 0-9 - _ . ~` (and `/` in paths) as `%XX`.
fn uri_encode(value: &str, keep_slash: bool) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (keep_slash && byte == b'/')
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// The inner text of every `<name>…</name>` element (S3 responses have no attributes on these elements).
fn xml_elements<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let mut elements = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(&open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(&close) else {
            break;
        };
        elements.push(&after[..end]);
        rest = &after[end + close.len()..];
    }
    elements
}

fn xml_value(xml: &str, name: &str) -> Option<String> {
    xml_elements(xml, name).first().map(|text| {
        text.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    /// The "GET Object" example of AWS's "Signature Calculations for the Authorization Header" page.
    #[test]
    fn signs_the_aws_documentation_example() {
        let mut headers = vec![
            (
                "host".to_owned(),
                "examplebucket.s3.amazonaws.com".to_owned(),
            ),
            ("range".to_owned(), "bytes=0-9".to_owned()),
        ];
        let authorization = sign(
            &SigningKey {
                access_key_id: "AKIAIOSFODNN7EXAMPLE",
                secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                region: "us-east-1",
            },
            "GET",
            "/test.txt",
            "",
            &mut headers,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap(),
        );
        assert_eq!(
            authorization,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[tokio::test]
    async fn small_reads_stop_at_their_byte_cap() {
        let bucket = S3Bucket::new(crate::start_fake_s3().await).unwrap();
        bucket.probe().await.unwrap();
        bucket
            .put_bytes("large", vec![7; SMALL_BODY_BYTES + 1])
            .await
            .unwrap();
        assert!(matches!(
            bucket.get_capped("large", SMALL_BODY_BYTES).await,
            Err(S3Error::Response(_))
        ));
        assert_eq!(
            bucket
                .get_bytes("large")
                .await
                .unwrap()
                .map(|body| body.len()),
            Some(SMALL_BODY_BYTES + 1)
        );
    }

    #[test]
    fn summaries_keep_only_the_status_and_a_plain_code() {
        let refused = S3Error::Status {
            status: 403,
            code: "AccessDenied".to_owned(),
            message: "secret detail from the server".to_owned(),
        };
        assert_eq!(
            refused.summary(),
            "the S3 request was refused (403 AccessDenied)"
        );
        let odd = S3Error::Status {
            status: 500,
            code: "<script>".to_owned(),
            message: String::new(),
        };
        assert_eq!(odd.summary(), "the S3 request was refused (500)");
    }

    #[test]
    fn builds_path_style_and_virtual_host_urls_under_the_prefix() {
        let config = S3Config {
            endpoint: "http://127.0.0.1:9000".to_owned(),
            region: String::new(),
            bucket: "orbit".to_owned(),
            prefix: "/prod/".to_owned(),
            access_key_id: "key".to_owned(),
            secret_access_key: "secret".to_owned(),
            path_style: true,
        };
        let bucket = S3Bucket::new(config.clone()).unwrap();
        assert_eq!(bucket.config().region, "us-east-1");
        let target = bucket.target("blobs/a b");
        assert_eq!(target.url, "http://127.0.0.1:9000/orbit/prod/blobs/a%20b");
        assert_eq!(target.host, "127.0.0.1:9000");

        let bucket = S3Bucket::new(S3Config {
            endpoint: "https://s3.eu-central-1.amazonaws.com".to_owned(),
            path_style: false,
            ..config
        })
        .unwrap();
        let target = bucket.target("");
        assert_eq!(target.url, "https://orbit.s3.eu-central-1.amazonaws.com/");
    }

    #[test]
    fn configs_overlap_on_one_bucket_with_nested_prefixes() {
        let config = |endpoint: &str, bucket: &str, prefix: &str| S3Config {
            endpoint: endpoint.to_owned(),
            region: String::new(),
            bucket: bucket.to_owned(),
            prefix: prefix.to_owned(),
            access_key_id: "key".to_owned(),
            secret_access_key: "secret".to_owned(),
            path_style: true,
        };
        let root = config("https://s3.example.com", "orbit", "");
        for (other, overlaps) in [
            (config("https://S3.example.com:443/", "orbit", ""), true),
            (config("http://s3.example.com", "orbit", "/"), true),
            (config("https://s3.example.com", "orbit", "x"), true),
            (config("https://s3.example.com", "other", ""), false),
            (config("https://s3.example.com:9000", "orbit", ""), false),
            (config("https://r2.example.com", "orbit", ""), false),
        ] {
            assert_eq!(root.overlaps(&other), overlaps, "{other:?}");
            assert_eq!(other.overlaps(&root), overlaps, "{other:?}");
        }
        let nested = config("https://s3.example.com", "orbit", "x/y/");
        assert!(nested.overlaps(&config("https://s3.example.com", "orbit", "x")));
        assert!(nested.overlaps(&config("https://s3.example.com", "orbit", "x/y")));
        assert!(!nested.overlaps(&config("https://s3.example.com", "orbit", "x/yz")));
        assert!(!nested.overlaps(&config("https://s3.example.com", "orbit", "z")));
        assert!(nested.same_location(&config("http://S3.example.com", "orbit", "/x/y")));
        assert!(!nested.same_location(&config("https://s3.example.com", "orbit", "x")));
    }

    #[test]
    fn rejects_endpoints_with_a_path_and_invalid_buckets() {
        let config = S3Config {
            endpoint: "https://s3.example.com/bucket".to_owned(),
            region: "us-east-1".to_owned(),
            bucket: "orbit".to_owned(),
            prefix: String::new(),
            access_key_id: "key".to_owned(),
            secret_access_key: "secret".to_owned(),
            path_style: false,
        };
        assert!(S3Bucket::new(config.clone()).is_err());
        assert!(
            S3Bucket::new(S3Config {
                endpoint: "https://s3.example.com".to_owned(),
                bucket: "Orbit Files".to_owned(),
                ..config
            })
            .is_err()
        );
    }

    #[test]
    fn reads_list_results_and_unescapes_keys() {
        let xml = "<ListBucketResult><IsTruncated>false</IsTruncated>\
            <Contents><Key>a&amp;b</Key><LastModified>2026-10-08T12:00:00.000Z</LastModified><Size>3</Size></Contents>\
            <Contents><Key>c</Key></Contents></ListBucketResult>";
        let contents = xml_elements(xml, "Contents");
        assert_eq!(contents.len(), 2);
        assert_eq!(xml_value(contents[0], "Key").as_deref(), Some("a&b"));
        assert_eq!(xml_value(xml, "IsTruncated").as_deref(), Some("false"));
    }
}
