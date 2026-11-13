// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! What a node's cell runtime holds whichever engine runs the cells: the
//! generations it resolves calls against, the data directory, replication,
//! wake discovery and facet streams. Each engine's `RuntimeManager` wraps one
//! and adds what only its engine knows: cells, isolates, requests.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;

use crate::asyncrt;
use crate::engine_api::AlarmObserver;
use crate::generation::Generation;
use crate::generation::GenerationId;
use crate::ltx_replication::Replication;
use crate::wake::WakeFlusher;

/// The generations a node can resolve a call against: the one it serves and
/// the superseded ones whose isolates are still draining.
///
/// One value under one lock, because installing the new generation and
/// keeping the previous one resolvable are the same instant. Held apart,
/// the window between them resolved an in-flight call from the previous
/// generation against the new deployment graph.
pub(crate) struct Generations {
    pub(crate) current: Arc<Generation>,
    pub(crate) draining: Vec<Arc<Generation>>,
}

#[derive(Clone)]
pub struct CellRuntime {
    /// The generations a call can resolve against. A reader takes one
    /// snapshot and never holds the lock across an await.
    pub(crate) generations: Arc<std::sync::RwLock<Generations>>,
    pub(crate) data_dir: Arc<PathBuf>,
    pub(crate) replication: Option<Replication>,
    pub(crate) wake: Option<Arc<WakeFlusher>>,
    pub(crate) alarm_observer: AlarmObserver,
    /// The facet streams of each resident root object.
    pub(crate) facets: crate::facet_streams::FacetStreams,
    pub(crate) node: Arc<str>,
    pub(crate) region: Arc<str>,
}

impl CellRuntime {
    pub(crate) fn new(
        generation: Arc<Generation>,
        data_dir: PathBuf,
        replication: Option<Replication>,
        wake: Option<Arc<WakeFlusher>>,
        alarm_observer: AlarmObserver,
        node: String,
        region: String,
    ) -> anyhow::Result<Self> {
        require_cell_scope_capacity(&data_dir)?;
        Ok(Self {
            generations: Arc::new(std::sync::RwLock::new(Generations {
                current: generation,
                draining: Vec::new(),
            })),
            data_dir: Arc::new(data_dir),
            replication,
            wake,
            alarm_observer,
            facets: Default::default(),
            node: Arc::from(node),
            region: Arc::from(region),
        })
    }

    /// The generation this node serves now.
    ///
    /// A caller that makes more than one decision from it takes the snapshot
    /// once and uses it throughout: the current generation can change between
    /// two reads, and a request must see one deployment graph, not two.
    pub fn generation(&self) -> Arc<Generation> {
        self.generations
            .read()
            .expect("generation lock poisoned")
            .current
            .clone()
    }

    /// The generation an isolate was built for: the current one when the id
    /// matches, otherwise a superseded generation still draining. Zero — the
    /// tag of an isolate built outside any generation — and an id whose
    /// generation has finished draining both resolve to the current one.
    pub fn generation_by_id(&self, id: GenerationId) -> Arc<Generation> {
        // One lock over both halves: read separately, a caller from the
        // previous generation could observe the new one as current before
        // the previous one was resolvable, find neither, and fall through to
        // the new graph -- the cross-generation call this exists to prevent.
        let generations = self.generations.read().expect("generation lock poisoned");
        if id == 0 || generations.current.id == id {
            return generations.current.clone();
        }
        generations
            .draining
            .iter()
            .find(|generation| generation.id == id)
            .cloned()
            .unwrap_or_else(|| generations.current.clone())
    }

    /// The id the next generation takes: one past the newest this node has
    /// built, draining generations included, so an id is never reused while
    /// an isolate still carries it.
    pub fn next_generation_id(&self) -> GenerationId {
        let generations = self.generations.read().expect("generation lock poisoned");
        let draining = generations
            .draining
            .iter()
            .map(|generation| generation.id)
            .max()
            .unwrap_or(0);
        generations.current.id.max(draining) + 1
    }

    /// The generations still draining, for `/state`. Each one holds its
    /// isolates until the last cell in them moves, so the caller reads the
    /// census from the generation and not only its version.
    pub fn draining_generations(&self) -> Vec<Arc<Generation>> {
        self.generations
            .read()
            .expect("generation lock poisoned")
            .draining
            .clone()
    }

