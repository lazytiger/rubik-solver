//! 转动动画:把一次 `Move` 演成"某一层绕轴转 90°×n"的动画,并支持暂停/单步/回退。
//!
//! ## 旋转方向怎么来的(不猜约定)
//! 不依赖"顺时针 = 正号"这类手性约定,而是**从模型本身推导**:
//! 对受影响的方块,取转动前的朝向矩阵 `R0` 与转动后的 `R1`,
//! 增量旋转就是 `Δ = R1 · R0ᵀ`。它的轴角即这次转动真正的轴与方向。
//! 这样无论 cubr-core 内部用哪种约定,动画都与逻辑模型严格一致。
//!
//! ## 一帧的变换
//! `translation(t) = Δ(t) · t0`,`rotation(t) = Δ(t) · r0`,
//! 其中 `Δ(t) = Quat::from_axis_angle(axis, angle · ease(t))`。
//! 动画结束时由 [`sync_transforms`] 从逻辑模型整体重算 —— 逻辑是权威,视觉不漂移。

use bevy::prelude::*;
use cubr_core::model::{CubeState, Move};

use crate::cube::{cubie_rotation, cubie_transform, sync_transforms, v3i, CubieIndex};
use crate::{Model, SolutionView};

/// 单步动画默认时长(秒),可用 `[` / `]` 调节
pub const DEFAULT_DURATION: f32 = 0.32;
pub const MIN_DURATION: f32 = 0.08;
pub const MAX_DURATION: f32 = 2.0;

/// 正在播放的一次转动
pub struct ActiveMove {
    pub mv: Move,
    pub elapsed: f32,
    pub duration: f32,
    /// 受影响的方块:(实体, 动画起始平移, 动画起始旋转)
    pub affected: Vec<(Entity, Vec3, Quat)>,
    /// 旋转轴(单位向量,方向由模型推导)
    pub axis: Vec3,
    /// 总旋转角(弧度,含方向)
    pub angle: f32,
    /// 当前已转过的角度(度,带符号)—— 供 HUD 显示"已转多少度"
    pub turned_deg: f32,
    /// 单步操作:播完自动回到暂停
    pub single_step: bool,
    /// 回退操作:播完不写历史(目前仅用于日志/HUD 语义,保留以便扩展)
    #[allow(dead_code)]
    pub undo: bool,
}

/// 播放器状态机
#[derive(Resource)]
pub struct Playback {
    /// 待播放的转动队列(解法剩余部分)
    pub queue: Vec<Move>,
    /// 当前正在播放的转动
    pub active: Option<ActiveMove>,
    /// 是否暂停(暂停时仍可单步)
    pub paused: bool,
    /// 单步动画时长
    pub duration: f32,
    /// 暂停状态下允许播放的步数(按一次"单步"就 +1)
    pub step_budget: u32,
    /// 已完成步数(统计)
    pub total_played: u32,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            queue: Vec::new(),
            active: None,
            paused: false,
            duration: DEFAULT_DURATION,
            step_budget: 0,
            total_played: 0,
        }
    }
}

impl Playback {
    /// 是否应该开始下一转(暂停时,只有"单步"预算可以放行)
    pub fn ready_to_start(&self) -> bool {
        self.active.is_none() && !self.queue.is_empty() && (!self.paused || self.step_budget > 0)
    }
    /// 排入一串转动(清空当前队列)
    pub fn enqueue(&mut self, moves: Vec<Move>) {
        self.queue = moves;
        self.active = None;
    }
    #[allow(dead_code)]
    pub fn is_busy(&self) -> bool {
        self.active.is_some() || !self.queue.is_empty()
    }
    /// 剩余待播步数
    pub fn remaining(&self) -> usize {
        self.queue.len() + if self.active.is_some() { 1 } else { 0 }
    }
}

/// 由两个朝向矩阵求增量旋转的轴与角:`Δ = R1 · R0ᵀ`
///
/// ⚠️ 必须传**同一个模型**在"这一转之前 / 之后"的朝向。
/// 早期版本用一个 `paint()` 出来的副本当"之后",而 `paint()` 会把所有块朝向重置为
/// 单位矩阵 → 推出的 Δ 变成 `R_move · R_真实⁻¹`(可能是体对角线轴),动画就会把
/// 内部翻出来。见 [`crate::diag`] 的回归测试。
pub fn delta_axis_angle(r0: Mat3, r1: Mat3) -> (Vec3, f32) {
    let d = r1 * r0.transpose();
    let (axis, angle) = Quat::from_mat3(&d).to_axis_angle();
    (axis.normalize_or_zero(), angle)
}

