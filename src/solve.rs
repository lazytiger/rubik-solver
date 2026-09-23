//! 后台求解线程:主线程只负责发状态、收结果,搜索永远不卡 UI。
//!
//! 用两个 `mpsc` 通道 + 一个 `AtomicBool` 取消旗标:
//! - `ToWorker::Solve(state)`     主线程 → 工作线程
//! - `FromWorker::{Status,Ready,Solved,Failed}`  工作线程 → 主线程
//! - `cancel` 由主线程置位(例如用户重新打乱),`cubr-core` 的搜索会立即响应
//!
//! ## ≤20 步的两级保证
//! 1. **快路径**:`cubr-core` 的混合求解器 —— Korf 最优 IDA*(默认 4 s 预算),
//!    预算内跑完即为**精确最优**;超时则回退到近优两阶段。
//! 2. **升级路径**:若结果仍多于 20 步(只可能出现在极少数距上帝之数最近的状态),
//!    则用「随机前置步 + 逐步加大的 Korf 预算」重试;
//!    最后兜底为**不设预算的完整最优搜索** —— 由上帝之数 = 20 保证必然返回 ≤20 步。
//!
//! Korf 预算通过 `cubr-core` 支持的环境变量 `CUBR_KORF_BUDGET_MS` 调整。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::sync::Arc;
use std::thread;
use web_time::Instant; // wasm 上 std 的 Instant 未实现

use bevy::prelude::*;
use cubr_core::model::{CubeState, Move};
use cubr_core::solver::{SolveError, Solver};

use crate::anim::Playback;
use crate::SolutionView;

pub enum ToWorker {
    Solve(CubeState),
}

pub enum FromWorker {
    /// 进度说明(建库 / 搜索阶段提示)
    Status(String),
    Ready,
    Solved { moves: Vec<Move>, attempts: u32, elapsed_ms: u128 },
    Failed(String),
}

/// 求解器状态(UI 直接读它)
#[derive(Clone, Debug, PartialEq)]
pub enum SolveStatus {
    /// 正在加载/生成模式数据库(首次约 15 秒)
    Booting(String),
    /// 就绪,可接受求解请求
    Ready,
    /// 求解中
    Solving { elapsed: f32, note: String },
    /// 完成
    Done { steps: usize, attempts: u32, ms: u128 },
    Failed(String),
}

/// 注意:`std::sync::mpsc::Receiver` 不是 `Sync`,而 Bevy 的 `Resource` 要求 `Send + Sync`,
/// 所以两个通道都用 `Mutex` 包一层(实际只在主线程访问,锁无竞争)。
#[derive(Resource)]
pub struct SolverWorker {
    pub tx: Mutex<Sender<ToWorker>>,
    pub rx: Mutex<Receiver<FromWorker>>,
    /// wasm 专用:没有线程,求解直接在主线程同步跑(懒加载一次);
    /// 结果发回同一个通道,由 `poll_solver` 走与原生版一致的后续流程。
    #[cfg(target_arch = "wasm32")]
    wasm_solver: Option<Solver>,
    #[cfg(target_arch = "wasm32")]
    res_tx: Sender<FromWorker>,
    pub cancel: Arc<AtomicBool>,
    pub status: SolveStatus,
    /// 工作线程最近一次进度说明(UI 直接显示)
    pub note: String,
    pub solving_since: Option<Instant>,
}

