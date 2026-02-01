use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::{Semaphore, SemaphorePermit};
use tracing::{debug, info, warn};

use ironfish_core::{Error, Result};

use crate::engine::StockfishEngine;

#[derive(Debug, Clone)]
pub struct EnginePoolConfig {
    pub binary_path: String,
    pub pool_size: usize,
}

impl Default for EnginePoolConfig {
    fn default() -> Self {
        Self {
            binary_path: "/usr/bin/stockfish".to_string(),
            pool_size: 4,
        }
    }
}

pub struct EnginePool {
    engines: Vec<Arc<StockfishEngine>>,
    semaphore: Arc<Semaphore>,
    next_engine: AtomicUsize,
    active_count: AtomicUsize,
}

impl EnginePool {
    pub async fn new(config: EnginePoolConfig) -> Result<Self> {
        info!(
            "creating engine pool with {} engines from {}",
            config.pool_size, config.binary_path
        );

        let mut engines = Vec::with_capacity(config.pool_size);

        for i in 0..config.pool_size {
            match StockfishEngine::new(&config.binary_path).await {
                Ok(engine) => {
                    debug!("engine {} initialized", i);
                    engines.push(Arc::new(engine));
                }
                Err(e) => {
                    warn!("failed to create engine {}: {}", i, e);
                    for engine in &engines {
                        let _ = engine.quit().await;
                    }
                    return Err(e);
                }
            }
        }

        Ok(Self {
            engines,
            semaphore: Arc::new(Semaphore::new(config.pool_size)),
            next_engine: AtomicUsize::new(0),
            active_count: AtomicUsize::new(0),
        })
    }

    pub async fn acquire(&self) -> Result<PooledEngine<'_>> {
        let permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| Error::PoolExhausted)?;

        loop {
            let idx = self.next_engine.fetch_add(1, Ordering::SeqCst) % self.engines.len();
            let engine = &self.engines[idx];

            if engine.send_command("isready").await.is_err() {
                continue;
            }

            self.active_count.fetch_add(1, Ordering::SeqCst);

            return Ok(PooledEngine {
                engine: Arc::clone(engine),
                permit,
                pool: self,
            });
        }
    }

    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    pub fn size(&self) -> usize {
        self.engines.len()
    }

    pub fn active(&self) -> usize {
        self.active_count.load(Ordering::SeqCst)
    }

    pub async fn shutdown(&self) -> Result<()> {
        info!("shutting down engine pool");
        for engine in &self.engines {
            let _ = engine.quit().await;
        }
        Ok(())
    }
}

pub struct PooledEngine<'a> {
    engine: Arc<StockfishEngine>,
    #[allow(dead_code)]
    permit: SemaphorePermit<'a>,
    pool: &'a EnginePool,
}

impl<'a> PooledEngine<'a> {
    pub fn engine(&self) -> &StockfishEngine {
        &self.engine
    }
}

impl Drop for PooledEngine<'_> {
    fn drop(&mut self) {
        self.pool.active_count.fetch_sub(1, Ordering::SeqCst);
    }
}
