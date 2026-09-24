//! 初始状态输入 —— 可点击的魔方展开图编辑器(**带合法性保证**)。
//!
//! ## 三条输入通道
//! 1. **图形化**:右侧展开图点格子涂色,中心块锁定,局部约束自动传播
//! 2. **命令行**:`--state <54 字符>`
//! 3. **环境变量**:`RUBIK_STATE=<54 字符>`
//!
//! ## 合法性是怎么保证的
//! 见 [`crate::validate`] 的模块文档(5 条充要不变量)。这里做三件事:
//! - **涂色时即时拒绝**:中心块 / 对面色同块 / 颜色超 9 格
//! - **涂色后自动推导**:角块已知 2 色 ⇒ 第 3 色确定;中心恒为各面标准色
//! - **每次修改后完整校验**:不满足不变量时状态行给出精确原因,`Apply` 被拒
//!
//! 展开图布局(标准十字):
//! ```text
//!         U
//!     L   F   R   B
//!         D
//! ```

use bevy::prelude::*;
use cubr_core::model::StickerColor;

use crate::anim::Playback;
use crate::cube::{sticker_color, CubieIndex};
use crate::validate::{self, Cells, CENTER_IDX, COLOR_ORDER, FACE_COLOR};
use crate::{Model, SolutionView};

#[allow(dead_code)] // 展开图编辑器已按需求移除(改为直接在魔方上操作)
const CELL: f32 = 22.0;
#[allow(dead_code)] // 展开图编辑器已按需求移除(改为直接在魔方上操作)
const INNER_GAP: f32 = 2.0;
#[allow(dead_code)] // 展开图编辑器已按需求移除(改为直接在魔方上操作)
const FACE: f32 = CELL * 3.0 + INNER_GAP * 2.0; // 70
#[allow(dead_code)] // 展开图编辑器已按需求移除(改为直接在魔方上操作)
const FACE_GAP: f32 = 4.0;
#[allow(dead_code)] // 展开图编辑器已按需求移除(改为直接在魔方上操作)
const FACE_PITCH: f32 = FACE + FACE_GAP; // 74
#[allow(dead_code)] // 展开图编辑器已按需求移除(改为直接在魔方上操作)
const PAD: f32 = 10.0;

/// 六个面在展开图中的 (列, 行) —— 顺序 U R F D L B
#[allow(dead_code)] // 展开图编辑器已按需求移除(改为直接在魔方上操作)
const FACE_POS: [(usize, usize); 6] = [(1, 0), (2, 1), (1, 1), (1, 2), (0, 1), (3, 1)];
#[allow(dead_code)] // 展开图编辑器已按需求移除(改为直接在魔方上操作)
const NET_W: f32 = FACE_PITCH * 4.0 - FACE_GAP;
#[allow(dead_code)] // 展开图编辑器已按需求移除(改为直接在魔方上操作)
const NET_H: f32 = FACE_PITCH * 3.0 - FACE_GAP;

/// 未填格子的底色
const UNKNOWN_BG: Color = Color::srgb(0.16, 0.16, 0.19);

#[derive(Component)]
pub struct EditorRoot;
#[derive(Component)]
pub struct FaceletCell {
    pub face: usize,
    pub idx: usize,
}
#[derive(Component)]
pub struct PaletteSwatch(pub StickerColor);
#[derive(Component)]
pub struct ApplyButton;
#[derive(Component)]
pub struct FromCubeButton;
#[derive(Component)]
pub struct ClearButton;
#[derive(Component)]
pub struct SolvedButton;
#[derive(Component)]
pub struct EraserButton;
#[derive(Component)]
pub struct RepairButton;
#[derive(Component)]
pub struct EditorStatus;

/// 编辑器状态
#[derive(Resource)]
pub struct EditState {
    /// 盘面:`None` = 未填
    pub cells: Cells,
    /// 该格是否由推导填出(而非手涂)
    pub derived: [[bool; 9]; 6],
    /// 调色板当前颜色
    pub active: StickerColor,
    /// 橡皮擦模式(点格子 = 清除,便于"交换"两个贴纸)
    pub erasing: bool,
    pub visible: bool,
    /// 需要把 `cells` 刷回 UI
    pub dirty: bool,
    /// 操作提示 / 校验结果
    pub message: String,
    /// 完整校验是否通过
    pub valid: bool,
    /// 处于"矛盾"状态的格子(没有合法块可放)—— 标红提示
    pub error_cells: Vec<usize>,
    /// 当前选中颜色的**可填位置**(会加闪烁绿框)
    pub placeable: Vec<usize>,
    /// 正在用六边形取色器编辑的格子(None = 未打开取色器)
    pub picker: Option<usize>,
    /// 取色器"已选好颜色、等指针抬起再关"(见 picker.rs::finish_close)。
    ///
    /// 为什么不能选完立刻关:指针通常还按着,遮罩一消失,Bevy 的 UI 命中测试
    /// 会把这次**仍然按住的指针**重新算到底下的格子上 ⇒ 同一笔输入穿透成
    /// "又选中了下一个格子"。让遮罩活到指针抬起,底下的格子就永远拿不到 Pressed。
    pub picker_closing: bool,
}

impl Default for EditState {
    fn default() -> Self {
        let mut st = Self {
            cells: [[None; 9]; 6],
            derived: [[false; 9]; 6],
            active: StickerColor::W,
            erasing: false,
            visible: true,
            dirty: true,
            message: String::new(),
            valid: false,
            error_cells: Vec::new(),
            placeable: Vec::new(),
            picker: None,
            picker_closing: false,
        };
        // 默认盘面 = 还原态(全部已知)
        st.from_cube(&cubr_core::model::CubeState::solved());
        st.message = "点格子涂色;金色=中心(不可改),青色=自动推导".into();
        st
    }
}

