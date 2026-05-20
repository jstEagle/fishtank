use anyhow::{Context, Result};
use async_trait::async_trait;
use fishtank_protocol::{CommandEnvelope, Event, SCHEMA_VERSION, WorldSnapshot};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tokio::fs;

#[derive(Clone, Debug)]
pub struct StoredState {
    pub snapshot: WorldSnapshot,
    pub events: Vec<Event>,
}

#[async_trait]
pub trait Storage: Send + Sync {
    async fn load(&self) -> Result<Option<StoredState>>;
    async fn save(
        &self,
        snapshot: &WorldSnapshot,
        events: &[Event],
        commands: &[CommandEnvelope],
    ) -> Result<()>;
    async fn character_for_token(&self, token_hash: &str) -> Result<Option<String>>;
    async fn bind_token(&self, token_hash: &str, character_id: &str) -> Result<()>;
    async fn delete_tokens_for_character(&self, character_id: &str) -> Result<u64>;
}

#[derive(Clone, Debug)]
pub struct FileStorage {
    state_dir: PathBuf,
}

impl FileStorage {
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }
}

#[async_trait]
impl Storage for FileStorage {
    async fn load(&self) -> Result<Option<StoredState>> {
        let snapshot_path = self.state_dir.join("snapshot.json");
        if !snapshot_path.exists() {
            return Ok(None);
        }
        let snapshot = serde_json::from_str::<WorldSnapshot>(
            &fs::read_to_string(&snapshot_path)
                .await
                .with_context(|| format!("failed to read {}", snapshot_path.display()))?,
        )?;
        let events = read_ndjson::<Event>(&self.state_dir.join("events.ndjson")).await?;
        Ok(Some(StoredState { snapshot, events }))
    }

    async fn save(
        &self,
        snapshot: &WorldSnapshot,
        events: &[Event],
        commands: &[CommandEnvelope],
    ) -> Result<()> {
        fs::create_dir_all(&self.state_dir)
            .await
            .with_context(|| format!("failed to create {}", self.state_dir.display()))?;
        fs::write(
            self.state_dir.join("snapshot.json"),
            serde_json::to_string_pretty(snapshot)?,
        )
        .await?;
        write_ndjson(&self.state_dir.join("events.ndjson"), events).await?;
        write_ndjson(&self.state_dir.join("commands.ndjson"), commands).await?;
        Ok(())
    }

    async fn character_for_token(&self, token_hash: &str) -> Result<Option<String>> {
        let tokens = read_json_map(&self.state_dir.join("tokens.json")).await?;
        Ok(tokens.get(token_hash).cloned())
    }

    async fn bind_token(&self, token_hash: &str, character_id: &str) -> Result<()> {
        fs::create_dir_all(&self.state_dir)
            .await
            .with_context(|| format!("failed to create {}", self.state_dir.display()))?;
        let path = self.state_dir.join("tokens.json");
        let mut tokens = read_json_map(&path).await?;
        tokens.insert(token_hash.to_string(), character_id.to_string());
        fs::write(path, serde_json::to_string_pretty(&tokens)?).await?;
        Ok(())
    }

    async fn delete_tokens_for_character(&self, character_id: &str) -> Result<u64> {
        let path = self.state_dir.join("tokens.json");
        let mut tokens = read_json_map(&path).await?;
        let before = tokens.len();
        tokens.retain(|_, bound_character_id| bound_character_id != character_id);
        fs::create_dir_all(&self.state_dir)
            .await
            .with_context(|| format!("failed to create {}", self.state_dir.display()))?;
        fs::write(path, serde_json::to_string_pretty(&tokens)?).await?;
        Ok((before - tokens.len()) as u64)
    }
}
#[derive(Clone)]
pub struct PgStorage {
    pool: PgPool,
    world_id: String,
}

impl PgStorage {
    pub async fn connect(database_url: &str, world_id: String) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;
        let storage = Self { pool, world_id };
        storage.ensure_schema().await?;
        Ok(storage)
    }

    async fn ensure_schema(&self) -> Result<()> {
        for statement in [
            r#"
            create table if not exists fishtank_meta (
                key text primary key,
                value text not null
            )
            "#,
            r#"
            create table if not exists fishtank_world_state (
                world_id text primary key,
                snapshot jsonb not null,
                events jsonb not null,
                commands jsonb not null,
                updated_at timestamptz not null default now()
            )
            "#,
            r#"
            create table if not exists fishtank_agent_tokens (
                token_hash text primary key,
                character_id text not null unique,
                created_at timestamptz not null default now()
            )
            "#,
        ] {
            sqlx::query(statement).execute(&self.pool).await?;
        }
        sqlx::query(
            r#"
            insert into fishtank_meta (key, value)
            values ('schema_version', $1)
            on conflict (key) do update set value = excluded.value
            "#,
        )
        .bind(SCHEMA_VERSION)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl Storage for PgStorage {
    async fn load(&self) -> Result<Option<StoredState>> {
        let row = sqlx::query_as::<_, (serde_json::Value, serde_json::Value, serde_json::Value)>(
            "select snapshot, events, commands from fishtank_world_state where world_id = $1",
        )
        .bind(&self.world_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(snapshot, events, commands)| {
            let _commands: Vec<CommandEnvelope> = serde_json::from_value(commands)?;
            Ok(StoredState {
                snapshot: serde_json::from_value(snapshot)?,
                events: serde_json::from_value(events)?,
            })
        })
        .transpose()
    }

    async fn save(
        &self,
        snapshot: &WorldSnapshot,
        events: &[Event],
        commands: &[CommandEnvelope],
    ) -> Result<()> {
        sqlx::query(
            r#"
            insert into fishtank_world_state (world_id, snapshot, events, commands, updated_at)
            values ($1, $2, $3, $4, now())
            on conflict (world_id) do update
            set snapshot = excluded.snapshot,
                events = excluded.events,
                commands = excluded.commands,
                updated_at = now()
            "#,
        )
        .bind(&self.world_id)
        .bind(serde_json::to_value(snapshot)?)
        .bind(serde_json::to_value(events)?)
        .bind(serde_json::to_value(commands)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn character_for_token(&self, token_hash: &str) -> Result<Option<String>> {
        let row = sqlx::query_as::<_, (String,)>(
            "select character_id from fishtank_agent_tokens where token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(character_id,)| character_id))
    }

    async fn bind_token(&self, token_hash: &str, character_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            insert into fishtank_agent_tokens (token_hash, character_id)
            values ($1, $2)
            on conflict (token_hash) do update set character_id = excluded.character_id
            "#,
        )
        .bind(token_hash)
        .bind(character_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_tokens_for_character(&self, character_id: &str) -> Result<u64> {
        let result = sqlx::query("delete from fishtank_agent_tokens where character_id = $1")
            .bind(character_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

async fn read_ndjson<T>(path: &Path) -> Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    if !path.exists() {
        return Ok(Vec::new());
    }
    let input = fs::read_to_string(path).await?;
    input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<T>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

async fn read_json_map(path: &Path) -> Result<BTreeMap<String, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path).await?)?)
}

async fn write_ndjson<T>(path: &Path, values: &[T]) -> Result<()>
where
    T: serde::Serialize,
{
    let mut output = String::new();
    for value in values {
        output.push_str(&serde_json::to_string(value)?);
        output.push('\n');
    }
    fs::write(path, output).await?;
    Ok(())
}
