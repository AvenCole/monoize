use super::{User, UserRole, UserStore};
use crate::db::DbPool;
use chrono::{DateTime, Duration, Utc};
use rand_core::{OsRng, RngCore};
use sea_orm::{ConnectionTrait, QueryResult, Value as SeaValue};

pub(crate) const VERIFICATION_TTL_MINUTES: i64 = 15;
pub(crate) const VERIFICATION_RESEND_COOLDOWN_SECONDS: i64 = 60;
pub(crate) const VERIFICATION_MAX_ATTEMPTS: i64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmailRegistrationError {
    RegistrationDisabled,
    UsernameExists,
    EmailExists,
    VerificationCooldown { retry_after: DateTime<Utc> },
    VerificationExpired,
    VerificationInvalid,
    VerificationAttemptsExceeded,
    Storage(String),
}

#[derive(Debug, Clone)]
pub struct RegistrationDispatch {
    pub registration_id: String,
    pub username: String,
    pub email: String,
    pub code: String,
    pub expires_at: DateTime<Utc>,
    pub resend_after: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct PendingRegistration {
    id: String,
    username: String,
    email: String,
    password_hash: String,
    code_hash: String,
    code_sent_at: DateTime<Utc>,
    code_expires_at: DateTime<Utc>,
    attempts: i64,
}

fn parse_timestamp(raw: String, field: &str) -> Result<DateTime<Utc>, EmailRegistrationError> {
    DateTime::parse_from_rfc3339(&raw)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| EmailRegistrationError::Storage(format!("invalid {field}: {error}")))
}

fn pending_from_row(row: &QueryResult) -> Result<PendingRegistration, EmailRegistrationError> {
    let code_sent_at = parse_timestamp(
        row.try_get("", "code_sent_at")
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?,
        "pending_registrations.code_sent_at",
    )?;
    let code_expires_at = parse_timestamp(
        row.try_get("", "code_expires_at")
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?,
        "pending_registrations.code_expires_at",
    )?;
    Ok(PendingRegistration {
        id: row
            .try_get("", "id")
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?,
        username: row
            .try_get("", "username")
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?,
        email: row
            .try_get("", "email")
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?,
        password_hash: row
            .try_get("", "password_hash")
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?,
        code_hash: row
            .try_get("", "code_hash")
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?,
        code_sent_at,
        code_expires_at,
        attempts: row
            .try_get("", "attempts")
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?,
    })
}

fn generate_code() -> String {
    let mut random = [0_u8; 4];
    OsRng.fill_bytes(&mut random);
    let value = u32::from_le_bytes(random) % 900_000 + 100_000;
    value.to_string()
}

fn is_unique_violation(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("unique") || lower.contains("duplicate") || lower.contains("constraint")
}

async fn registration_enabled<C: ConnectionTrait>(
    db: &DbPool,
    connection: &C,
) -> Result<bool, EmailRegistrationError> {
    let row = connection
        .query_one(db.stmt(
            "SELECT value FROM system_settings WHERE key = $1",
            vec!["registration_enabled".into()],
        ))
        .await
        .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
    let Some(row) = row else {
        return Ok(true);
    };
    let raw: String = row
        .try_get("", "value")
        .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
    match raw.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        value => Err(EmailRegistrationError::Storage(format!(
            "invalid persisted registration_enabled value: {value:?}"
        ))),
    }
}

async fn non_internal_user_count<C: ConnectionTrait>(
    db: &DbPool,
    connection: &C,
) -> Result<i64, EmailRegistrationError> {
    let row = connection
        .query_one(db.stmt(
            "SELECT COUNT(*) AS count FROM users WHERE substr(lower(username), 1, 9) != '_monoize_'",
            vec![],
        ))
        .await
        .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?
        .ok_or_else(|| EmailRegistrationError::Storage("user count row missing".to_string()))?;
    row.try_get("", "count")
        .map_err(|error| EmailRegistrationError::Storage(error.to_string()))
}