impl EditState {
    pub fn from_cube(&mut self, s: &cubr_core::model::CubeState) {
        self.cells = validate::from_cube_state(s);
        self.derived = [[false; 9]; 6];
        self.recheck();
        self.autoswitch_if_full();
        self.dirty = true;
    }

    /// 清空:只保留中心(中心是固定的)
    /// **重置为规范状态**:每一格都取该面的中心色 ⇒ 各面同色。
    /// 幂等:不论之前是什么(半成品 / 打乱 / 清空),结果都一样。
    pub fn reset_to_solid(&mut self) {
        for f in 0..6 {
            for i in 0..9 {
                self.cells[f][i] = Some(FACE_COLOR[f]);
            }
            self.derived[f] = [false; 9];
        }
        self.picker = None;
        self.recheck();
        self.dirty = true;
        self.message = "已重置(各面同色)".into();
    }

    pub fn clear(&mut self) {
        self.cells = [[None; 9]; 6];
        self.derived = [[false; 9]; 6];
        for f in 0..6 {
            self.cells[f][CENTER_IDX] = Some(FACE_COLOR[f]);
        }
        self.recheck();
        self.active = StickerColor::W;
        self.dirty = true;
        self.message = "已清空(保留中心块 —— 它们固定不可改)".into();
    }

    /// 涂一格:**先试探性应用 + 可行性判定,不可行就回滚**。
    ///
    /// 关键设计:不允许"先涂进去、最后再报错"。
    /// 只要这一笔会让盘面**再也补不成合法魔方**(配额超了 / 造出非法块 /
    /// 块重复 / 剩余块配不上剩余位置 / 填满后朝向或奇偶不对),就直接拒绝。
    pub fn paint(&mut self, face: usize, idx: usize, c: StickerColor) {
        let g = face * 9 + idx;
        if let Err(e) = validate::can_paint(&self.cells, g, c) {
            self.message = format!("【不能涂】{e}");
            return;
        }
        // 快照(便于回滚)
        let cells_bak = self.cells;
        let derived_bak = self.derived;

        self.cells[face][idx] = Some(c);
        self.derived[face][idx] = false;
        self.rederive();

        if let Err(e) = validate::is_feasible(&self.cells) {
            self.cells = cells_bak;
            self.derived = derived_bak;
            self.recheck();
            self.dirty = true;
            self.message = if e.contains("朝向") || e.contains("奇偶") || e.contains("不存在") {
                format!("【不能涂】{e}\n   点「修正」可自动修一处;或用「擦除」腾出空位")
            } else {
                format!("【不能涂】{e}")
            };
            return;
        }

        let msg = format!(
            "已涂 {} 到 {}{}",
            color_cn(c),
            "上右前下左后".chars().nth(face).unwrap(),
            idx + 1
        );
        self.autoswitch_if_full();
        self.recheck();
        self.dirty = true;
        self.message = msg;
    }

    /// 一步修正:自动找一处最小改动让盘面重新可解
    pub fn repair(&mut self) {
        match validate::repair(&mut self.cells, &mut self.derived) {
            Ok(msg) => {
                self.recheck();
                self.dirty = true;
                self.message = format!("已修正:{msg}");
            }
            Err(e) => self.message = e,
        }
    }

    /// 擦除一格(不触发自动推导 —— 让用户能先腾出空位再交换两个贴纸)
    pub fn erase(&mut self, face: usize, idx: usize) {
        if idx == CENTER_IDX {
            self.message = "中心块固定,不能清除".into();
            return;
        }
        self.cells[face][idx] = None;
        self.derived[face][idx] = false;
        self.recheck();
        self.dirty = true;
        self.message = format!(
            "已清除 {}{} —— 现在可以把它涂成别的颜色",
            "上右前下左后".chars().nth(face).unwrap(),
            idx + 1
        );
    }

    /// 供自检调用的公开入口(与涂色后走同一条推导/修正路径)
    pub fn rederive_public(&mut self) {
        self.rederive();
        self.recheck();
    }

    /// 清掉所有"推导"格,再重新推导一遍(保证推导结果与手涂一致)
    fn rederive(&mut self) {
        for f in 0..6 {
            for i in 0..9 {
                if self.derived[f][i] {
                    self.cells[f][i] = None;
                    self.derived[f][i] = false;
                }
            }
        }
        for f in 0..6 {
            self.cells[f][CENTER_IDX] = Some(FACE_COLOR[f]);
        }
        let filled = validate::propagate(&mut self.cells, &mut self.derived);
        if !filled.is_empty() {
            self.message = format!("{}  (+{} auto-filled)", self.message, filled.len());
        }

        // ── 快填满时的收尾:补全 / 修正 ──
        //
        // 注意顺序陷阱:propagate 可能**先把剩余格子填满**(填成非法状态),
        // 所以不能只看"还剩几格",必须区分三种情况:
        //   A. 已填满但非法            → 卡死 → 修正
        //   B. 没填满但怎么补都不合法    → 卡死 → 修正
        //   C. 没填满且存在合法补全      → 直接补完
        let left = validate::empty_count(&self.cells);
        let complete = left == 0;
        let stuck = if complete {
            validate::validate(&self.cells).is_err()
        } else if left <= 4 {
            validate::find_completion(&self.cells).is_none()
        } else {
            false
        };

        if stuck {
            // 先试"一处最小改动";若失败(说明同时坏了多个不变量)则用"定向全修"
            let outcome = match validate::repair(&mut self.cells, &mut self.derived) {
                Ok(m) => Ok(("一处修正", m)),
                Err(_) => validate::force_fix(&mut self.cells, &mut self.derived)
                    .map(|m| ("定向修正", m)),
            };
            match outcome {
                Ok((_kind, fix)) => {
                    // 修完再试一次补全,尽量把盘面交给用户时是"可继续/已完整"的
                    let n = validate::empty_count(&self.cells);
                    if n > 0 && n <= 4 {
                        // 只有"唯一解"才替用户补全 —— 多解时补一个等于把用户的魔方换掉
                        if let Some(done) = validate::find_unique_completion(&self.cells) {
                            self.cells = done;
                        }
                    }
                    let n2 = validate::empty_count(&self.cells);
                    self.message = if n2 == 0 {
                        format!("盘面原本无解(全局约束,涂的过程中查不出)—— {fix};已全部补全")
                    } else {
                        format!("盘面原本无解 —— {fix};还剩 {n2} 格请继续涂")
                    };
                }
                Err(e) => self.message = e,
            }
        } else if !complete && left <= 4 {
            if let Some(done) = validate::find_unique_completion(&self.cells) {
                self.cells = done;
                self.message = format!("已自动补全最后 {left} 格(合法解唯一)");
            } else if validate::find_completion(&self.cells).is_some() {
                // 有解但不唯一:不替用户猜,把决定权交回去(以前这里会静默补一个,
                // 约 23% 的会话会被补成"另一个魔方")
                self.message =
                    format!("还剩 {left} 格,存在多种合法补法 —— 请按你自己的魔方继续涂");
            }
        }
    }

