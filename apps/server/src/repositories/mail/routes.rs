//! Send routes (spec §6.3): the kinds of way Orbit sends mail out.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// How a route sends: from Orbit itself, through an SMTP relay or through a provider's API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SendRouteKind {
    Direct,
    Smtp,
    Postshiba,
    Postmark,
    Resend,
    Mailgun,
    Ses,
    Cloudflare,
}

impl SendRouteKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Smtp => "smtp",
            Self::Postshiba => "postshiba",
            Self::Postmark => "postmark",
            Self::Resend => "resend",
            Self::Mailgun => "mailgun",
            Self::Ses => "ses",
            Self::Cloudflare => "cloudflare",
        }
    }
}

impl FromStr for SendRouteKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "direct" => Self::Direct,
            "smtp" => Self::Smtp,
            "postshiba" => Self::Postshiba,
            "postmark" => Self::Postmark,
            "resend" => Self::Resend,
            "mailgun" => Self::Mailgun,
            "ses" => Self::Ses,
            "cloudflare" => Self::Cloudflare,
            _ => return Err(()),
        })
    }
}
