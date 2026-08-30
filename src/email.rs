use lettre::message::{Mailbox, Message, header::ContentType};
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncTransport, Tokio1Executor};
use std::time::Duration;

#[derive(Clone)]
pub struct EmailService {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    from_name: Option<String>,
}

impl EmailService {
    pub fn from_env() -> Result<Option<Self>, String> {
        let host = env_value("MONOIZE_SMTP_HOST");
        if host.is_none() {
            return Ok(None);
        }
        let host = host.expect("host checked above");
        let port = env_value("MONOIZE_SMTP_PORT")
            .ok_or_else(|| "MONOIZE_SMTP_PORT is required when SMTP is enabled".to_string())?
            .parse::<u16>()
            .map_err(|_| "MONOIZE_SMTP_PORT must be an integer from 1 to 65535".to_string())?;
        if port == 0 {
            return Err("MONOIZE_SMTP_PORT must be an integer from 1 to 65535".to_string());
        }
        let username = env_value("MONOIZE_SMTP_USERNAME")
            .ok_or_else(|| "MONOIZE_SMTP_USERNAME is required when SMTP is enabled".to_string())?;
        let password = env_value("MONOIZE_SMTP_PASSWORD")
            .ok_or_else(|| "MONOIZE_SMTP_PASSWORD is required when SMTP is enabled".to_string())?;
        let from_raw = env_value("MONOIZE_SMTP_FROM")
            .ok_or_else(|| "MONOIZE_SMTP_FROM is required when SMTP is enabled".to_string())?;
        let from = from_raw
            .parse::<Mailbox>()
            .map_err(|_| "MONOIZE_SMTP_FROM must be a valid email address".to_string())?;
        let from_name = env_value("MONOIZE_SMTP_FROM_NAME");
        let security = env_value("MONOIZE_SMTP_SECURITY").unwrap_or_else(|| "starttls".to_string());
        let credentials = Credentials::new(username, password);
        let builder = match security.as_str() {
            "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
                .map_err(|error| format!("invalid SMTP host: {error}"))?,
            "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(&host)
                .map_err(|error| format!("invalid SMTP host: {error}"))?,
            _ => return Err("MONOIZE_SMTP_SECURITY must be either 'starttls' or 'tls'".to_string()),
        };
        let transport = builder
            .port(port)
            .credentials(credentials)
            .timeout(Some(Duration::from_secs(10)))
            .build();
        Ok(Some(Self {
            transport,
            from,
            from_name,
        }))
    }

    pub async fn send_verification_code(
        &self,
        recipient: &str,
        username: &str,
        code: &str,
    ) -> Result<(), String> {
        let recipient = recipient
            .parse::<Mailbox>()
            .map_err(|_| "invalid recipient email address".to_string())?;
        let from = self
            .from_name
            .as_deref()
            .map(|name| Mailbox::new(Some(name.to_string()), self.from.email.clone()))
            .unwrap_or_else(|| self.from.clone());
        let message = Message::builder()
            .from(from)
            .to(recipient)
            .subject("Verify your Monoize account")
            .header(ContentType::TEXT_PLAIN)
            .body(format!(
                "Hello {username},\n\nYour Monoize verification code is {code}. It expires in 15 minutes.\n\nIf you did not request this registration, ignore this message."
            ))
            .map_err(|error| format!("failed to build verification email: {error}"))?;
        self.transport
            .send(message)
            .await
            .map(|_| ())
            .map_err(|error| format!("SMTP delivery failed: {error}"))
    }
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
