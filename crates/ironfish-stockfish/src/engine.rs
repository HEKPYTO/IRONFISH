use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{debug, trace};
use ironfish_core::{Error, Result};
pub struct StockfishEngine {
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<BufReader<ChildStdout>>>,
    ready: AtomicBool,
    _process: Arc<Mutex<Child>>,
    binary_path: String,
}
impl StockfishEngine {
    pub async fn new(binary_path: &str) -> Result<Self> {
        let mut process = Command::new(binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::Engine(format!("failed to spawn stockfish: {}", e)))?;
        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| Error::Engine("failed to get stdin".into()))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| Error::Engine("failed to get stdout".into()))?;
        let engine = Self {
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            ready: AtomicBool::new(false),
            _process: Arc::new(Mutex::new(process)),
            binary_path: binary_path.to_string(),
        };
        engine.initialize().await?;
        Ok(engine)
    }
    pub async fn restart(&self) -> Result<()> {
        debug!("restarting stockfish engine");
        let mut process = Command::new(&self.binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::Engine(format!("failed to spawn stockfish: {}", e)))?;
        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| Error::Engine("failed to get stdin".into()))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| Error::Engine("failed to get stdout".into()))?;
        {
            let mut p_guard = self._process.lock().await;
            *p_guard = process;
        }
        {
            let mut in_guard = self.stdin.lock().await;
            *in_guard = stdin;
        }
        {
            let mut out_guard = self.stdout.lock().await;
            *out_guard = BufReader::new(stdout);
        }
        self.ready.store(false, Ordering::SeqCst);
        self.initialize().await?;
        Ok(())
    }
    async fn initialize(&self) -> Result<()> {
        self.send_command("uci").await?;
        self.wait_for("uciok").await?;
        self.send_command("isready").await?;
        self.wait_for("readyok").await?;
        self.ready.store(true, Ordering::SeqCst);
        debug!("stockfish engine initialized");
        Ok(())
    }
    pub async fn send_command(&self, cmd: &str) -> Result<()> {
        trace!("sending command: {}", cmd);
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(format!("{}\n", cmd).as_bytes())
            .await
            .map_err(|e| Error::Engine(format!("write failed: {}", e)))?;
        stdin
            .flush()
            .await
            .map_err(|e| Error::Engine(format!("flush failed: {}", e)))?;
        Ok(())
    }
    pub async fn read_line(&self) -> Result<String> {
        let mut stdout = self.stdout.lock().await;
        let mut line = String::new();
        stdout
            .read_line(&mut line)
            .await
            .map_err(|e| Error::Engine(format!("read failed: {}", e)))?;
        trace!("received: {}", line.trim());
        Ok(line)
    }
    async fn wait_for(&self, expected: &str) -> Result<()> {
        loop {
            let line = self.read_line().await?;
            if line.trim().starts_with(expected) {
                return Ok(());
            }
        }
    }
    pub async fn set_position(&self, fen: &str) -> Result<()> {
        let cmd = if fen == "startpos" {
            "position startpos".to_string()
        } else {
            format!("position fen {}", fen)
        };
        self.send_command(&cmd).await
    }
    pub async fn go_depth(&self, depth: u8) -> Result<()> {
        self.send_command(&format!("go depth {}", depth)).await
    }
    pub async fn go_movetime(&self, ms: u64) -> Result<()> {
        self.send_command(&format!("go movetime {}", ms)).await
    }
    pub async fn set_multipv(&self, n: u8) -> Result<()> {
        self.send_command(&format!("setoption name MultiPV value {}", n))
            .await
    }
    pub async fn stop(&self) -> Result<()> {
        self.send_command("stop").await
    }
    pub async fn quit(&self) -> Result<()> {
        self.send_command("quit").await
    }
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
    pub async fn ensure_ready(&self) -> Result<()> {
        self.send_command("isready").await?;
        self.wait_for("readyok").await
    }
    pub async fn is_running(&self) -> bool {
        let mut process = self._process.lock().await;
        match process.try_wait() {
            Ok(Some(_)) => false,  
            Ok(None) => true,      
            Err(_) => false,       
        }
    }
}
impl Drop for StockfishEngine {
    fn drop(&mut self) {
        debug!("dropping stockfish engine");
    }
}
#[derive(Debug, Clone, Default)]
pub struct UciInfo {
    pub depth: Option<u8>,
    pub seldepth: Option<u8>,
    pub multipv: Option<u8>,
    pub score_cp: Option<i32>,
    pub score_mate: Option<i32>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    pub time: Option<u64>,
    pub pv: Vec<String>,
    pub currmove: Option<String>,
    pub hashfull: Option<u16>,
}
impl UciInfo {
    pub fn parse(line: &str) -> Option<Self> {
        let mut parts = line.split_whitespace();
        if parts.next() != Some("info") {
            return None;
        }
        let mut info = Self::default();
        while let Some(token) = parts.next() {
            match token {
                "depth" => info.depth = parts.next().and_then(|s| s.parse().ok()),
                "seldepth" => info.seldepth = parts.next().and_then(|s| s.parse().ok()),
                "multipv" => info.multipv = parts.next().and_then(|s| s.parse().ok()),
                "score" => match parts.next() {
                    Some("cp") => info.score_cp = parts.next().and_then(|s| s.parse().ok()),
                    Some("mate") => info.score_mate = parts.next().and_then(|s| s.parse().ok()),
                    _ => {}
                },
                "nodes" => info.nodes = parts.next().and_then(|s| s.parse().ok()),
                "nps" => info.nps = parts.next().and_then(|s| s.parse().ok()),
                "time" => info.time = parts.next().and_then(|s| s.parse().ok()),
                "hashfull" => info.hashfull = parts.next().and_then(|s| s.parse().ok()),
                "currmove" => info.currmove = parts.next().map(|s| s.to_string()),
                "pv" => {
                    for m in parts {
                        info.pv.push(m.to_string());
                    }
                    break;
                }
                _ => {}
            }
        }
        Some(info)
    }
}
#[derive(Debug, Clone)]
pub struct BestMove {
    pub mv: String,
    pub ponder: Option<String>,
}
impl BestMove {
    pub fn parse(line: &str) -> Option<Self> {
        if !line.starts_with("bestmove") {
            return None;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let mv = parts.get(1)?.to_string();
        let ponder = parts
            .iter()
            .position(|&s| s == "ponder")
            .and_then(|i| parts.get(i + 1))
            .map(|s| s.to_string());
        Some(Self { mv, ponder })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_uci_info_parse_depth() {
        let line = "info depth 20 seldepth 25 nodes 1000000 nps 500000";
        let info = UciInfo::parse(line).unwrap();
        assert_eq!(info.depth, Some(20));
        assert_eq!(info.seldepth, Some(25));
        assert_eq!(info.nodes, Some(1000000));
        assert_eq!(info.nps, Some(500000));
    }
    #[test]
    fn test_uci_info_parse_score_cp() {
        let line = "info depth 10 score cp 150 nodes 50000";
        let info = UciInfo::parse(line).unwrap();
        assert_eq!(info.depth, Some(10));
        assert_eq!(info.score_cp, Some(150));
        assert_eq!(info.score_mate, None);
    }
    #[test]
    fn test_uci_info_parse_score_mate() {
        let line = "info depth 15 score mate 3 nodes 100000";
        let info = UciInfo::parse(line).unwrap();
        assert_eq!(info.score_mate, Some(3));
        assert_eq!(info.score_cp, None);
    }
    #[test]
    fn test_uci_info_parse_pv() {
        let line = "info depth 10 pv e2e4 e7e5 g1f3";
        let info = UciInfo::parse(line).unwrap();
        assert_eq!(info.pv, vec!["e2e4", "e7e5", "g1f3"]);
    }
    #[test]
    fn test_uci_info_parse_multipv() {
        let line = "info depth 10 multipv 2 score cp 50 pv d2d4";
        let info = UciInfo::parse(line).unwrap();
        assert_eq!(info.multipv, Some(2));
    }
    #[test]
    fn test_uci_info_not_info_line() {
        let line = "bestmove e2e4";
        assert!(UciInfo::parse(line).is_none());
        let line = "readyok";
        assert!(UciInfo::parse(line).is_none());
    }
    #[test]
    fn test_bestmove_parse() {
        let line = "bestmove e2e4";
        let bm = BestMove::parse(line).unwrap();
        assert_eq!(bm.mv, "e2e4");
        assert_eq!(bm.ponder, None);
    }
    #[test]
    fn test_bestmove_parse_with_ponder() {
        let line = "bestmove e2e4 ponder e7e5";
        let bm = BestMove::parse(line).unwrap();
        assert_eq!(bm.mv, "e2e4");
        assert_eq!(bm.ponder, Some("e7e5".to_string()));
    }
    #[test]
    fn test_bestmove_not_bestmove_line() {
        let line = "info depth 10";
        assert!(BestMove::parse(line).is_none());
        let line = "uciok";
        assert!(BestMove::parse(line).is_none());
    }
    #[test]
    fn test_uci_info_hashfull() {
        let line = "info depth 10 hashfull 500";
        let info = UciInfo::parse(line).unwrap();
        assert_eq!(info.hashfull, Some(500));
    }
    #[test]
    fn test_uci_info_time() {
        let line = "info depth 10 time 1500";
        let info = UciInfo::parse(line).unwrap();
        assert_eq!(info.time, Some(1500));
    }
}