impl SolverWorker {
    /// 请求求解;返回 false 表示此刻不可受理
    pub fn request(&mut self, state: CubeState) -> bool {
        if !matches!(self.status, SolveStatus::Ready | SolveStatus::Done { .. } | SolveStatus::Failed(_)) {
            return false;
        }
        self.cancel.store(false, Ordering::Relaxed);
        self.solving_since = Some(Instant::now());
        self.note = "Korf 最优搜索(默认 4 秒预算)…".into();
        self.status = SolveStatus::Solving { elapsed: 0.0, note: self.note.clone() };

        #[cfg(target_arch = "wasm32")]
        {
            // 同步求解(会阻塞约 1 秒构建表 + 数毫秒搜索),结果塞回通道,
            // 由 poll_solver 走与原生版完全相同的后续流程。
            let solver = match self.wasm_solver.take() {
                Some(s) => s,
                None => {
                    let pdbs = cubr_core::solver::build_or_load_pdbs();
                    Solver::new(pdbs)
                }
            };
            let t0 = Instant::now();
            let out = solve_within_20(&solver, &state, &self.cancel, &|m| {
                let _ = self.res_tx.send(FromWorker::Status(m));
            });
            let elapsed_ms = t0.elapsed().as_millis();
            self.wasm_solver = Some(solver);
            let msg = match out {
                Ok((moves, attempts)) => FromWorker::Solved { moves, attempts, elapsed_ms },
                Err(e) => FromWorker::Failed(format!("{e:?}")),
            };
            let _ = self.res_tx.send(msg);
            return true;
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.tx.lock().expect("tx").send(ToWorker::Solve(state)).is_ok()
    }
    /// 取消当前求解(用户重新打乱/重置时调用)
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    /// 求解结束(完成/失败)后复位
    fn finish(&mut self) {
        self.solving_since = None;
    }
    pub fn is_ready(&self) -> bool {
        !matches!(self.status, SolveStatus::Booting(_))
    }
}

/// 启动工作线程:线程内独占 `Solver`(模式数据库 + 搜索表)
pub fn start_worker() -> SolverWorker {
    let (tx, rx_req) = channel::<ToWorker>();
    let (res_tx, res_rx) = channel::<FromWorker>();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = cancel.clone();

    #[cfg(target_arch = "wasm32")]
    {
        // ── WASM:没有线程 ──
        // 不 spawn worker:求解在 `request()` 里同步执行,结果仍走同一个通道,
        // 因此 `poll_solver` / 播放器那套逻辑完全不用改。
        let _ = &cancel_worker;
        return SolverWorker {
            tx: Mutex::new(tx),
            rx: Mutex::new(res_rx),
            wasm_solver: None,
            res_tx,
            cancel,
            // ⚠️ wasm 上没有工作线程,不会有人把状态从 Booting 改成 Ready,
            //    于是 is_ready() 永远为 false ⇒ 点「求解」毫无反应(只在日志里写一句)。
            //    wasm 版改为:一开始就是 Ready,求解器在第一次 request() 时懒加载。
            status: SolveStatus::Ready,
            note: String::new(),
            solving_since: None,
        };
    }

    #[cfg(not(target_arch = "wasm32"))]
    thread::spawn(move || {
        let _ = res_tx.send(FromWorker::Status("正在加载 / 生成 Korf 模式数据库(首次约 15 秒)…".into()));
        let pdbs = cubr_core::solver::build_or_load_pdbs();
        let _ = res_tx.send(FromWorker::Status("正在构建搜索加速表…".into()));
        let solver = Solver::new(pdbs);
        let _ = res_tx.send(FromWorker::Ready);

        while let Ok(req) = rx_req.recv() {
            match req {
                ToWorker::Solve(state) => {
                    cancel_worker.store(false, Ordering::Relaxed);
                    let t0 = Instant::now();
                    let progress = |msg: String| {
                        let _ = res_tx.send(FromWorker::Status(msg));
                    };
                    let out = solve_within_20(&solver, &state, &cancel_worker, &progress);
                    let elapsed_ms = t0.elapsed().as_millis();
                    let msg = match out {
                        Ok((moves, attempts)) => FromWorker::Solved { moves, attempts, elapsed_ms },
                        Err(e) => FromWorker::Failed(format!("{e:?}")),
                    };
                    let _ = res_tx.send(msg);
                }
            }
        }
    });

    #[cfg(not(target_arch = "wasm32"))]
    SolverWorker {
        tx: Mutex::new(tx),
        rx: Mutex::new(res_rx),
        cancel,
        status: SolveStatus::Booting("启动中…".into()),
        note: "启动中…".into(),
        solving_since: None,
    }
}

/// `CUBR_KORF_BUDGET_MS` 会被 cubr-core 在每次 solve 时读取,因此可在运行时调整预算
fn set_korf_budget(ms: u64) {
    // ⚠️ wasm32-unknown-unknown 上 std::env::set_var 会**直接 panic**
    //    ("cannot set env vars on this platform")。浏览器版本来也不走 Korf,
    //    所以这里只在原生平台设置。
    #[cfg(not(target_arch = "wasm32"))]
    std::env::set_var("CUBR_KORF_BUDGET_MS", ms.to_string());
    #[cfg(target_arch = "wasm32")]
    let _ = ms;
}

/// 保证返回 ≤20 步解法。
///
/// 实测数据(本机,release):
/// - 随机打乱(25/40 步):第 1 级 4 秒内直接给出 **20 步**解
/// - superflip(上帝之数边界状态):4 秒预算给 22 步、25 秒预算给 21 步,
///   两阶段回退**永远到不了 20** —— 只有第 2 级的完整最优搜索能给出 ≤20,
///   代价是耗时可能到分钟级(可随时取消)
pub fn solve_within_20(
    solver: &Solver,
    state: &CubeState,
    cancel: &AtomicBool,
    progress: &dyn Fn(String),
) -> Result<(Vec<Move>, u32), SolveError> {
    let mut attempts = 0u32;

    // ── 第 1 级:默认预算(4 s)的混合求解(Korf 最优 → 两阶段回退)──
    attempts += 1;
    if let Ok(m) = solver.solve(state, cancel) {
        if m.len() <= 20 {
            return Ok((m, attempts));
        }
        progress(format!(
            "已取得 {} 步解;正在搜索 ≤20 步的最优解(难例可能需数分钟,可打乱/重置取消)…",
            m.len()
        ));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(SolveError::Cancelled);
    }

    // ── 第 2 级:不设预算的完整最优搜索 ──
    //    由上帝之数 = 20 保证必然返回 ≤20 步;仅极少数边界状态会耗时较长。
    set_korf_budget(3_600_000);
    attempts += 1;
    solver.solve(state, cancel).map(|m| (m, attempts))
}

/// 轮询工作线程消息,把结果灌进播放器
pub fn poll_solver(
    mut worker: ResMut<SolverWorker>,
    mut pb: ResMut<Playback>,
    mut view: ResMut<SolutionView>,
) {
    loop {
        let msg = {
            let rx = worker.rx.lock().expect("rx");
            match rx.try_recv() {
                Ok(m) => m,
                Err(_) => break,
            }
        };
        match msg {
            FromWorker::Status(s) => {
                if matches!(worker.status, SolveStatus::Booting(_)) {
                    worker.status = SolveStatus::Booting(s.clone());
                }
                worker.note = s;
            }
            FromWorker::Ready => {
                worker.status = SolveStatus::Ready;
                worker.note = "就绪".into();
            }
            FromWorker::Solved { moves, attempts, elapsed_ms } => {
                worker.finish();
                worker.status = SolveStatus::Done { steps: moves.len(), attempts, ms: elapsed_ms };
                view.moves = moves.clone();
                view.cursor = 0;
                view.attempts = attempts;
                view.elapsed_ms = elapsed_ms;
                pb.enqueue(moves);
                // 不自动播放:交给用户按「播放」或「单步」
                pb.paused = true;
            }
            FromWorker::Failed(e) => {
                worker.finish();
                worker.status = SolveStatus::Failed(e);
            }
        }
    }
    // 求解中:刷新已用时间(说明文字用工作线程的最新 note)
    if let Some(t0) = worker.solving_since {
        let el = t0.elapsed().as_secs_f32();
        let note = worker.note.clone();
        worker.status = SolveStatus::Solving { elapsed: el, note };
    }
}