/// 是否轴对齐(单分量 ±1,其余 ≈0)—— 单层转动必须满足
pub fn axis_aligned(v: Vec3) -> bool {
    let c = [v.x.abs(), v.y.abs(), v.z.abs()];
    c.iter().filter(|&&x| x > 0.999).count() == 1 && c.iter().filter(|&&x| x < 1e-3).count() == 2
}

/// **推导一次转动的轴与角**,并把 `core` 推进到转动之后。
///
/// 这是动画与自检共用的唯一实现:
/// 1. 挑一个该层里的非中心方块(中心块朝向不变,推不出旋转);
/// 2. 记下它**转动前**的朝向矩阵;
/// 3. `core.apply(mv)` 真正推进模型;
/// 4. 读它**转动后**的朝向矩阵,`Δ = R_after · R_beforeᵀ` 的轴角就是这一转。
///
/// ⚠️ 关键是 2 与 4 必须在**同一份模型**上取。早期版本用一个 `paint()` 出来的副本
/// 当"转动后",而 `paint()` 会把所有朝向重置为单位矩阵,于是 Δ 变成
/// `R_move · R_真实⁻¹` —— 可能是体对角线轴,动画就会把方块甩出立方体、露出内部。
pub fn derive_axis_angle(core: &mut cubr_core::core::CubeCore, mv: Move) -> (Vec3, f32) {
    let probe = core
        .layer(mv)
        .into_iter()
        .find(|&i| core.cubies()[i].pos.length_squared() > 1);
    let before = probe.map(|i| cubie_rotation(&core.cubies()[i]));
    core.apply(mv);
    match (probe, before) {
        (Some(i), Some(b)) => {
            let (ax, an) = delta_axis_angle(b, cubie_rotation(&core.cubies()[i]));
            if axis_aligned(ax) {
                (ax, an)
            } else {
                fallback_axis_angle(mv) // 防御:绝不绕斜轴转
            }
        }
        _ => fallback_axis_angle(mv),
    }
}

/// 兜底的轴角:面法线方向 + 90°×n(不会翻出内部,只是方向约定可能不同)
fn fallback_axis_angle(mv: Move) -> (Vec3, f32) {
    let a = mv.axis();
    let a = v3i(a.x, a.y, a.z);
    (
        a.normalize_or_zero(),
        std::f32::consts::FRAC_PI_2 * mv.quarter_turns_cw() as f32,
    )
}

/// 开始播放队列里的下一转
pub fn start_next_move(
    mut model: ResMut<Model>,
    mut pb: ResMut<Playback>,
    mut view: ResMut<SolutionView>,
    q: Query<(Entity, &CubieIndex)>,
) {
    if !pb.ready_to_start() {
        return;
    }
    let mv = pb.queue.remove(0);
    // 暂停状态下的放行 = 单步:播完自动回到暂停
    let single = pb.paused && pb.step_budget > 0;
    if single {
        pb.step_budget -= 1;
    }
    let affected = snapshot_layer(&model, &q, mv);
    begin_move(&mut model, &mut pb, &mut view, affected, mv, false, single);
}

/// 快照某一次转动会影响的方块(实体 + 起始变换)
fn snapshot_layer(
    model: &Model,
    q: &Query<(Entity, &CubieIndex)>,
    mv: Move,
) -> Vec<(Entity, Vec3, Quat)> {
    let idxs = model.core.layer(mv);
    let mut out = Vec::new();
    for (e, idx) in q.iter() {
        if idxs.contains(&idx.0) {
            if let Some(c) = model.core.cubies().get(idx.0) {
                let tf = cubie_transform(c);
                out.push((e, tf.translation, tf.rotation));
            }
        }
    }
    out
}

/// 内部:开始一次转动动画(正常播放 / 回退逆转动 都走这里)
///
/// - `is_undo = true` 时不写历史(那一步已在 `undo_last` 里弹出)
/// - 动画与 `paused` 无关:`advance_animation` 只看 `active`,**暂停中也能回退**
fn begin_move(
    model: &mut Model,
    pb: &mut Playback,
    view: &mut SolutionView,
    affected: Vec<(Entity, Vec3, Quat)>,
    mv: Move,
    is_undo: bool,
    single: bool,
) {

    // 受影响的方块快照由调用方准备好传入

    // 2) 轴角推导 + 逻辑模型推进(同一份模型上取"转动前 / 转动后")
    let (axis, angle) = derive_axis_angle(&mut model.core, mv);
    if is_undo {
        // history 已在 undo_last 里弹出
    } else {
        model.history.push(mv);
    }
    let _ = &view; // 进度在"播放完成"时计数(见 advance_animation),这里不计数

    pb.total_played += 1;
    pb.active = Some(ActiveMove {
        mv,
        elapsed: 0.0,
        duration: pb.duration,
        affected,
        axis,
        angle,
        turned_deg: 0.0,
        single_step: single,
        undo: is_undo,
    });
}

