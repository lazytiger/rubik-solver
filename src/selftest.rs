//! 无人值守自检模式 —— 用于验证整条链路(求解 → 动画 → 渲染 → 截图)。
//!
//! ```bash
//! RUBIK_SELFTEST=/tmp/shot.png cargo run --release
//! ```
//! 流程:等求解器就绪 → 打乱 → 求解 → 播放前几步 → 截图 → 退出。
//! 退出码 0 表示"求解成功且解法 ≤20 步"。
//!
//! 这个模式让"≤20 步"这个承诺可以被自动化验证,而不只靠肉眼看。

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use cubr_core::model::Move;

use crate::anim::{is_solved, Playback};
use crate::cube::{sync_transforms, CubieIndex};
use crate::solve::{SolveStatus, SolverWorker};
use crate::{Model, SolutionView};

/// 到点截图并退出(用于验证特定场景的渲染)
#[derive(Resource)]
pub struct ShotAt {
    at: f32,
    t: f32,
    done: bool,
}

pub fn setup_shot_at(mut commands: Commands) {
    if let Ok(secs) = std::env::var("RUBIK_SHOT_AT") {
        if let Ok(at) = secs.parse::<f32>() {
            commands.insert_resource(ShotAt { at, t: 0.0, done: false });
        }
    }
}

pub fn shot_at_system(
    time: Res<Time>,
    mut s: ResMut<ShotAt>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if s.done {
        return;
    }
    s.t += time.delta_secs();
    if s.t >= s.at {
        let path = std::env::var("RUBIK_SHOT_PATH").unwrap_or_else(|_| "/tmp/shot.png".into());
        println!("[shot] {:.1}s 到点,截图 → {path}", s.t);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        s.done = true;
    } else if s.t > s.at + 2.0 {
        exit.write(AppExit::Success);
    }
}

#[derive(PartialEq, Debug)]
enum Phase {
    /// 等待求解器就绪(Korf 模式数据库首次约 15 秒)
    Boot,
    /// 一次性打乱
    Scramble,
    /// 请求求解
    Request,
    /// 播放解法前几步
    Play,
    /// 截图
    Shot,
    /// 等整段解法播完并验证终态
    Verify,
    /// 等截图落盘后退出
    Done,
    Failed(String),
}

#[derive(Resource)]
pub struct SelfTest {
    pub path: String,
    t: f32,
    phase_t: f32,
    phase: Phase,
}

impl SelfTest {
    pub fn new(path: String) -> Self {
        Self { path, t: 0.0, phase_t: 0.0, phase: Phase::Boot }
    }
}

/// 固定打乱,保证自检可复现
const SELFTEST_SCRAMBLE: [&str; 25] = [
    "L", "B'", "F2", "R", "F2", "L2", "R2", "D2", "U'", "B2", "U", "L", "F", "D2", "U'", "L2",
    "R2", "F'", "R", "D'", "R", "D2", "R", "F2", "B",
];

#[allow(clippy::too_many_arguments)]
pub fn selftest_system(
    time: Res<Time>,
    mut st: ResMut<SelfTest>,
    mut commands: Commands,
    mut model: ResMut<Model>,
    mut pb: ResMut<Playback>,
    view: Res<SolutionView>,
    mut worker: ResMut<SolverWorker>,
    mut q: Query<(&CubieIndex, &mut Transform)>,
    external: Res<crate::ExternalState>,
    mut exit: MessageWriter<AppExit>,
) {
    let dt = time.delta_secs();
    st.t += dt;
    st.phase_t += dt;

    let advance = |st: &mut SelfTest, phase: Phase| {
        st.phase = phase;
        st.phase_t = 0.0;
    };

    match &st.phase {
        // ① 等求解器就绪(最多等 120 秒)
        Phase::Boot => {
            if worker.is_ready() {
                advance(&mut st, Phase::Scramble);
            } else if st.t > 120.0 {
                advance(&mut st, Phase::Failed("求解器初始化超时".into()));
            }
        }
        // ② 打乱(直接落到模型上,不走动画,省时间)
        //    若外部已通过 --state / RUBIK_STATE 给定状态,则跳过打乱,直接求解它
        Phase::Scramble => {
            if external.0 {
                println!("[selftest] 使用外部给定的初始状态(跳过内置打乱)");
            } else {
                for tok in SELFTEST_SCRAMBLE {
                    model.core.apply(Move::parse(tok).expect("bad move"));
                }
                model.history.clear();
                sync_transforms(&model.core, &mut q);
                println!("[selftest] 打乱完成,已还原? {}", is_solved(&model.core.to_state()));
            }
            advance(&mut st, Phase::Request);
        }
        // ③ 请求求解
        Phase::Request => {
            if st.phase_t < 0.2 {
                return;
            }
            let state = model.core.to_state();
            if worker.request(state) {
                println!("[selftest] 已提交求解请求…");
                advance(&mut st, Phase::Play);
            } else if st.t > 150.0 {
                advance(&mut st, Phase::Failed("无法提交求解请求".into()));
            }
        }
        // ④ 解法到手 → 播放;等前 3 步演完再截图
        Phase::Play => {
            match &worker.status {
                SolveStatus::Done { steps, attempts, ms } => {
                    println!(
                        "[selftest] ✅ 解法 {steps} 步(尝试 {attempts} 次 / {ms} ms),开始播放…"
                    );
                    // 交互版已改为"求解后不自动播放";自检要自己放行
                    pb.paused = false;
                    if *steps > 20 {
                        advance(&mut st, Phase::Failed(format!("解法 {steps} 步,超过 20 步上限")));
                        return;
                    }
                    advance(&mut st, Phase::Shot); // 下面 Shot 阶段会先等动画
                }
                SolveStatus::Failed(e) => {
                    advance(&mut st, Phase::Failed(format!("求解失败: {e}")));
                }
                _ => {}
            }
        }
        // ⑤ 等动画播够 2.5 秒再截图(画面处于"转动中",便于肉眼确认动画在工作)
        Phase::Shot => {
            if st.phase_t > 2.5 {
                let path = st.path.clone();
                println!("[selftest] 📸 截图 → {path}");
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
                advance(&mut st, Phase::Verify);
            }
        }
        // ⑥ 等整段解法播完,验证终态确实被还原 —— 这才是"解法正确"的硬证据
        Phase::Verify => {
            let playing = pb.active.is_some() || !pb.queue.is_empty();
            if !playing {
                let state = model.core.to_state();
                if is_solved(&state) {
                    println!(
                        "[selftest] ✅ 全部 {} 步播放完毕,终态已还原(动画驱动的是真实模型)",
                        view.cursor
                    );
                    advance(&mut st, Phase::Done);
                } else {
                    advance(&mut st, Phase::Failed("播放结束后魔方未还原!".into()));
                }
            } else if st.phase_t > 60.0 {
                advance(&mut st, Phase::Failed("播放超时".into()));
            }
        }
        Phase::Done => {
            if st.phase_t > 2.0 {
                println!("[selftest] 完成,退出。");
                exit.write(AppExit::Success);
            }
        }
        Phase::Failed(why) => {
            eprintln!("[selftest] ❌ {why}");
            exit.write(AppExit::error());
        }
    }
}