impl UserStore {
    /// Create or replace a pending registration. The caller sends the returned
    /// code only after this transaction commits.
    pub async fn begin_email_registration(
        &self,
        username: &str,
        email: &str,
        password: &str,
    ) -> Result<RegistrationDispatch, EmailRegistrationError> {
        let password_hash = Self::hash_password_async(password)
            .await
            .map_err(EmailRegistrationError::Storage)?;
        let code = generate_code();
        let code_hash = Self::hash_password_async(&code)
            .await
            .map_err(EmailRegistrationError::Storage)?;
        let now = Utc::now();
        let expires_at = now + Duration::minutes(VERIFICATION_TTL_MINUTES);
        let resend_after = now + Duration::seconds(VERIFICATION_RESEND_COOLDOWN_SECONDS);
        let registration_id = uuid::Uuid::new_v4().to_string();
        let email_key = email.to_ascii_lowercase();

        let _registration_guard = self.registration_lock.lock().await;
        let tx = self
            .db
            .begin_write()
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        let user_count = non_internal_user_count(&self.db, &*tx).await?;
        let registration_is_enabled = registration_enabled(&self.db, &*tx).await?;
        if user_count != 0 && !registration_is_enabled {
            return Err(EmailRegistrationError::RegistrationDisabled);
        }

        let username_row = tx
            .query_one(self.db.stmt(
                "SELECT id FROM users WHERE username = $1",
                vec![username.into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        if username_row.is_some() {
            return Err(EmailRegistrationError::UsernameExists);
        }

        let email_row = tx
            .query_one(self.db.stmt(
                "SELECT id FROM users WHERE email IS NOT NULL AND trim(email) <> '' AND lower(trim(email)) = $1",
                vec![email_key.clone().into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        if email_row.is_some() {
            return Err(EmailRegistrationError::EmailExists);
        }

        let pending_username = tx
            .query_one(self.db.stmt(
                "SELECT id FROM pending_registrations WHERE username = $1 AND email_key <> $2",
                vec![username.into(), email_key.clone().into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        if pending_username.is_some() {
            return Err(EmailRegistrationError::UsernameExists);
        }

        let previous = tx
            .query_one(self.db.stmt(
                "SELECT code_sent_at FROM pending_registrations WHERE email_key = $1",
                vec![email_key.clone().into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        if let Some(previous) = previous {
            let sent_at = parse_timestamp(
                previous
                    .try_get("", "code_sent_at")
                    .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?,
                "pending_registrations.code_sent_at",
            )?;
            let retry_after = sent_at + Duration::seconds(VERIFICATION_RESEND_COOLDOWN_SECONDS);
            if retry_after > now {
                return Err(EmailRegistrationError::VerificationCooldown { retry_after });
            }
        }

        let result = tx
            .execute(self.db.stmt(
                "INSERT INTO pending_registrations (id, username, email, email_key, password_hash, code_hash, code_sent_at, code_expires_at, attempts, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9, $9)
                 ON CONFLICT(email_key) DO UPDATE SET id = excluded.id, username = excluded.username, email = excluded.email, password_hash = excluded.password_hash, code_hash = excluded.code_hash, code_sent_at = excluded.code_sent_at, code_expires_at = excluded.code_expires_at, attempts = 0, updated_at = excluded.updated_at",
                vec![
                    registration_id.clone().into(),
                    username.into(),
                    email.into(),
                    email_key.into(),
                    password_hash.into(),
                    code_hash.into(),
                    now.to_rfc3339().into(),
                    expires_at.to_rfc3339().into(),
                    now.to_rfc3339().into(),
                ],
            ))
            .await;
        if let Err(error) = result {
            let text = error.to_string();
            if is_unique_violation(&text) {
                return Err(EmailRegistrationError::UsernameExists);
            }
            return Err(EmailRegistrationError::Storage(text));
        }
        tx.commit()
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;

        Ok(RegistrationDispatch {
            registration_id,
            username: username.to_string(),
            email: email.to_string(),
            code,
            expires_at,
            resend_after,
        })
    }

    pub async fn resend_email_registration(
        &self,
        registration_id: &str,
    ) -> Result<RegistrationDispatch, EmailRegistrationError> {
        let code = generate_code();
        let code_hash = Self::hash_password_async(&code)
            .await
            .map_err(EmailRegistrationError::Storage)?;
        let now = Utc::now();
        let expires_at = now + Duration::minutes(VERIFICATION_TTL_MINUTES);
        let resend_after = now + Duration::seconds(VERIFICATION_RESEND_COOLDOWN_SECONDS);

        let _registration_guard = self.registration_lock.lock().await;
        let tx = self
            .db
            .begin_write()
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        let row = tx
            .query_one(self.db.stmt(
                "SELECT id, username, email, password_hash, code_hash, code_sent_at, code_expires_at, attempts FROM pending_registrations WHERE id = $1",
                vec![registration_id.into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        let Some(row) = row else {
            return Err(EmailRegistrationError::VerificationExpired);
        };
        let pending = pending_from_row(&row)?;
        if pending.code_expires_at <= now {
            tx.execute(self.db.stmt(
                "DELETE FROM pending_registrations WHERE id = $1",
                vec![registration_id.into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            tx.commit()
                .await
                .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            return Err(EmailRegistrationError::VerificationExpired);
        }

        let count = non_internal_user_count(&self.db, &*tx).await?;
        let registration_is_enabled = registration_enabled(&self.db, &*tx).await?;
        if count != 0 && !registration_is_enabled {
            return Err(EmailRegistrationError::RegistrationDisabled);
        }
        let retry_after =
            pending.code_sent_at + Duration::seconds(VERIFICATION_RESEND_COOLDOWN_SECONDS);
        if retry_after > now {
            return Err(EmailRegistrationError::VerificationCooldown { retry_after });
        }

        tx.execute(self.db.stmt(
            "UPDATE pending_registrations SET code_hash = $1, code_sent_at = $2, code_expires_at = $3, attempts = 0, updated_at = $2 WHERE id = $4",
            vec![
                code_hash.into(),
                now.to_rfc3339().into(),
                expires_at.to_rfc3339().into(),
                registration_id.into(),
            ],
        ))
        .await
        .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;

        Ok(RegistrationDispatch {
            registration_id: pending.id,
            username: pending.username,
            email: pending.email,
            code,
            expires_at,
            resend_after,
        })
    }

    pub async fn verify_email_registration(
        &self,
        registration_id: &str,
        code: &str,
    ) -> Result<User, EmailRegistrationError> {
        let _registration_guard = self.registration_lock.lock().await;
        let tx = self
            .db
            .begin_write()
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        let row = tx
            .query_one(self.db.stmt(
                "SELECT id, username, email, password_hash, code_hash, code_sent_at, code_expires_at, attempts FROM pending_registrations WHERE id = $1",
                vec![registration_id.into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        let Some(row) = row else {
            return Err(EmailRegistrationError::VerificationExpired);
        };
        let pending = pending_from_row(&row)?;
        let now = Utc::now();
        if pending.code_expires_at <= now {
            tx.execute(self.db.stmt(
                "DELETE FROM pending_registrations WHERE id = $1",
                vec![registration_id.into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            tx.commit()
                .await
                .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            return Err(EmailRegistrationError::VerificationExpired);
        }
        if pending.attempts >= VERIFICATION_MAX_ATTEMPTS {
            tx.execute(self.db.stmt(
                "DELETE FROM pending_registrations WHERE id = $1",
                vec![registration_id.into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            tx.commit()
                .await
                .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            return Err(EmailRegistrationError::VerificationAttemptsExceeded);
        }

        let valid = Self::verify_password_async(code, &pending.code_hash)
            .await
            .map_err(EmailRegistrationError::Storage)?;
        if !valid {
            let next_attempts = pending.attempts.saturating_add(1);
            if next_attempts >= VERIFICATION_MAX_ATTEMPTS {
                tx.execute(self.db.stmt(
                    "DELETE FROM pending_registrations WHERE id = $1",
                    vec![registration_id.into()],
                ))
                .await
                .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
                tx.commit()
                    .await
                    .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
                return Err(EmailRegistrationError::VerificationAttemptsExceeded);
            }
            tx.execute(self.db.stmt(
                "UPDATE pending_registrations SET attempts = $1, updated_at = $2 WHERE id = $3",
                vec![
                    SeaValue::BigInt(Some(next_attempts)),
                    now.to_rfc3339().into(),
                    registration_id.into(),
                ],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            tx.commit()
                .await
                .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            return Err(EmailRegistrationError::VerificationInvalid);
        }

        let count = non_internal_user_count(&self.db, &*tx).await?;
        let registration_is_enabled = registration_enabled(&self.db, &*tx).await?;
        if count != 0 && !registration_is_enabled {
            return Err(EmailRegistrationError::RegistrationDisabled);
        }
        let username_exists = tx
            .query_one(self.db.stmt(
                "SELECT id FROM users WHERE username = $1",
                vec![pending.username.clone().into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?
            .is_some();
        if username_exists {
            tx.execute(self.db.stmt(
                "DELETE FROM pending_registrations WHERE id = $1",
                vec![registration_id.into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            tx.commit()
                .await
                .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            return Err(EmailRegistrationError::UsernameExists);
        }
        let email_exists = tx
            .query_one(self.db.stmt(
                "SELECT id FROM users WHERE email IS NOT NULL AND trim(email) <> '' AND lower(trim(email)) = $1",
                vec![pending.email.to_ascii_lowercase().into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?
            .is_some();
        if email_exists {
            tx.execute(self.db.stmt(
                "DELETE FROM pending_registrations WHERE id = $1",
                vec![registration_id.into()],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            tx.commit()
                .await
                .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
            return Err(EmailRegistrationError::EmailExists);
        }

        let group_row = tx
            .query_one(self.db.stmt(
                "SELECT id FROM monoize_groups WHERE is_default = 1 LIMIT 1",
                vec![],
            ))
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?
            .ok_or_else(|| {
                EmailRegistrationError::Storage(
                    "default group row missing (GR-D2 violated)".to_string(),
                )
            })?;
        let group_id: String = group_row
            .try_get("", "id")
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        let role = if count == 0 {
            UserRole::SuperAdmin
        } else {
            UserRole::User
        };
        let user_id = uuid::Uuid::new_v4().to_string();
        tx.execute(self.db.stmt(
            "INSERT INTO users (id, username, password_hash, role, created_at, updated_at, enabled, balance_nano_usd, balance_unlimited, email, group_id)
             VALUES ($1, $2, $3, $4, $5, $5, 1, '0', 0, $6, $7)",
            vec![
                user_id.clone().into(),
                pending.username.clone().into(),
                pending.password_hash.clone().into(),
                role.as_str().into(),
                now.to_rfc3339().into(),
                pending.email.clone().into(),
                group_id.clone().into(),
            ],
        ))
        .await
        .map_err(|error| {
            let text = error.to_string();
            if is_unique_violation(&text) {
                EmailRegistrationError::EmailExists
            } else {
                EmailRegistrationError::Storage(text)
            }
        })?;
        tx.execute(self.db.stmt(
            "DELETE FROM pending_registrations WHERE id = $1",
            vec![registration_id.into()],
        ))
        .await
        .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;
        tx.commit()
            .await
            .map_err(|error| EmailRegistrationError::Storage(error.to_string()))?;

        Ok(User {
            id: user_id,
            username: pending.username,
            password_hash: pending.password_hash,
            role,
            created_at: now,
            updated_at: now,
            last_login_at: None,
            enabled: true,
            balance_nano_usd: "0".to_string(),
            balance_unlimited: false,
            email: Some(pending.email),
            group_id,
        })
    }

    pub async fn cleanup_expired_email_registrations(&self) -> Result<u64, String> {
        let result = self
            .db
            .write()
            .await
            .execute(self.db.stmt(
                "DELETE FROM pending_registrations WHERE code_expires_at <= $1",
                vec![Utc::now().to_rfc3339().into()],
            ))
            .await
            .map_err(|error| error.to_string())?;
        Ok(result.rows_affected())
    }
}