    /// Resolve a client-supplied cell id to a scope.
    ///
    /// The id arrives from the network, and the scope it becomes is used as a
    /// path component and as an object-store key, so the charset gate runs
    /// first. Without it a scope carries its own path segments and `db_path`
    /// walks out of the data directory through them.
    ///
    /// The fleet-wide storage gate runs a second time on the composed scope. A
    /// bare id takes a class prefix, so the scope that reaches storage is the
    /// value that must fit.
    pub fn cell_scope(&self, id: &str) -> anyhow::Result<String> {
        if !celld_logic::cell::valid_cell_scope(id) {
            return Err(anyhow!("cell id is not a well-formed scope"));
        }
        if id.contains(':') {
            return Ok(id.to_string());
        }
        let generation = self.generation();
        let class = generation.default_do_class().ok_or_else(|| {
            anyhow!("a bare cell id requires exactly one configured Durable Object class")
        })?;
        let scope = format!("{class}:{id}");
        if !celld_logic::cell::valid_cell_scope(&scope) {
            return Err(anyhow!("cell id is not a well-formed scope"));
        }
        Ok(scope)
    }

    pub fn replication(&self) -> Option<Replication> {
        self.replication.clone()
    }

    /// Whether the deployment declared `residency` for the class of the root
    /// cell `scope`: `"celld": { "durable_objects": { "<Class>": { "residency":
    /// true } } }`. The host loop asks before the core is, because a hold the
    /// deployment never allowed must not spend the node's pin budget.
    pub fn residency_declared(&self, scope: &str) -> bool {
        let class = scope.split_once(':').map_or(scope, |(class, _)| class);
        self.generation()
            .cell_configs
            .get(class)
            .is_some_and(|config| config.residency_class(class))
    }

    /// Read the filesystem inventory after the core has replaced the exact
    /// clean predecessor lease generation.
    pub fn local_reload_cells(&self) -> anyhow::Result<Vec<celld_logic::LocalCell>> {
        let replication = self
            .replication
            .as_ref()
            .context("local reload requires replication")?;
        Ok(replication.local_cells())
    }

    pub fn replication_status(&self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match &self.replication {
            Some(replication) => replication.process_status(),
            None => Ok(None),
        }
    }

    pub async fn ensure_durable(
        &self,
        cell: &str,
        epoch: u64,
        revocable: bool,
    ) -> anyhow::Result<()> {
        let Some(replication) = &self.replication else {
            return Ok(());
        };
        for facet in self.facets.resident(cell, epoch) {
            replication.ensure_durable(&facet, epoch, revocable).await?;
        }
        replication.ensure_durable(cell, epoch, revocable).await
    }

    /// The output-gate durability wait (see `Replication::await_durable`).
    /// Returns the proved durable position and its proof source; with no
    /// replicator every position is trivially durable, and the fleet source
    /// keeps the gate read-free exactly like the old immediate release.
    pub async fn await_durable(
        &self,
        cell: &str,
        epoch: u64,
        position: u64,
    ) -> anyhow::Result<(u64, celld_logic::ProofSource)> {
        match &self.replication {
            Some(replication) => replication.await_durable(cell, epoch, position).await,
            None => Ok((position, celld_logic::ProofSource::Fleet)),
        }
    }

    /// Activate a facet's stream and answer its database file.
    pub async fn open_facet(
        &self,
        root: &str,
        epoch: u64,
        names: &[String],
        engine: celld_ltx::Engine,
    ) -> anyhow::Result<(PathBuf, bool)> {
        self.facets
            .open(
                self.replication.as_ref(),
                |cell, epoch| self.db_path(cell, epoch),
                root,
                epoch,
                names,
                engine,
            )
            .await
    }

    /// Delete a facet's stream and every stream below it.
    pub async fn delete_facet(
        &self,
        root: &str,
        epoch: u64,
        names: &[String],
    ) -> anyhow::Result<()> {
        self.facets
            .delete(
                self.replication.as_ref(),
                |cell| self.data_dir.join(&*crate::fsname::cell_path(cell)),
                root,
                epoch,
                names,
            )
            .await
    }

