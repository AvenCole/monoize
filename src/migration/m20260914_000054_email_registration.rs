use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement, TransactionTrait};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

async fn execute<C: ConnectionTrait>(
    connection: &C,
    backend: DbBackend,
    sql: impl Into<String>,
) -> Result<(), DbErr> {
    connection
        .execute(Statement::from_string(backend, sql.into()))
        .await?;
    Ok(())
}

async fn reject_legacy_duplicate_emails<C: ConnectionTrait>(
    connection: &C,
    backend: DbBackend,
) -> Result<(), DbErr> {
    let duplicate = connection
        .query_one(Statement::from_string(
            backend,
            "SELECT lower(trim(email)) AS email_key, COUNT(*) AS duplicate_count
             FROM users
             WHERE email IS NOT NULL AND trim(email) <> ''
             GROUP BY lower(trim(email))
             HAVING COUNT(*) > 1"
                .to_string(),
        ))
        .await?;
    if let Some(row) = duplicate {
        let key: String = row.try_get("", "email_key")?;
        let count: i64 = row.try_get("", "duplicate_count")?;
        return Err(DbErr::Custom(format!(
            "cannot create unique user email index: legacy email key {key:?} has {count} rows"
        )));
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let connection = manager.get_connection();
        let tx = connection.begin().await?;
        reject_legacy_duplicate_emails(&tx, backend).await?;
        execute(
            &tx,
            backend,
            "CREATE TABLE IF NOT EXISTS pending_registrations (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL,
                email TEXT NOT NULL,
                email_key TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                code_hash TEXT NOT NULL,
                code_sent_at TEXT NOT NULL,
                code_expires_at TEXT NOT NULL,
                attempts BIGINT NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .await?;
        execute(
            &tx,
            backend,
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_pending_registrations_email_key
             ON pending_registrations (email_key)",
        )
        .await?;
        execute(
            &tx,
            backend,
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_pending_registrations_username
             ON pending_registrations (username)",
        )
        .await?;
        execute(
            &tx,
            backend,
            "CREATE INDEX IF NOT EXISTS idx_pending_registrations_expires
             ON pending_registrations (code_expires_at)",
        )
        .await?;
        execute(
            &tx,
            backend,
            "CREATE UNIQUE INDEX IF NOT EXISTS uq_users_email_ci
             ON users (lower(trim(email)))
             WHERE email IS NOT NULL AND trim(email) <> ''",
        )
        .await?;
        tx.commit().await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let connection = manager.get_connection();
        let tx = connection.begin().await?;
        execute(&tx, backend, "DROP INDEX IF EXISTS uq_users_email_ci").await?;
        execute(&tx, backend, "DROP TABLE IF EXISTS pending_registrations").await?;
        tx.commit().await
    }
}
