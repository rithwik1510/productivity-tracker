//! The single writer: applies engine ops and config changes. Only the poll thread holds one.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use tracker_api::{
    AppIdentity, Appearance, Category, Site, TargetRef, normalize_domain, watched_by_default,
};

use super::{
    SqlAppearance, SqlCategory, StoreError, connect_writer, identity_columns, identity_from_columns,
};
use crate::engine::{Key, Op, Rules, Targets, TrackedApp, TrackedSite, fold};

const MIN_GOAL_MINUTES: u32 = 30;
const MAX_GOAL_MINUTES: u32 = 960;
const MIN_KEYWORD_CHARS: usize = 3;
/// The "Count as away after" choices, as schema v5 checks them.
const AWAY_AFTER_MINUTES: [u32; 4] = [2, 5, 10, 15];

pub(crate) struct Writer {
    conn: Connection,
    /// The stretch's open row; the engine keeps at most one. A row whose Close failed stays here,
    /// with its end, until a later write closes it (`close_open`).
    open: Option<OpenRow>,
    /// First row of the current stretch: trims never reach below it. Rowids only grow under a
    /// single writer, so `id >= first` is exactly "this stretch".
    stretch_first_id: Option<i64>,
    /// Cutoff of an EndStretch whose transaction hasn't committed yet; `finish_stretch` retries it.
    pending_cutoff_ms: Option<i64>,
    /// Counted time the last committed EndStretch removed: its rows' ends past the cutoff. The
    /// status line's "Away · removed 6m idle time".
    pub(crate) removed_ms: i64,
}

/// An open row; its end lives in memory between flushes.
struct OpenRow {
    id: i64,
    end_ms: i64,
}