    /// Fork the Dolt facet at `source` into the new Dolt facet at `target`,
    /// both below `root` at its resident `epoch`.
    ///
    /// The target's stream is activated first and must be empty: nothing in
    /// the bucket and no local file, so a fork never overwrites a facet.
    /// The source's file is read at its newest sealed root (a chunk file is
    /// append-only, so any reader sees a committed prefix), and its stream
    /// is proved durable past that read before the target is written, so a
    /// fork never starts from state the source could still lose. The target
    /// is then proved too: once this returns, the fork survives a crash.
    pub async fn fork_facet(
        &self,
        root: &str,
        epoch: u64,
        source: &[String],
        target: &[String],
        at: Option<&str>,
    ) -> anyhow::Result<u64> {
        let engine = celld_ltx::Engine::Dolt;
        let (target_path, restored) = self.open_facet(root, epoch, target, engine).await?;
        let (source_path, _) = self.open_facet(root, epoch, source, engine).await?;
        let at = at.map(str::to_string);
        let checked = target_path.clone();
        let image = asyncrt::blocking(move || -> anyhow::Result<Vec<u8>> {
            crate::dolt_cell::ensure_fork_target(&checked, restored)?;
            crate::dolt_cell::fork_facet_image(&source_path, &checked, at.as_deref())
        })
        .await??;
        let source_stream = crate::engine_api::facet_cell(root, source);
        self.prove_facet(&source_stream, epoch)
            .await
            .context("prove the source facet before its fork")?;
        let size = image.len() as u64;
        let installed = target_path.clone();
        asyncrt::blocking(move || -> anyhow::Result<()> {
            crate::dolt_cell::ensure_fork_target(&installed, restored)?;
            crate::dolt_cell::install_fork_image(&installed, &image)
        })
        .await??;
        let target_stream = crate::engine_api::facet_cell(root, target);
        self.prove_facet(&target_stream, epoch)
            .await
            .context("prove the forked facet")?;
        Ok(size)
    }

    /// Prove every committed write of a facet's stream durable.
    pub async fn prove_facet(&self, stream: &str, epoch: u64) -> anyhow::Result<()> {
        self.await_durable(stream, epoch, 0).await.map(|_| ())
    }

    pub(crate) fn db_path(&self, cell: &str, epoch: u64) -> PathBuf {
        // A runtime without replication has no remote epoch namespace, so it
        // keeps its only SQLite family at the existing e1 path across logical
        // ownership epochs. The stable path survives a restart, needs no
        // multi-file SQLite family move, and remains compatible with older
        // releases.
        let epoch = if self.replication.is_some() { epoch } else { 1 };
        self.data_dir
            .join(&*crate::fsname::cell_path(cell))
            .join("ltx")
            .join(format!("e{epoch}"))
            .join("db.sqlite")
    }

    pub fn alarm_covered(&self, cell: &str, alarm: celld_logic::wake::AlarmSnapshot) -> bool {
        match (alarm.at_ms(), &self.wake) {
            (None, _) => true,
            (Some(_), Some(wake)) if self.replication.is_some() => wake.covered(cell, alarm),
            (Some(_), None) => false,
            (Some(_), Some(_)) => false,
        }
    }

    pub fn node(&self) -> &str {
        &self.node
    }

    pub fn region(&self) -> &str {
        &self.region
    }
}

fn require_cell_scope_capacity(data_dir: &Path) -> anyhow::Result<()> {
    asyncrt::fs()
        .create_dir_all(data_dir)
        .with_context(|| format!("create data directory {}", data_dir.display()))?;
    let reported = asyncrt::filesystem_name_max(data_dir)
        .with_context(|| format!("read NAME_MAX for {}", data_dir.display()))?
        .with_context(|| {
            format!(
                "the filesystem does not report NAME_MAX for {}",
                data_dir.display()
            )
        })?;
    let name_max = usize::try_from(reported).context("NAME_MAX does not fit usize")?;
    require_cell_scope_name_max(name_max)
}

pub(crate) fn require_cell_scope_name_max(name_max: usize) -> anyhow::Result<()> {
    anyhow::ensure!(
        name_max >= celld_logic::cell::MAX_CELL_SCOPE,
        "the data filesystem supports {name_max}-byte names, but celld requires {}",
        celld_logic::cell::MAX_CELL_SCOPE
    );
    Ok(())
}
