//! Outgoing email for the core account flows only: invitations, registration links and password recovery. It goes
//! through the SMTP server the root user saves in Admin; without one, Orbit sends nothing. Messages are plain text.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use orbit_platform::Database;
use thiserror::Error;

use crate::repositories::instance_settings::{
    InstanceSettingsRepository, SmtpSecurity, SmtpSettings,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutgoingEmail {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// Messages a recording mailer kept instead of sending them.
pub type Outbox = Arc<Mutex<Vec<OutgoingEmail>>>;

#[derive(Debug, Error)]
pub enum MailError {
    #[error("no mail server is configured")]
    NotConfigured,
    #[error("the saved SMTP password cannot be decrypted with this server's app key")]
    Key,
    #[error("invalid email address: {0}")]
    Address(String),
    #[error("the mail server refused the message: {0}")]
    Smtp(String),
    #[error("the mail settings could not be read")]
    Unavailable(#[from] sqlx::Error),
}

#[derive(Clone)]
pub struct Mailer {
    settings: InstanceSettingsRepository,
    app_key: Option<[u8; 32]>,
    public_origin: String,
    outbox: Option<Outbox>,
}

/// Whether `value` is one email address (no display name).
#[must_use]
pub fn valid_address(value: &str) -> bool {
    value.len() <= 320 && value.parse::<Address>().is_ok()
}

impl Mailer {
    #[must_use]
    pub fn new(database: Database, app_key: Option<[u8; 32]>, public_origin: &str) -> Self {
        Self {
            settings: InstanceSettingsRepository::new(database),
            app_key,
            public_origin: public_origin.trim_end_matches('/').to_owned(),
            outbox: None,
        }
    }

    /// For tests: keeps messages in the returned outbox instead of sending them, and counts as configured.
    #[must_use]
    pub fn recording(database: Database, public_origin: &str) -> (Self, Outbox) {
        let outbox = Outbox::default();
        let mut mailer = Self::new(database, Some([7; 32]), public_origin);
        mailer.outbox = Some(Arc::clone(&outbox));
        (mailer, outbox)
    }

    /// The key secrets are encrypted with, also for saving the SMTP password.
    #[must_use]
    pub fn app_key(&self) -> Option<&[u8; 32]> {
        self.app_key.as_ref()
    }

    /// An absolute link into the web app, for example `/recovery?token=…`.
    #[must_use]
    pub fn link(&self, path: &str) -> String {
        format!("{}{path}", self.public_origin)
    }

    /// Whether a mail server is saved, so the email flows can be offered.
    pub async fn enabled(&self) -> Result<bool, sqlx::Error> {
        Ok(self.outbox.is_some() || self.settings.get().await?.smtp.is_some())
    }

    pub async fn send(&self, email: OutgoingEmail) -> Result<(), MailError> {
        if let Some(outbox) = &self.outbox {
            outbox.lock().expect("outbox lock").push(email);
            return Ok(());
        }
        let smtp = self
            .settings
            .get()
            .await?
            .smtp
            .ok_or(MailError::NotConfigured)?;
        let message = message(&smtp, &email)?;
        transport(&smtp, self.app_key.as_ref())?
            .send(message)
            .await
            .map(|_| ())
            .map_err(|error| MailError::Smtp(error.to_string()))
    }

    /// Sends in the background and logs a failure. For flows whose answer must not depend on the email (recovery,
    /// registration), so the response neither waits for the mail server nor tells whether an account exists.
    pub fn send_later(&self, email: OutgoingEmail) {
        let mailer = self.clone();
        tokio::spawn(async move {
            if let Err(error) = mailer.send(email).await {
                tracing::warn!(%error, "email could not be sent");
            }
        });
    }
}

fn message(smtp: &SmtpSettings, email: &OutgoingEmail) -> Result<Message, MailError> {
    let address = |value: &str| {
        value
            .parse::<Address>()
            .map_err(|_| MailError::Address(value.to_owned()))
    };
    let from = Mailbox::new(
        Some(smtp.from_name.clone().unwrap_or_else(|| "Orbit".to_owned())),
        address(&smtp.from_address)?,
    );
    Message::builder()
        .from(from)
        .to(Mailbox::new(None, address(&email.to)?))
        .subject(&email.subject)
        .header(ContentType::TEXT_PLAIN)
        .body(email.body.clone())
        .map_err(|error| MailError::Smtp(error.to_string()))
}

fn transport(
    smtp: &SmtpSettings,
    app_key: Option<&[u8; 32]>,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, MailError> {
    let builder = match smtp.security {
        SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host),
        SmtpSecurity::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host),
        SmtpSecurity::None => Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
            &smtp.host,
        )),
    }
    .map_err(|error| MailError::Smtp(error.to_string()))?;
    let mut builder = builder
        .port(smtp.port)
        .timeout(Some(Duration::from_secs(20)));
    if let Some(username) = &smtp.username {
        let password = match &smtp.password {
            Some(sealed) => {
                crate::secret_box::decrypt_secret(app_key.ok_or(MailError::Key)?, sealed)
                    .map_err(|()| MailError::Key)?
            }
            None => String::new(),
        };
        builder = builder.credentials(Credentials::new(username.clone(), password));
    }
    Ok(builder.build())
}

/// The text of each email Orbit sends.
pub mod templates {
    use super::OutgoingEmail;