impl Writer {
    /// Opens and migrates the DB, then recovers from a crash: the unfinished stretch is trimmed to the cutoff its last flush stored (as a shutdown would have), every row left open
    /// is closed at its last flushed end, and rows that never got any length are dropped.
    pub(crate) fn open(data_dir: &Path) -> Result<Self, StoreError> {
        let mut conn = connect_writer(data_dir)?;
        let tx = conn.transaction()?;
        let pending: Option<(i64, i64)> = tx
            .query_row("SELECT first_id, cutoff_ms FROM pending_trim", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        if let Some((first, cutoff_ms)) = pending {
            trim(&tx, first, cutoff_ms)?;
            // Applied once: left behind, it would trim later stretches' rows at the next start.
            tx.execute("DELETE FROM pending_trim", [])?;
        }
        tx.execute("UPDATE interval SET open = 0 WHERE open = 1", [])?;
        tx.execute("DELETE FROM interval WHERE end_ms = start_ms", [])?;
        tx.commit()?;
        Ok(Self {
            conn,
            open: None,
            stretch_first_id: None,
            pending_cutoff_ms: None,
            removed_ms: 0,
        })
    }

    /// Tracked apps and sites as the engine needs them.
    pub(crate) fn targets(&self) -> Result<Targets, StoreError> {
        let mut apps = self.conn.prepare(
            "SELECT id, kind, key, name, category, watch FROM app WHERE category IS NOT NULL",
        )?;
        let apps = apps
            .query_map([], |row| {
                let kind: String = row.get(1)?;
                let identity = identity_from_columns(&kind, row.get(2)?)?;
                let SqlCategory(category) = row.get(4)?;
                let app = TrackedApp {
                    id: row.get(0)?,
                    name: row.get(3)?,
                    category,
                    watch: row.get(5)?,
                };
                Ok((identity, app))
            })?
            .collect::<Result<_, _>>()?;
        let mut sites = self.conn.prepare(
            "SELECT id, domain, keyword, category FROM site WHERE category IS NOT NULL ORDER BY domain",
        )?;
        let sites = sites
            .query_map([], |row| {
                let SqlCategory(category) = row.get(3)?;
                Ok(TrackedSite {
                    id: row.get(0)?,
                    domain: row.get(1)?,
                    keyword: row.get(2)?,
                    category,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(Targets { apps, sites })
    }

    /// The engine's rules from the settings row.
    pub(crate) fn rules(&self) -> Result<Rules, StoreError> {
        let rules = self.conn.query_row(
            "SELECT away_after_minutes, waiting_credit, paused_until_ms FROM settings WHERE id = 1",
            [],
            |row| {
                Ok(Rules {
                    away_after_ms: row.get::<_, i64>(0)? * 60_000,
                    waiting_credit: row.get(1)?,
                    paused_until_ms: row.get(2)?,
                })
            },
        )?;
        Ok(rules)
    }

    /// Applies one op. A failed EndStretch must land first (`finish_stretch`): until it does, a
    /// new stretch's rows would fall inside its trims. The runtime stops applying a tick's ops at
    /// the first error and has the engine forget its open row, so the two never disagree.
    pub(crate) fn apply(&mut self, op: Op) -> Result<(), StoreError> {
        debug_assert!(self.pending_cutoff_ms.is_none(), "EndStretch still pending");
        match op {
            Op::Open {
                key:
                    Key {
                        target,
                        category,
                        minimized,
                    },
                start_ms,
            } => {
                // A row whose Close failed ends where that Close said, before this one starts.
                self.close_open()?;
                let (app_id, site_id) = match target {
                    TargetRef::App { id } => (Some(id), None),
                    TargetRef::Site { id } => (None, Some(id)),
                };
                self.conn.execute(
                    "INSERT INTO interval (app_id, site_id, category, start_ms, end_ms, open, minimized)
                     VALUES (?1, ?2, ?3, ?4, ?4, 1, ?5)",
                    params![app_id, site_id, SqlCategory(category), start_ms, minimized],
                )?;
                let id = self.conn.last_insert_rowid();
                self.stretch_first_id.get_or_insert(id);
                self.open = Some(OpenRow {
                    id,
                    end_ms: start_ms,
                });
            }
            Op::Extend { end_ms } => {
                if let Some(open) = &mut self.open {
                    open.end_ms = end_ms;
                }
            }
            Op::Close { end_ms } => {
                if let Some(open) = &mut self.open {
                    open.end_ms = end_ms;
                }
                self.close_open()?;
            }
            Op::EndStretch { cutoff_ms } => {
                self.pending_cutoff_ms = Some(cutoff_ms);
                self.finish_stretch()?;
            }
        }
        Ok(())
    }

    /// Runs the pending EndStretch, if any. The stretch and open rows are cleared only after the
    /// commit, so a failed transaction (disk full, a lock timeout) is retried on the next tick
    /// instead of leaving the stretch untrimmed and open.
    pub(crate) fn finish_stretch(&mut self) -> Result<(), StoreError> {
        let Some(cutoff_ms) = self.pending_cutoff_ms else {
            return Ok(());
        };
        self.removed_ms = match self.stretch_first_id {
            Some(first) => {
                let tx = self.conn.transaction()?;
                write_end(&tx, self.open.as_ref())?;
                let removed_ms = trim(&tx, first, cutoff_ms)?;
                tx.execute("DELETE FROM pending_trim", [])?;
                tx.commit()?;
                removed_ms
            }
            // The stretch never counted anything.
            None => 0,
        };
        self.open = None;
        self.stretch_first_id = None;
        self.pending_cutoff_ms = None;
        Ok(())
    }

    /// Closes the open row at its in-memory end. It stays the open row until the UPDATE lands, so
    /// a failed close keeps its end for the next try (the next Open, or the stretch's trim)
    /// instead of falling back to the last flush.
    fn close_open(&mut self) -> Result<(), StoreError> {
        if let Some(open) = &self.open {
            self.conn.execute(
                "UPDATE interval SET end_ms = ?2, open = 0 WHERE id = ?1",
                params![open.id, open.end_ms],
            )?;
            self.open = None;
        }
        Ok(())
    }

    /// Writes the open row's in-memory end, and with them `stop_cutoff` (the engine's
    /// `Engine::stop_cutoff`) for crash recovery, so a crash loses at most one flush interval and
    /// never keeps the idle tail a shutdown would have trimmed.
    pub(crate) fn flush(&mut self, stop_cutoff: Option<i64>) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        write_end(&tx, self.open.as_ref())?;
        if let (Some(first), Some(cutoff_ms)) = (self.stretch_first_id, stop_cutoff) {
            tx.execute(
                "INSERT INTO pending_trim (id, first_id, cutoff_ms) VALUES (1, ?1, ?2)
                 ON CONFLICT (id) DO UPDATE SET first_id = excluded.first_id, cutoff_ms = excluded.cutoff_ms",
                params![first, cutoff_ms],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// `category: None` stops tracking; the row stays so history keeps the name, and so does its
    /// watch setting. An app's first row starts watched if it is a preset agent app
    /// (`watched_by_default`).
    pub(crate) fn set_app_category(
        &self,
        identity: &AppIdentity,
        name: &str,
        category: Option<Category>,
    ) -> Result<(), StoreError> {
        let (kind, key) = identity_columns(identity);
        self.conn.execute(
            "INSERT INTO app (kind, key, name, category, watch) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (kind, key) DO UPDATE SET name = excluded.name, category = excluded.category",
            params![
                kind,
                key,
                name,
                category.map(SqlCategory),
                watched_by_default(identity)
            ],
        )?;
        Ok(())
    }

    /// "Counts while I watch" (ADR-0008). Only an app with a row (tracked now or before) has one.
    pub(crate) fn set_app_watch(
        &self,
        identity: &AppIdentity,
        watch: bool,
    ) -> Result<(), StoreError> {
        let (kind, key) = identity_columns(identity);
        let changed = self.conn.execute(
            "UPDATE app SET watch = ?3 WHERE kind = ?1 AND key = ?2",
            params![kind, key, watch],
        )?;
        if changed == 0 {
            return Err(StoreError::Invalid(
                "Track this app first, then choose whether it counts while you watch.".into(),
            ));
        }
        Ok(())
    }

    /// Creates or updates a site by normalized domain. `keyword: None` keeps an existing keyword,
    /// and on create defaults to the domain's first label if that has at least 3 characters. A
    /// keyword may list several, comma-separated, each of at least 3 characters; it is stored
    /// folded, as "a, b".
    pub(crate) fn upsert_site(
        &self,
        domain: &str,
        keyword: Option<&str>,
        category: Option<Category>,
    ) -> Result<Site, StoreError> {
        let domain = normalize_domain(domain).map_err(|e| StoreError::Invalid(e.message))?;
        let keyword = keyword
            .map(|k| {
                let keywords: Vec<String> = fold(k).split(',').map(|k| k.trim().into()).collect();
                if keywords
                    .iter()
                    .any(|k| k.chars().count() < MIN_KEYWORD_CHARS)
                {
                    Err(StoreError::Invalid(
                        "A keyword needs at least 3 characters.".into(),
                    ))
                } else {
                    Ok(keywords.join(", "))
                }
            })
            .transpose()?;
        let default_keyword = domain
            .split('.')
            .next()
            .filter(|label| label.chars().count() >= MIN_KEYWORD_CHARS);
        let site = self.conn.query_row(
            "INSERT INTO site (domain, keyword, category) VALUES (?1, COALESCE(?2, ?3), ?4)
             ON CONFLICT (domain) DO UPDATE SET keyword = COALESCE(?2, keyword), category = excluded.category
             RETURNING id, domain, keyword, category",
            params![domain, keyword, default_keyword, category.map(SqlCategory)],
            |row| {
                Ok(Site {
                    id: row.get(0)?,
                    domain: row.get(1)?,
                    keyword: row.get(2)?,
                    category: row.get::<_, Option<SqlCategory>>(3)?.map(|c| c.0),
                })
            },
        )?;
        Ok(site)
    }

    pub(crate) fn set_daily_goal(&self, minutes: u32) -> Result<(), StoreError> {
        if !(MIN_GOAL_MINUTES..=MAX_GOAL_MINUTES).contains(&minutes) {
            return Err(StoreError::Invalid(
                "The daily goal must be between 30 minutes and 16 hours.".into(),
            ));
        }
        self.conn.execute(
            "UPDATE settings SET daily_goal_minutes = ?1 WHERE id = 1",
            [minutes],
        )?;
        Ok(())
    }

    pub(crate) fn set_appearance(&self, appearance: Appearance) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE settings SET appearance = ?1 WHERE id = 1",
            [SqlAppearance(appearance)],
        )?;
        Ok(())
    }

    pub(crate) fn complete_onboarding(&self) -> Result<(), StoreError> {
        self.conn
            .execute("UPDATE settings SET onboarded = 1 WHERE id = 1", [])?;
        Ok(())
    }

    pub(crate) fn set_away_after(&self, minutes: u32) -> Result<(), StoreError> {
        if !AWAY_AFTER_MINUTES.contains(&minutes) {
            return Err(StoreError::Invalid(
                "Away can start after 2, 5, 10 or 15 minutes.".into(),
            ));
        }
        self.conn.execute(
            "UPDATE settings SET away_after_minutes = ?1 WHERE id = 1",
            [minutes],
        )?;
        Ok(())
    }

    pub(crate) fn set_waiting_credit(&self, enabled: bool) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE settings SET waiting_credit = ?1 WHERE id = 1",
            [enabled],
        )?;
        Ok(())
    }

    pub(crate) fn set_auto_update(&self, enabled: bool) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE settings SET auto_update = ?1 WHERE id = 1",
            [enabled],
        )?;
        Ok(())
    }

    /// `None` resumes.
    pub(crate) fn set_paused_until(&self, until_ms: Option<i64>) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE settings SET paused_until_ms = ?1 WHERE id = 1",
            [until_ms],
        )?;
        Ok(())
    }

    /// Deletes every row and puts the settings back to a new install's defaults (the column
    /// defaults, so waiting credit is off), so the app starts over at first run. The open row goes too, and nothing in memory refers to a row any more. Then
    /// VACUUM and a truncating checkpoint, so the deleted history doesn't linger in free pages of
    /// the file or in the WAL.
    pub(crate) fn delete_all(&mut self) -> Result<(), StoreError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM interval;
             DELETE FROM pending_trim;
             DELETE FROM app;
             DELETE FROM site;
             DELETE FROM settings;
             INSERT INTO settings (id) VALUES (1);",
        )?;
        tx.commit()?;
        self.open = None;
        self.stretch_first_id = None;
        self.pending_cutoff_ms = None;
        self.removed_ms = 0;
        // The wipe has committed; a VACUUM that can't run now (the reader busy) only leaves free
        // pages behind.
        let compacted = self.conn.execute_batch("VACUUM").and_then(|()| {
            self.conn
                .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        });
        if let Err(e) = compacted {
            tracing::warn!("couldn't compact the database after deleting everything: {e}");
        }
        Ok(())
    }
}