    pub fn recheck(&mut self) {
        self.valid = validate::validate(&self.cells).is_ok();
        self.error_cells = validate::contradiction_cells(&self.cells);
        self.recompute_placeable();
    }

    /// 算出"当前选中的颜色还能填在哪些格子":
    /// 试探性涂上去 → 局部检查 + 可行性判定,通过才算可填。
    /// (已经就是这个颜色的格子跳过 —— 它们不需要再涂)
    pub fn recompute_placeable(&mut self) {
        let c = self.active;
        let mut out = Vec::new();
        if !self.erasing {
            for g in 0..54 {
                let (f, i) = validate::split(g);
                if self.cells[f][i] == Some(c) {
                    continue;
                }
                if validate::can_paint(&self.cells, g, c).is_err() {
                    continue;
                }
                let mut probe = self.cells;
                validate::set(&mut probe, g, Some(c));
                if validate::is_feasible(&probe).is_ok() {
                    out.push(g);
                }
            }
        }
        self.placeable = out;
    }

    /// 当前调色板颜色若已放满,自动切到还有余量的颜色
    fn autoswitch_if_full(&mut self) {
        let full = validate::full_colors(&self.cells);
        if !full[validate::color_idx(self.active)] {
            return;
        }
        if let Some(c) = validate::available_colors(&self.cells).first().copied() {
            self.active = c;
            self.message = format!("该颜色已满,已自动切到「{}」", color_cn(c));
        }
    }

    /// 校验结果文本
    pub fn valid_text(&self) -> String {
        match validate::validate(&self.cells) {
            Ok(()) => "状态合法 —— 点「应用」后再按 S 求解".to_string(),
            Err(errs) => {
                let filled = 54 - (0..54).filter(|&g| validate::get(&self.cells, g).is_none()).count();
                let mut out = format!("[{filled}/54 格已填]");
                for (i, e) in errs.iter().take(3).enumerate() {
                    out.push_str(if i == 0 { " " } else { "\n   " });
                    out.push_str(e);
                }
                if errs.len() > 3 {
                    out.push_str(&format!("\n   (+{} more)", errs.len() - 3));
                }
                out
            }
        }
    }
}

/// 颜色中文名(界面提示用)
fn color_cn(c: StickerColor) -> &'static str {
    match c {
        StickerColor::W => "白",
        StickerColor::Y => "黄",
        StickerColor::R => "红",
        StickerColor::O => "橙",
        StickerColor::B => "蓝",
        StickerColor::G => "绿",
    }
}

/// 解析 `RUBIK_STATE` / `--state` 的 54 字符状态串
///
/// 面序 **U R F D L B**(每面 9 格行优先);字母可用 `U/R/F/D/L/B` 或颜色名
/// `W/R/G/Y/O/B`,大小写与空白忽略。会做**完整合法性校验**,非法状态直接拒绝。
pub fn parse_facelets(s: &str) -> Result<cubr_core::model::CubeState, String> {
    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() != 54 {
        return Err(format!("需要 54 个字符,收到 {}", chars.len()));
    }
    let mut cells: Cells = [[None; 9]; 6];
    for (i, ch) in chars.iter().enumerate() {
        let c = match ch.to_ascii_uppercase() {
            'U' | 'W' => StickerColor::W,
            'R' => StickerColor::R,
            'F' | 'G' => StickerColor::G,
            'D' | 'Y' => StickerColor::Y,
            'L' | 'O' => StickerColor::O,
            'B' => StickerColor::B,
            other => return Err(format!("第 {} 个字符 {other:?} 不是合法颜色", i + 1)),
        };
        let (f, k) = validate::split(i);
        cells[f][k] = Some(c);
    }
    if let Err(errs) = validate::validate(&cells) {
        return Err(format!("状态不合法: {}", errs.join("; ")));
    }
    validate::to_cube_state(&cells).ok_or_else(|| "内部错误:状态未能组装".to_string())
}

/// 从环境变量 / 命令行取初始状态(命令行优先)
pub fn state_from_cli() -> Option<cubr_core::model::CubeState> {
    let arg = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|w| w[0] == "--state")
        .map(|w| w[1].clone());
    let raw = arg.or_else(|| std::env::var("RUBIK_STATE").ok())?;
    match parse_facelets(&raw) {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("[state] 解析失败: {e}");
            None
        }
    }
}

// ─────────────────────────── UI ───────────────────────────

/// 某格该显示什么底色(未填=深灰;推导=压暗;手涂=原色)
fn cell_bg(edit: &EditState, face: usize, idx: usize) -> Color {
    match edit.cells[face][idx] {
        None => UNKNOWN_BG,
        Some(c) => {
            let base = sticker_color(c);
            if edit.derived[face][idx] {
                match base {
                    Color::Srgba(s) => Color::srgb(s.red * 0.6, s.green * 0.6, s.blue * 0.6),
                    other => other,
                }
            } else {
                base
            }
        }
    }
}