    /// The subject is fixed and the body names only the workspace, cleaned up: the names come from
    /// users, and a name must not make the email say something Orbit does not.
    #[must_use]
    pub fn invitation(to: &str, workspace: &str, url: &str) -> OutgoingEmail {
        OutgoingEmail {
            to: to.to_owned(),
            subject: "You have been invited to Orbit".to_owned(),
            body: format!(
                "You have been invited to join the workspace {} on Orbit.\n\n\
                 Accept the invitation:\n{url}\n\n\
                 The link expires in 7 days. If you did not expect this invitation, ignore this email.\n",
                quoted_name(workspace)
            ),
        }
    }

    /// `name` on one line in quotes: without control, invisible or direction characters (which
    /// could add lines or reorder text), and at most 60 characters long.
    fn quoted_name(name: &str) -> String {
        const MAX_CHARS: usize = 60;
        let visible: String = name
            .chars()
            .filter(|character| {
                !matches!(character,
                    '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
                    | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
            })
            .map(|character| match character {
                '"' => '\'',
                _ if character.is_control() => ' ',
                _ => character,
            })
            .collect();
        let words = visible.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut short: String = words.chars().take(MAX_CHARS).collect();
        if words.chars().count() > MAX_CHARS {
            short.push('…');
        }
        format!("\"{short}\"")
    }

    #[must_use]
    pub fn registration(to: &str, url: &str) -> OutgoingEmail {
        OutgoingEmail {
            to: to.to_owned(),
            subject: "Finish creating your Orbit account".to_owned(),
            body: format!(
                "Open this link to finish creating your Orbit account:\n{url}\n\n\
                 The link works once and expires in 60 minutes. If you did not ask for an account, ignore this email.\n"
            ),
        }
    }

    #[must_use]
    pub fn recovery(to: &str, url: &str) -> OutgoingEmail {
        OutgoingEmail {
            to: to.to_owned(),
            subject: "Reset your Orbit password".to_owned(),
            body: format!(
                "Open this link to set a new Orbit password:\n{url}\n\n\
                 The link works once and expires in 30 minutes. If you did not ask for this, ignore this email; \
                 your password stays the same.\n"
            ),
        }
    }

    /// A recovery link an Orbit Cloud administrator sent from Admin → Users.
    #[must_use]
    pub fn admin_recovery(to: &str, url: &str) -> OutgoingEmail {
        OutgoingEmail {
            to: to.to_owned(),
            subject: "Reset your Orbit password".to_owned(),
            body: format!(
                "An Orbit administrator started a password reset for your account. Open this link to set a new \
                 password:\n{url}\n\n\
                 The link works once and expires in 30 minutes. If you did not ask for this, contact support; \
                 your password stays the same until the link is used.\n"
            ),
        }
    }

    /// Sent to the new address when an Orbit Cloud administrator changes an account's email.
    #[must_use]
    pub fn email_set_by_admin(to: &str) -> OutgoingEmail {
        OutgoingEmail {
            to: to.to_owned(),
            subject: "Your Orbit email was changed".to_owned(),
            body: "An Orbit administrator set this address as the email of your account.\n\n\
                   If you did not ask for this, contact support.\n"
                .to_owned(),
        }
    }

    /// Sent when an Orbit Cloud administrator turns off an account's two-step sign-in.
    #[must_use]
    pub fn two_factor_reset(to: &str) -> OutgoingEmail {
        OutgoingEmail {
            to: to.to_owned(),
            subject: "Two-step sign-in was turned off".to_owned(),
            body: "An Orbit administrator turned off two-step sign-in for your account. If you did not ask for \
                   this, contact support.\n"
                .to_owned(),
        }
    }

    /// Sent to the new address of an email change.
    #[must_use]
    pub fn email_change(to: &str, url: &str) -> OutgoingEmail {
        OutgoingEmail {
            to: to.to_owned(),
            subject: "Confirm your new Orbit email".to_owned(),
            body: format!(
                "Open this link to make this address the email of your Orbit account:\n{url}\n\n\
                 The link works once and expires in 60 minutes. If you did not ask for this, ignore this email.\n"
            ),
        }
    }

    /// Sent to the old address once an email change is confirmed.
    #[must_use]
    pub fn email_changed(to: &str, new_email: &str) -> OutgoingEmail {
        OutgoingEmail {
            to: to.to_owned(),
            subject: "Your Orbit email was changed".to_owned(),
            body: format!(
                "The email of your Orbit account was changed to {new_email}.\n\nIf this was not you, contact support right away.\n"
            ),
        }
    }

    #[must_use]
    pub fn test(to: &str) -> OutgoingEmail {
        OutgoingEmail {
            to: to.to_owned(),
            subject: "Orbit test email".to_owned(),
            body: "Your Orbit mail settings work. Orbit can now send invitations, registration links and \
                   password reset links.\n"
                .to_owned(),
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn an_invitation_names_the_workspace_on_one_quoted_line() {
            let email = super::invitation(
                "guest@example.com",
                "Acme\n\nYour account is locked: \"visit\" evil.example\u{202E}",
                "https://orbit.test/accept-invitation?token=t",
            );
            assert_eq!(email.subject, "You have been invited to Orbit");
            assert!(email.body.starts_with(
                "You have been invited to join the workspace \"Acme Your account is locked: 'visit' evil.example\" on Orbit.\n\n"
            ));
            let long = super::invitation("g@example.com", &"x".repeat(100), "u");
            assert!(long.body.contains(&format!("\"{}…\"", "x".repeat(60))));
        }
    }
}