/// Writes the open row's in-memory end, if a row is open.
fn write_end(tx: &Transaction<'_>, open: Option<&OpenRow>) -> Result<(), StoreError> {
    if let Some(row) = open {
        tx.execute(
            "UPDATE interval SET end_ms = ?2 WHERE id = ?1",
            params![row.id, row.end_ms],
        )?;
    }
    Ok(())
}

/// Trims the stretch whose first row is `first` back to `cutoff_ms` and closes its rows, returning
/// the counted ms it removed. Never touches earlier stretches. Deletes before clamping, so every
/// UPDATE keeps CHECK (end_ms >= start_ms) true.
fn trim(tx: &Transaction<'_>, first: i64, cutoff_ms: i64) -> Result<i64, StoreError> {
    let removed_ms = tx.query_row(
        "SELECT COALESCE(SUM(end_ms - MAX(start_ms, ?2)), 0) FROM interval WHERE id >= ?1 AND end_ms > ?2",
        params![first, cutoff_ms],
        |row| row.get(0),
    )?;
    tx.execute(
        "DELETE FROM interval WHERE id >= ?1 AND start_ms >= ?2",
        params![first, cutoff_ms],
    )?;
    tx.execute(
        "UPDATE interval SET end_ms = MIN(end_ms, ?2), open = 0 WHERE id >= ?1",
        params![first, cutoff_ms],
    )?;
    Ok(removed_ms)
}