/// 冲突标记色:**品红** —— 刻意避开 6 种贴纸色(白/黄/红/橙/蓝/绿),
/// 否则红贴纸旁边出现红框根本分不清是"涂的红色"还是"冲突提示"
pub const CONFLICT_COLOR: Color = Color::srgb(1.0, 0.15, 0.85);

/// "这个颜色能填在这里"的提示色。
/// 用**亮薄荷绿 + 闪烁**:绿色本身是 6 种贴纸色之一,靠"动起来"和静态贴纸区分。
pub const HINT_DIM: Color = Color::srgb(0.10, 0.45, 0.20);
pub const HINT_BRIGHT: Color = Color::srgb(0.45, 1.00, 0.55);

/// 边框:冲突=品红(最高优先,且加粗);中心=金色(锁定);推导=青色;其余=灰
fn cell_border(edit: &EditState, face: usize, idx: usize) -> Color {
    let g = face * 9 + idx;
    if edit.error_cells.contains(&g) {
        CONFLICT_COLOR
    } else if idx == CENTER_IDX {
        Color::srgb(0.85, 0.72, 0.30)
    } else if edit.derived[face][idx] {
        Color::srgb(0.35, 0.75, 0.85)
    } else {
        Color::srgb(0.25, 0.25, 0.3)
    }
}

/// 调色板色块的显示底色:已放满的颜色压暗表示不可选
fn swatch_bg(edit: &EditState, c: StickerColor) -> Color {
    let base = sticker_color(c);
    if validate::full_colors(&edit.cells)[validate::color_idx(c)] {
        match base {
            Color::Srgba(s) => Color::srgb(s.red * 0.32, s.green * 0.32, s.blue * 0.32),
            other => other,
        }
    } else {
        base
    }
}

#[allow(dead_code)]
pub fn spawn_editor(mut commands: Commands, edit: Res<EditState>, font: Res<crate::UiFont>) {
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                top: Val::Px(48.0),
                width: Val::Px(NET_W + PAD * 2.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(PAD)),
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.10, 0.10, 0.13, 0.92)),
            EditorRoot,
            crate::VisibilityInMode(crate::UiMode::Input),
        ))
        .id();

    commands.entity(root).with_children(|panel| {
        panel.spawn((
            Text::new("初始状态编辑器 [E] 隐藏"),
            TextFont { font: font.0.clone().into(), font_size: FontSize::Px(12.0), ..default() },
            TextColor(Color::srgb(0.8, 0.85, 0.95)),
            TextLayout { linebreak: LineBreak::AnyCharacter, ..default() },
        ));

        // 边框颜色图例:用真实色块 + 文字,避免"为什么这格带框那格不带"的困惑
        panel
            .spawn(Node {
                column_gap: Val::Px(4.0),
                row_gap: Val::Px(2.0),
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                for (col, label) in [
                    (Color::srgb(0.85, 0.72, 0.30), "中心(固定)"),
                    (Color::srgb(0.35, 0.75, 0.85), "自动推导"),
                    (Color::srgb(0.25, 0.25, 0.3), "你手涂的"),
                    (CONFLICT_COLOR, "冲突位置"),
                    (HINT_BRIGHT, "可填此处"),
                ] {
                    row.spawn((
                        Node { width: Val::Px(10.0), height: Val::Px(10.0), border: UiRect::all(Val::Px(2.0)), ..default() },
                        BorderColor::all(col),
                        BackgroundColor(Color::NONE),
                    ));
                    row.spawn((
                        Text::new(label),
                        TextFont { font: font.0.clone().into(), font_size: FontSize::Px(11.0), ..default() },
                        TextColor(Color::srgb(0.72, 0.76, 0.85)),
                        TextLayout { linebreak: LineBreak::AnyCharacter, ..default() },
                    ));
                }
            });

        panel
            .spawn((
                Node {
                    width: Val::Px(NET_W),
                    height: Val::Px(NET_H),
                    position_type: PositionType::Relative,
                    ..default()
                },
                FaceNet,
            ))
            .with_children(|net| {
                for face in 0..6 {
                    let (fc, fr) = FACE_POS[face];
                    for idx in 0..9 {
                        let (cx, cy) = ((idx % 3) as f32, (idx / 3) as f32);
                        let x = fc as f32 * FACE_PITCH + cx * (CELL + INNER_GAP);
                        let y = fr as f32 * FACE_PITCH + cy * (CELL + INNER_GAP);
                        net.spawn((
                            Button,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(x),
                                top: Val::Px(y),
                                width: Val::Px(CELL),
                                height: Val::Px(CELL),
                                border: UiRect::all(Val::Px(if idx == CENTER_IDX { 2.0 } else { 1.0 })),
                                ..default()
                            },
                            BorderColor::all(cell_border(&edit, face, idx)),
                            BackgroundColor(cell_bg(&edit, face, idx)),
                            FaceletCell { face, idx },
                        ));
                    }
                }
            });

        panel
            .spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() })
            .with_children(|row| {
                // 橡皮擦
                row.spawn((
                    Button,
                    Node {
                        // 固定宽度,避免"擦除"被挤成一个字
                        min_width: Val::Px(42.0),
                        height: Val::Px(20.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.3, 0.3, 0.35)),
                    BackgroundColor(Color::srgb(0.35, 0.33, 0.3)),
                    Text::new("擦除"),
                    TextLayout { linebreak: LineBreak::NoWrap, ..default() },
                    TextFont { font: font.0.clone().into(), font_size: FontSize::Px(11.0), ..default() },
                    TextColor(Color::srgb(0.95, 0.9, 0.8)),
                    EraserButton,
                ));
                for c in COLOR_ORDER {
                    row.spawn((
                        Button,
                        Node {
                            width: Val::Px(30.0),
                            height: Val::Px(20.0),
                            border: UiRect::all(Val::Px(2.0)),
                            ..default()
                        },
                        BorderColor::all(if c == edit.active {
                            Color::srgb(1.0, 1.0, 1.0)
                        } else {
                            Color::srgb(0.3, 0.3, 0.35)
                        }),
                        BackgroundColor(swatch_bg(&edit, c)),
                        PaletteSwatch(c),
                    ));
                }
            });

        panel
            .spawn(Node { column_gap: Val::Px(6.0), ..default() })
            .with_children(|row| {
                for (label, which) in [
                    ("应用", 0u8),
                    ("修正", 4),
                    ("从魔方读取", 1),
                    ("清空", 2),
                    ("还原态", 3),
                ] {
                    let mut e = row.spawn((
                        Button,
                        Node {
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BorderColor::all(Color::srgb(0.4, 0.5, 0.7)),
                        BackgroundColor(Color::srgb(0.18, 0.22, 0.32)),
                        Text::new(label),
                        TextFont { font: font.0.clone().into(), font_size: FontSize::Px(12.0), ..default() },
                        TextColor(Color::srgb(0.9, 0.93, 1.0)),
                    ));
                    match which {
                        0 => e.insert(ApplyButton),
                        1 => e.insert(FromCubeButton),
                        2 => e.insert(ClearButton),
                        3 => e.insert(SolvedButton),
                        _ => e.insert(RepairButton),
                    };
                }
            });

        panel.spawn((
            Text::new(""),
            TextFont { font: font.0.clone().into(), font_size: FontSize::Px(12.0), ..default() },
            TextColor(Color::srgb(0.75, 0.8, 0.9)),
            // 报错文字很长且面板很窄 ⇒ 必须能按字断行
            TextLayout { linebreak: LineBreak::AnyCharacter, ..default() },
            EditorStatus,
        ));
    });
}

