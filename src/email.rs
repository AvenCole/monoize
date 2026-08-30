use crate::settings::SettingsStore;
use lettre::message::{Mailbox, Message, header::ContentType};
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncTransport, Tokio1Executor};
use std::time::Duration;

const SMTP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: Mailbox,
    pub from_name: Option<String>,
    pub use_tls: bool,
}

impl SmtpConfig {
    pub fn from_fields(
        host: String,
        port: u16,
        username: String,
        password: String,
        from: String,
        from_name: String,
        use_tls: bool,
    ) -> Result<Self, String> {
        let host = host.trim().to_string();
        let username = username.trim().to_string();
        let password = password.trim().to_string();
        let from = from.trim().to_string();
        if host.is_empty() {
            return Err("SMTP host is required".to_string());
        }
        if port == 0 {
            return Err("SMTP port must be from 1 to 65535".to_string());
        }
        if username.is_empty() {
            return Err("SMTP username is required".to_string());
        }
        if password.is_empty() {
            return Err("SMTP password is required".to_string());
        }
        let from = from
            .parse::<Mailbox>()
            .map_err(|_| "SMTP sender must be a valid email address".to_string())?;
        let from_name = (!from_name.trim().is_empty()).then(|| from_name.trim().to_string());
        Ok(Self {
            host,
            port,
            username,
            password,
            from,
            from_name,
            use_tls,
        })
    }

    fn from_env() -> Result<Option<Self>, String> {
        let Some(host) = env_value("MONOIZE_SMTP_HOST") else {
            return Ok(None);
        };
        let port = env_value("MONOIZE_SMTP_PORT")
            .ok_or_else(|| "MONOIZE_SMTP_PORT is required when SMTP is enabled".to_string())?
            .parse::<u16>()
            .map_err(|_| "MONOIZE_SMTP_PORT must be an integer from 1 to 65535".to_string())?;
        let security = env_value("MONOIZE_SMTP_SECURITY").unwrap_or_else(|| "starttls".to_string());
        let use_tls = match security.as_str() {
            "starttls" => false,
            "tls" => true,
            _ => return Err("MONOIZE_SMTP_SECURITY must be either 'starttls' or 'tls'".to_string()),
        };
        Self::from_fields(
            host,
            port,
            env_value("MONOIZE_SMTP_USERNAME").ok_or_else(|| {
                "MONOIZE_SMTP_USERNAME is required when SMTP is enabled".to_string()
            })?,
            env_value("MONOIZE_SMTP_PASSWORD").ok_or_else(|| {
                "MONOIZE_SMTP_PASSWORD is required when SMTP is enabled".to_string()
            })?,
            env_value("MONOIZE_SMTP_FROM")
                .ok_or_else(|| "MONOIZE_SMTP_FROM is required when SMTP is enabled".to_string())?,
            env_value("MONOIZE_SMTP_FROM_NAME").unwrap_or_default(),
            use_tls,
        )
        .map(Some)
    }
}

#[derive(Clone)]
pub struct EmailService {
    settings_store: SettingsStore,
    env_config: Option<SmtpConfig>,
}

impl EmailService {
    pub fn environment_config() -> Result<Option<SmtpConfig>, String> {
        SmtpConfig::from_env()
    }

    pub fn new(settings_store: SettingsStore, env_config: Option<SmtpConfig>) -> Self {
        Self {
            settings_store,
            env_config,
        }
    }

    pub async fn is_configured(&self) -> Result<bool, String> {
        Ok(self.config().await.is_ok())
    }

    async fn config(&self) -> Result<SmtpConfig, String> {
        let settings = self.settings_store.get_all().await?;
        if settings.smtp_host.trim().is_empty() {
            return self
                .env_config
                .clone()
                .ok_or_else(|| "SMTP is not configured".to_string());
        }
        SmtpConfig::from_fields(
            settings.smtp_host,
            settings.smtp_port,
            settings.smtp_username,
            settings.smtp_password,
            settings.smtp_from_email,
            settings.smtp_from_name,
            settings.smtp_use_tls,
        )
    }

    fn transport(config: &SmtpConfig) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
        let builder = if config.use_tls {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)
        }
        .map_err(|error| format!("invalid SMTP host: {error}"))?;
        Ok(builder
            .port(config.port)
            .credentials(Credentials::new(
                config.username.clone(),
                config.password.clone(),
            ))
            .timeout(Some(SMTP_TIMEOUT))
            .build())
    }

    async fn send_with_config(
        &self,
        config: &SmtpConfig,
        recipient: &str,
        subject: String,
        body: String,
    ) -> Result<(), String> {
        let recipient = recipient
            .parse::<Mailbox>()
            .map_err(|_| "invalid recipient email address".to_string())?;
        let from = config
            .from_name
            .as_deref()
            .map(|name| Mailbox::new(Some(name.to_string()), config.from.email.clone()))
            .unwrap_or_else(|| config.from.clone());
        let message = Message::builder()
            .from(from)
            .to(recipient)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)
            .map_err(|error| format!("failed to build email: {error}"))?;
        Self::transport(config)?
            .send(message)
            .await
            .map(|_| ())
            .map_err(|error| format!("SMTP delivery failed: {error}"))
    }

    pub async fn test_config(&self, config: SmtpConfig) -> Result<(), String> {
        Self::transport(&config)?
            .test_connection()
            .await
            .map(|_| ())
            .map_err(|error| format!("SMTP connection failed: {error}"))
    }

    pub async fn send_test_email(&self, config: SmtpConfig, recipient: &str) -> Result<(), String> {
        self.send_with_config(
            &config,
            recipient,
            "Monoize SMTP test".to_string(),
            "This email confirms that Monoize can deliver mail through the configured SMTP server."
                .to_string(),
        )
        .await
    }

    pub async fn send_verification_code(
        &self,
        recipient: &str,
        username: &str,
        code: &str,
    ) -> Result<(), String> {
        self.send_with_config(
            &self.config().await?,
            recipient,
            "Verify your Monoize account".to_string(),
            format!(
                "Hello {username},\n\nYour Monoize verification code is {code}. It expires in 15 minutes.\n\nIf you did not request this registration, ignore this message."
            ),
        )
        .await
    }
}

fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