/// 每帧推进动画;结束后校正回模型
pub fn advance_animation(
    time: Res<Time>,
    mut pb: ResMut<Playback>,
    mut view: ResMut<SolutionView>,
    model: Res<Model>,
    mut q: Query<(&CubieIndex, &mut Transform)>,
) {
    // 先取出时长,避免与 pb.active 的可变借用冲突
    let dur = pb.duration;
    let Some(active) = pb.active.as_mut() else { return };

    active.duration = dur; // 允许播放中实时调速
    active.elapsed += time.delta_secs();
    let t = (active.elapsed / active.duration).clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t); // smoothstep 缓动

    active.turned_deg = active.angle.to_degrees() * eased;
    let delta = Quat::from_axis_angle(active.axis, active.angle * eased);
    for (entity, t0, r0) in &active.affected {
        if let Ok((_, mut tf)) = q.get_mut(*entity) {
            tf.translation = delta * *t0;
            tf.rotation = delta * *r0;
        }
    }

    if t >= 1.0 {
        let was_single = active.single_step;
        let was_undo = active.undo;

        // 防御性自检:动画插值的终点必须等于"模型当前"的位置。
        // 轴推导一旦出问题(例如推出体对角线轴),这里会立刻发现。
        if std::env::var("RUBIK_DEBUG").is_ok() {
            let d = Quat::from_axis_angle(active.axis, active.angle);
            for (entity, t0, _) in &active.affected {
                if let Ok((idx, _)) = q.get(*entity) {
                    if let Some(c) = model.core.cubies().get(idx.0) {
                        let got = d * *t0;
                        if got.distance(cubie_transform(c).translation) > 0.01 {
                            warn!(
                                "[anim] 终点不一致:{} 实得 {:?}(轴 {:?} 角 {:.0}°)",
                                active.mv.notation(), got, active.axis, active.angle.to_degrees()
                            );
                        }
                    }
                }
            }
        }

        pb.active = None;
        if was_single {
            pb.paused = true; // 单步结束 → 回到暂停
        }
        // 播放完成才计数:HUD 里 [ ] 标记的正是"当前正在播/下一个要播"的那步
        if !was_undo {
            view.cursor = (view.cursor + 1).min(view.moves.len().max(view.cursor + 1));
        }
        sync_transforms(&model.core, &mut q);
    }
}

/// 回退一步 —— **任何时刻都能用**:
///
/// 1. 若有动画正在播,先把它**瞬间播完**(模型在开始时就已经推进过了,只需补上计数与摆位)
/// 2. 从历史里弹出最后一步,并把它**放回队列头** ⇒ 之后按「播放」就能再重做,
///    所以回退/重做是对称的,不会"退一步就再也前进不了"
/// 3. 立刻播放这一步的**逆转动**(暂停中也会播 —— 这是对按钮的直接响应)
///
/// `cursor` 在原始那一步完成时加过 1,这里减回去。
pub fn undo_last(
    model: &mut Model,
    pb: &mut Playback,
    view: &mut SolutionView,
    q: &mut Query<(Entity, &CubieIndex, &mut Transform)>,
) {
    // 1) 把在播的那一步瞬间收尾
    if let Some(a) = pb.active.take() {
        if !a.undo {
            view.cursor = (view.cursor + 1).min(view.moves.len().max(view.cursor + 1));
        }
        if a.single_step {
            pb.paused = true;
        }
        crate::cube::sync_transforms3(&model.core, q);
    }

    // 2) 弹出最后一步,并放回队列(供重做)
    let Some(m) = model.history.pop() else { return };
    pb.queue.insert(0, m);

    // 3) 立刻播它的逆转动(不写历史)
    let inv = m.inverse();
    let idxs = model.core.layer(inv);
    let mut affected: Vec<(Entity, Vec3, Quat)> = Vec::new();
    for (e, idx, _tf) in q.iter() {
        if idxs.contains(&idx.0) {
            if let Some(c) = model.core.cubies().get(idx.0) {
                let t = cubie_transform(c);
                affected.push((e, t.translation, t.rotation));
            }
        }
    }
    begin_move(model, pb, view, affected, inv, true, false);
    view.cursor = view.cursor.saturating_sub(1);
}

/// 当前局面描述(供 UI)
pub fn is_solved(state: &CubeState) -> bool {
    let s = CubeState::solved();
    state.U == s.U && state.R == s.R && state.F == s.F
        && state.D == s.D && state.L == s.L && state.B == s.B
}