// ─────────────────────────── 交互 ───────────────────────────

/// 点击格子涂色(中心/违规会被 `EditState::paint` 拒绝)
pub fn cell_click(
    mut q: Query<(&Interaction, &FaceletCell), (Changed<Interaction>, With<Button>)>,
    mut edit: ResMut<EditState>,
) {
    // 取色器是模态层；弹层打开时，底下的展开图不能再次消费同一点击。
    if edit.picker.is_some() {
        return;
    }
    for (interaction, cell) in q.iter_mut() {
        if *interaction == Interaction::Pressed {
            // 新的交互:点格子弹出六边形取色器(见 ui::apply_picker)
            if edit.picker.is_none() && !edit.erasing {
                edit.picker = Some(cell.face * 9 + cell.idx);
                edit.dirty = true;
                continue;
            }
            if edit.erasing {
                edit.erase(cell.face, cell.idx);
            } else {
                let active = edit.active;
                edit.paint(cell.face, cell.idx, active);
            }
        }
    }
}

/// 橡皮擦按钮
pub fn eraser_click(
    mut q: Query<(&Interaction, &mut BorderColor), (Changed<Interaction>, With<EraserButton>)>,
    mut edit: ResMut<EditState>,
) {
    for (interaction, mut border) in q.iter_mut() {
        if *interaction == Interaction::Pressed {
            edit.erasing = !edit.erasing;
            edit.message = if edit.erasing {
                "橡皮擦:点格子清除该贴纸".into()
            } else {
                "退出橡皮擦".into()
            };
            edit.recompute_placeable();
            edit.dirty = true;
        }
        *border = BorderColor::all(if edit.erasing {
            Color::srgb(1.0, 0.85, 0.4)
        } else {
            Color::srgb(0.3, 0.3, 0.35)
        });
    }
}

/// 选调色板颜色
pub fn palette_click(
    mut q: Query<(&Interaction, &PaletteSwatch, &mut BorderColor), (Changed<Interaction>, With<Button>)>,
    mut edit: ResMut<EditState>,
) {
    if edit.picker.is_some() {
        return;
    }
    for (interaction, sw, _) in q.iter_mut() {
        if *interaction == Interaction::Pressed {
            if validate::full_colors(&edit.cells)[validate::color_idx(sw.0)] {
                edit.message = format!("「{}」已放满 9 格,不可选", color_cn(sw.0));
            } else {
                edit.active = sw.0;
                edit.message = format!("当前颜色:{}", color_cn(sw.0));
            }
            edit.dirty = true; // 让选中高亮刷新
        }
    }
    let active = edit.active;
    for (_, sw, mut border) in q.iter_mut() {
        *border = BorderColor::all(if sw.0 == active {
            Color::srgb(1.0, 1.0, 1.0)
        } else {
            Color::srgb(0.3, 0.3, 0.35)
        });
    }
}

/// 把编辑器盘面应用到魔方(**Apply 按钮与调试钩子共用同一条路径**)
pub fn apply_to_model(
    edit: &mut EditState,
    model: &mut Model,
    pb: &mut Playback,
    view: &mut SolutionView,
    q: &mut Query<(Entity, &CubieIndex, &mut Transform)>,
) {
    match validate::validate(&edit.cells) {
        Err(errs) => edit.message = format!("【拒绝应用】{}", errs.join(";")),
        Ok(()) => {
            let st = validate::to_cube_state(&edit.cells).unwrap();
            model.core.paint(&st);
            model.history.clear();
            pb.queue.clear();
            pb.active = None;
            pb.paused = false;
            view.moves.clear();
            view.cursor = 0;
            crate::cube::sync_transforms3(&model.core, q);
            edit.message = "已应用到魔方 —— 按 S 开始求解".into();
        }
    }
}

/// 修正按钮
pub fn repair_click(
    q: Query<&Interaction, (Changed<Interaction>, With<RepairButton>)>,
    mut edit: ResMut<EditState>,
) {
    if q.iter().any(|i| *i == Interaction::Pressed) {
        edit.repair();
    }
}

/// Apply / From cube / Clear / Solved
#[allow(clippy::too_many_arguments)]
pub fn editor_buttons(
    apply_q: Query<&Interaction, (Changed<Interaction>, With<ApplyButton>)>,
    from_q: Query<&Interaction, (Changed<Interaction>, With<FromCubeButton>)>,
    clear_q: Query<&Interaction, (Changed<Interaction>, With<ClearButton>)>,
    solved_q: Query<&Interaction, (Changed<Interaction>, With<SolvedButton>)>,
    mut edit: ResMut<EditState>,
    mut model: ResMut<Model>,
    mut pb: ResMut<Playback>,
    mut view: ResMut<SolutionView>,
    mut q: Query<(Entity, &CubieIndex, &mut Transform)>,
) {
    if apply_q.iter().any(|i| *i == Interaction::Pressed) {
        apply_to_model(&mut edit, &mut model, &mut pb, &mut view, &mut q);
    }

    if from_q.iter().any(|i| *i == Interaction::Pressed) {
        let st = model.core.to_state();
        edit.from_cube(&st);
        edit.message = "已从当前魔方读取".into();
    }
    if clear_q.iter().any(|i| *i == Interaction::Pressed) {
        edit.clear();
    }
    if solved_q.iter().any(|i| *i == Interaction::Pressed) {
        edit.from_cube(&cubr_core::model::CubeState::solved());
        edit.message = "已切回还原态".into();
    }
}

/// 把盘面 + 调色板刷回 UI(底色 + 边框)
pub fn refresh_cells(
    mut edit: ResMut<EditState>,
    mut cells: Query<
        (&FaceletCell, &mut BackgroundColor, &mut BorderColor, &mut Node),
        Without<PaletteSwatch>,
    >,
    mut swatches: Query<(&PaletteSwatch, &mut BackgroundColor, &mut BorderColor), Without<FaceletCell>>,
) {
    if !edit.dirty {
        return;
    }
    {
        let e = &*edit;
        // 底色还是要在这里刷(边框交给 pulse_hints 每帧处理)
        for (cell, mut bg, _border, _node) in cells.iter_mut() {
            bg.0 = cell_bg(e, cell.face, cell.idx);
        }
        let active = e.active;
        let full = validate::full_colors(&e.cells);
        for (sw, mut bg, mut border) in swatches.iter_mut() {
            bg.0 = swatch_bg(e, sw.0);
            *border = BorderColor::all(if sw.0 == active {
                Color::srgb(1.0, 1.0, 1.0) // 选中
            } else if full[validate::color_idx(sw.0)] {
                Color::srgb(0.5, 0.2, 0.2) // 已满:暗红边
            } else {
                Color::srgb(0.3, 0.3, 0.35)
            });
        }
    }
    edit.dirty = false;
}

/// 单面视图的单格尺寸与间距(比展开图大得多,方便手指点)
pub const BIG_CELL: f32 = 56.0;
pub const BIG_GAP: f32 = 6.0;
pub const FACE_SIDE: f32 = BIG_CELL * 3.0 + BIG_GAP * 2.0;

/// 单面正交视图:**只显示当前选中的面**,9 格按 3×3 放大铺开
/// (这就是"输入界面以魔方展示,每次展示一面,正交显示" ——
///  平面 3×3 即正交投影下的一面)
pub fn layout_single_face(
    ui_state: Res<crate::UiState>,
    _edit: ResMut<EditState>,
    mut q: Query<(&FaceletCell, &mut Node, &mut BorderColor)>,
    mut nets: Query<&mut Node, (With<FaceNet>, Without<FaceletCell>)>,
) {
    for (cell, mut node, mut border) in q.iter_mut() {
        if cell.face != ui_state.face {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        }
        node.display = Display::Flex;
        let (cx, cy) = (cell.idx % 3, cell.idx / 3);
        node.left = Val::Px(cx as f32 * (BIG_CELL + BIG_GAP));
        node.top = Val::Px(cy as f32 * (BIG_CELL + BIG_GAP));
        node.width = Val::Px(BIG_CELL);
        node.height = Val::Px(BIG_CELL);
        // 中心格边框高亮一点,提示"这是哪个面"
        if cell.idx == CENTER_IDX {
            *border = BorderColor::all(Color::srgb(0.95, 0.85, 0.45));
        }
    }
    for mut net in nets.iter_mut() {
        net.width = Val::Px(FACE_SIDE);
        net.height = Val::Px(FACE_SIDE);
    }
}

/// 面网格容器标记
#[derive(Component)]
pub struct FaceNet;

/// 面板显隐跟随 `EditState.visible`(由动作层切换)
pub fn sync_visibility(edit: Res<EditState>, mut root: Query<&mut Node, With<EditorRoot>>) {
    let want = if edit.visible { Display::Flex } else { Display::None };
    for mut node in root.iter_mut() {
        if node.display != want {
            node.display = want;
        }
    }
}

/// 调试:把指定的几格标成"冲突"以便检查渲染效果
/// (`RUBIK_DEBUG_CONFLICT=1` 时在启动后调用一次)
pub fn debug_mark_conflict(mut edit: ResMut<EditState>) {
    edit.error_cells = vec![8, 9, 20, 5, 10]; // URF 角 + UR 棱
    edit.dirty = true;
    edit.message = "【调试】已把 URF 角与 UR 棱标记为冲突位置".into();
    println!("[debug] 已标记冲突格用于检查品红边框渲染");
}

/// 闪烁相位
#[derive(Resource, Default)]
pub struct HintPulse {
    pub t: f32,
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Srgba(x), Color::Srgba(y)) => Color::srgb(
            x.red + (y.red - x.red) * t,
            x.green + (y.green - x.green) * t,
            x.blue + (y.blue - x.blue) * t,
        ),
        _ => b,
    }
}

/// 每帧刷新所有格子的边框:
/// - **可填位置**(当前选中颜色能合法放上去的) → 亮绿呼吸闪烁
/// - 冲突位置 → 品红 3px;中心 → 金色 2px;其余按来源(灰=手涂 / 青=推导)
pub fn pulse_hints(
    time: Res<Time>,
    mut pulse: ResMut<HintPulse>,
    edit: Res<EditState>,
    mut cells: Query<(&FaceletCell, &mut BorderColor, &mut Node), Without<PaletteSwatch>>,
) {
    pulse.t += time.delta_secs() * 4.0;
    let k = 0.5 - 0.5 * pulse.t.cos(); // 0..1 平滑呼吸
    let hint = lerp_color(HINT_DIM, HINT_BRIGHT, k);

    for (cell, mut border, mut node) in cells.iter_mut() {
        let g = cell.face * 9 + cell.idx;
        if edit.placeable.contains(&g) {
            *border = BorderColor::all(hint);
            node.border = UiRect::all(Val::Px(2.5));
        } else {
            *border = BorderColor::all(cell_border(&edit, cell.face, cell.idx));
            let conflict = edit.error_cells.contains(&g);
            node.border = UiRect::all(Val::Px(if conflict {
                3.0
            } else if cell.idx == CENTER_IDX {
                2.0
            } else {
                1.0
            }));
        }
    }
}

// ─────────────── 输入回归测试:随机打乱 → 逐格输入(能否完整输入?) ───────────────
//
// 回答一个问题:**拿一个真实的打乱魔方,按「自定义输入」逐格输入,能不能顺利输入完?**
//
// 为什么必须有它:输入流程里有**自动推导**(validate::propagate)和**自动补全**
// (validate::find_completion)。它们只要"猜"错一次,错色就会写进盘面;错色占掉颜色
// 配额后,用户输入自己的真实颜色就会被「已放满 9 格」拒绝。这类 bug 单测某个函数
// 测不出来,必须走完整流程(**paint → rederive → is_feasible**)才会暴露。
//
// 用法:
//   cargo test -- --nocapture                          # 默认 80 次打乱
//   RUBIK_INPUT_FUZZ=1000 cargo run                    # 无图形环境跑 1000 次
//   RUBIK_INPUT_FUZZ=1000 RUBIK_INPUT_FUZZ_SEED=7 cargo run
//
// 失败时会打印**可直接复现**的 54 字符盘面,配合 `RUBIK_STATE=<字符串>` 复现。

struct FuzzRng(u64);

impl FuzzRng {
    fn new(seed: u64) -> Self {
        FuzzRng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x1234_5678_9ABC_DEF0))
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}

/// 盘面 → 54 字符(面序 U R F D L B,行优先,颜色字母)—— `RUBIK_STATE` 同格式
pub fn cells_to_facelets(cells: &Cells) -> String {
    let mut s = String::with_capacity(54);
    for f in 0..6 {
        for i in 0..9 {
            s.push(match cells[f][i] {
                Some(StickerColor::W) => 'W',
                Some(StickerColor::Y) => 'Y',
                Some(StickerColor::R) => 'R',
                Some(StickerColor::O) => 'O',
                Some(StickerColor::B) => 'B',
                Some(StickerColor::G) => 'G',
                None => '.',
            });
        }
    }
    s
}

/// 一次会话失败的原因
enum SessionFail {
    /// 自动推导/补全填出的颜色与真实魔方不一致
    Derived(String),
    /// 取色器把真实颜色判为"不可填"(用户视角:色块被置灰)
    Picker(String),
    /// paint() 直接拒绝(状态行出现「不能涂」)
    Paint(String),
    /// 输入结束后盘面 ≠ 真实魔方
    Final(String),
}

/// 统计
#[derive(Default)]
pub struct InputFuzzStats {
    pub trials: usize,
    pub sessions: usize,
    pub picker_rejected: usize,
    pub paint_rejected: usize,
    pub derived_mismatch: usize,
    pub final_mismatch: usize,
    pub bad_target: usize,
    pub example: Option<String>,
}

impl InputFuzzStats {
    pub fn failed(&self) -> bool {
        self.picker_rejected > 0
            || self.paint_rejected > 0
            || self.derived_mismatch > 0
            || self.final_mismatch > 0
            || self.bad_target > 0
    }
}

/// 模拟几种真实输入顺序
fn fuzz_orders(rng: &mut FuzzRng) -> Vec<(&'static str, Vec<usize>)> {
    let row: Vec<usize> = (0..54).collect();
    let rev: Vec<usize> = (0..54).rev().collect();
    let mut rand: Vec<usize> = (0..54).collect();
    for i in (1..rand.len()).rev() {
        let j = rng.below(i + 1);
        rand.swap(i, j);
    }
    // 逐面输入:面的顺序随机、面内行优先 —— 最接近真人"一面一面抄"
    let mut faces: Vec<usize> = (0..6).collect();
    for i in (1..faces.len()).rev() {
        let j = rng.below(i + 1);
        faces.swap(i, j);
    }
    let mut by_face: Vec<usize> = Vec::with_capacity(54);
    for f in faces {
        for i in 0..9 {
            by_face.push(f * 9 + i);
        }
    }
    vec![("行优先", row), ("逆序", rev), ("随机", rand), ("逐面", by_face)]
}

/// 跑一次完整会话:空盘面(只留中心)→ 按 order 逐格输入 target
fn fuzz_one_session(target: &Cells, order: &[usize]) -> Result<(), SessionFail> {
    let mut ed = EditState::default();
    ed.clear(); // 与 Action::CustomInput 一致:清空,只留中心块

    for &g in order {
        let (f, i) = validate::split(g);
        let want = match target[f][i] {
            Some(c) => c,
            None => continue,
        };
        let face_ch = "URFDLB".chars().nth(f).unwrap_or('?');

        // 已被自动推导/补全填上 ⇒ 必须与真实魔方一致
        if let Some(have) = ed.cells[f][i] {
            if have != want {
                return Err(SessionFail::Derived(format!(
                    "自动填的 {face_ch}{} 是 {have:?},真实是 {want:?}",
                    i + 1
                )));
            }
            continue;
        }
        // ① 取色器允许吗(用户看到的是色块置灰 / 点了没反应)
        let allowed = validate::allowed_colors(&ed.cells, g);
        if !allowed[validate::color_idx(want)] {
            let why = validate::can_paint(&ed.cells, g, want)
                .err()
                .unwrap_or_else(|| "可行性判定拒绝".into());
            return Err(SessionFail::Picker(format!(
                "取色器不允许 {want:?} 填 {face_ch}{} —— {why}",
                i + 1
            )));
        }
        // ② 真正涂上去(走 paint → rederive → is_feasible)
        ed.active = want;
        ed.paint(f, i, want);
        if ed.cells[f][i] != Some(want) {
            return Err(SessionFail::Paint(format!(
                "paint() 拒绝 {want:?} → {face_ch}{} —— {}",
                i + 1,
                ed.message
            )));
        }
    }
    // ③ 结束时盘面必须等于真实魔方
    if ed.cells != *target {
        return Err(SessionFail::Final(
            "输入结束后盘面与真实魔方不一致(被自动补成了别的解?)".into(),
        ));
    }
    Ok(())
}

/// 主入口:随机打乱 trials 次,每次用 4 种顺序输入。返回 true = 全部通过。
pub fn run_input_fuzz(trials: usize, seed: u64) -> bool {
    use cubr_core::core::CubeCore;
    use cubr_core::model::Move;

    let mut rng = FuzzRng::new(seed);
    let mut st = InputFuzzStats { trials, ..Default::default() };

    for t in 0..trials {
        // ① 随机打乱 25 步(避免连续转同一面)
        let mut core = CubeCore::solved();
        let mut moves: Vec<Move> = Vec::new();
        let mut last_axis = (99, 99, 99);
        while moves.len() < 25 {
            let m = Move::ALL[rng.below(Move::ALL.len())];
            let a = m.axis();
            let cur = (a.x, a.y, a.z);
            if cur == last_axis {
                continue;
            }
            last_axis = cur;
            moves.push(m);
            core.apply(m);
        }
        let target = validate::from_cube_state(&core.to_state());
        if validate::validate(&target).is_err() {
            st.bad_target += 1; // 理论上不该发生
            continue;
        }

        // ② 用 4 种顺序各输入一遍
        for (name, order) in fuzz_orders(&mut rng) {
            st.sessions += 1;
            if let Err(f) = fuzz_one_session(&target, &order) {
                let (kind, msg) = match &f {
                    SessionFail::Derived(m) => ("自动推导错误", m.clone()),
                    SessionFail::Picker(m) => ("取色器拒绝真实颜色", m.clone()),
                    SessionFail::Paint(m) => ("涂色被拒", m.clone()),
                    SessionFail::Final(m) => ("结束时盘面不符", m.clone()),
                };
                match f {
                    SessionFail::Derived(_) => st.derived_mismatch += 1,
                    SessionFail::Picker(_) => st.picker_rejected += 1,
                    SessionFail::Paint(_) => st.paint_rejected += 1,
                    SessionFail::Final(_) => st.final_mismatch += 1,
                }
                if st.example.is_none() {
                    st.example = Some(format!(
                        "试验 #{t} 顺序「{name}」: {kind}: {msg}\n        RUBIK_STATE={}\n        打乱 = {}",
                        cells_to_facelets(&target),
                        moves.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>().join(" ")
                    ));
                }
            }
        }
    }

    println!("== 自定义输入回归测试:随机打乱 → 逐格输入 ==");
    println!("  打乱次数(试验)      = {}", st.trials);
    println!("  输入会话(试验×4 顺序) = {}", st.sessions);
    println!("  ────────────────────────────────");
    println!("  取色器拒绝真实颜色   = {}", st.picker_rejected);
    println!("  涂色被拒(【不能涂】)  = {}", st.paint_rejected);
    println!("  自动推导/补全填错     = {}", st.derived_mismatch);
    println!("  结束时盘面不符        = {}", st.final_mismatch);
    println!("  生成的盘面本身非法    = {}", st.bad_target);
    if let Some(e) = &st.example {
        println!("\n  首个失败:\n        {e}");
    }
    let ok = !st.failed();
    println!(
        "\n  结论: {}",
        if ok {
            "✅ 全部会话都能完整输入(无拒绝/无错填)"
        } else {
            "❌ 存在无法完成或被误拒的输入会话"
        }
    );
    ok
}

#[cfg(test)]
mod input_fuzz_test {
    /// 回归:随机打乱 → 逐格输入必须次次都能完成。
    /// (2026-09 曾因 propagate 猜朝向导致 23% 会话被污染,见 validate.rs 的同名守卫)
    #[test]
    fn random_scramble_can_always_be_entered() {
        let trials: usize = std::env::var("RUBIK_FUZZ_TRIALS")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(80);
        let seed: u64 = std::env::var("RUBIK_FUZZ_SEED")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);
        assert!(
            super::run_input_fuzz(trials, seed),
            "输入回归测试失败:有会话无法完整输入(详见上方统计)"
        );
    }
}

/// 状态行
pub fn update_status(edit: Res<EditState>, mut q: Query<&mut Text, With<EditorStatus>>) {
    // 详细提示/错误都进日志弹窗(见 ui::log_editor_messages),这里只留一行紧凑状态
    let want = format!(
        "{}   可填 {} 格(闪烁绿框)   详情见「日志」",
        edit.valid_text(),
        edit.placeable.len()
    );
    for mut t in q.iter_mut() {
        if t.0 != want {
            t.0 = want.clone();
        }
    }
}
