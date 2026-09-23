//! 界面:HUD(状态 + 解法 + 转动角度)、操作按钮面板、日志弹窗。

use std::collections::VecDeque;
use web_time::Instant; // wasm 上 std 的 Instant 未实现

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::anim::{is_solved, Playback};
use crate::solve::{SolveStatus, SolverWorker};
use crate::{Action, Model, PendingAction, SolutionView, UiMode};

/// 标记 HUD 文本实体
#[derive(Component)]
pub struct HudText;

// ───────────────────────── 日志 ─────────────────────────

/// 界面日志:不再直接铺在画面上,而是攒着,**点「日志」按钮弹窗看**。
#[derive(Resource)]
pub struct LogBuffer {
    start: Instant,
    pub lines: VecDeque<String>,
    pub open: bool,
}

impl Default for LogBuffer {
    fn default() -> Self {
        let mut b = Self { start: Instant::now(), lines: VecDeque::new(), open: false };
        b.push("就绪 —— 可用右侧编辑器改状态,或点「打乱」随机一局");
        b
    }
}

impl LogBuffer {
    pub fn push(&mut self, msg: impl Into<String>) {
        let t = self.start.elapsed().as_secs_f32();
        self.lines.push_back(format!("[{t:6.1}s] {}", msg.into()));
        while self.lines.len() > 300 {
            self.lines.pop_front();
        }
    }

    /// 最近 n 行拼成文本(最新的在最下面)
    pub fn tail(&self, n: usize) -> String {
        self.lines.iter().rev().take(n).rev().cloned().collect::<Vec<_>>().join("\n")
    }
}

#[derive(Component)]
pub struct LogPanel;
#[derive(Component)]
pub struct LogText;

// ───────────────────────── 操作按钮 ─────────────────────────

#[derive(Component)]
pub struct ActionButton(pub Action);

/// 输入界面按钮:打乱(随机)/ 求解
const BUTTON_ROWS_INPUT: [&[(&str, Action)]; 1] = [
    &[
        ("随机打乱 (X)", Action::Scramble),
        ("自定义输入 (C)", Action::CustomInput),
        ("求解 (S)", Action::Solve),
        ("重置 (R)", Action::Reset),
    ],
];

/// 播放界面按钮:前进 / 后退 / 返回
const BUTTON_ROWS_PLAY: [&[(&str, Action)]; 2] = [
    &[
        ("前进 (→)", Action::Step),
        ("后退 (←)", Action::Undo),
        ("返回输入 (B)", Action::BackToInput),
    ],
    &[
        ("播放/暂停 (空格)", Action::TogglePlay),
        ("放慢 ([)", Action::Slower),
        ("加快 (])", Action::Faster),
        ("视角 ← (A)", Action::OrbitLeft),
        ("视角 → (D)", Action::OrbitRight),
    ],
];

#[derive(Component)]
pub struct ControlsRoot;

/// 按钮的三种底色(常态 / 悬停 / 按下)—— 配合圆角+描边营造"实体按键"感
#[derive(Component)]
pub struct BtnStyle {
    pub base: Color,
    pub hover: Color,
    pub press: Color,
}

/// **触摸友好**:按钮尺寸反向补偿 `UiScale` ——
/// 手机上 UiScale 会降到 0.42,若按同比例缩小,按钮物理尺寸只剩 42%(点不中 ✗)。
/// 这里让 UI 单位按 1/scale 放大(上限 2.4×),使**物理尺寸**基本恒定。
pub fn scale_buttons(
    ui_scale: Res<UiScale>,
    mut q: Query<(&mut Node, &mut TextFont), With<ActionButton>>,
    mut last: Local<f32>,
) {
    let f = (1.0 / ui_scale.0.max(0.01)).clamp(1.0, 2.4);
    if (f - *last).abs() < 0.02 {
        return;
    }
    *last = f;
    for (mut node, mut font) in q.iter_mut() {
        node.padding = UiRect::axes(Val::Px(13.0 * f), Val::Px(7.0 * f));
        node.border_radius = BorderRadius::all(Val::Px(9.0 * f));
        font.font_size = FontSize::Px(12.5 * f);
    }
}

/// 悬停/按下反馈:让按钮"按得动"
pub fn button_feedback(
    mut q: Query<
        (&Interaction, &BtnStyle, &mut BackgroundColor, &mut BorderColor),
        Changed<Interaction>,
    >,
) {
    for (i, st, mut bg, mut border) in q.iter_mut() {
        let (fill, edge) = match i {
            Interaction::Pressed => (st.press, Color::srgb(0.98, 0.86, 0.45)),
            Interaction::Hovered => (st.hover, Color::srgb(0.72, 0.80, 0.95)),
            Interaction::None => (st.base, Color::srgb(0.34, 0.38, 0.48)),
        };
        if bg.0 != fill {
            bg.0 = fill;
        }
        *border = BorderColor::all(edge);
    }
}

/// 造一个"有质感"的按钮:圆角、上亮下暗的边、悬停/按下变色
pub fn nice_button(
    commands: &mut ChildSpawnerCommands,
    font: &crate::UiFont,
    label: &str,
    action: Action,
    base: Color,
) {
    let light = |c: Color, k: f32| match c {
        Color::Srgba(x) => Color::srgb(
            (x.red * k).min(1.0),
            (x.green * k).min(1.0),
            (x.blue * k).min(1.0),
        ),
        o => o,
    };
    commands
        .spawn((
            Button,
            ActionButton(action),
            BtnStyle { base, hover: light(base, 1.45), press: light(base, 0.7) },
            Node {
                padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                border: UiRect::top(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(9.0)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.34, 0.38, 0.48)),
            BackgroundColor(base),
            Text::new(label.to_string()),
            TextFont { font: font.0.clone().into(), font_size: FontSize::Px(12.5), ..default() },
            TextColor(Color::srgb(0.93, 0.95, 1.0)),
            TextLayout { linebreak: LineBreak::NoWrap, ..default() },
        ));
}

#[allow(dead_code)]
pub fn spawn_hud(mut commands: Commands, font: Res<crate::UiFont>) {
    commands.spawn((
        Text::new("初始化…"),
        TextFont { font: font.0.clone().into(), font_size: FontSize::Px(15.0), ..default() },
        TextColor(Color::srgb(0.93, 0.93, 0.96)),
        TextLayout { linebreak: LineBreak::AnyCharacter, ..default() },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(12.0),
            ..default()
        },
        HudText,
    ));
}

/// 左下角操作面板:**按状态机显示**(输入态与播放态各一套按钮)
pub fn spawn_controls(mut commands: Commands, font: Res<crate::UiFont>) {
    for (rows, mode) in [
        (BUTTON_ROWS_INPUT.as_slice(), UiMode::Input),
        (BUTTON_ROWS_PLAY.as_slice(), UiMode::Play),
    ] {
        spawn_button_panel(&mut commands, &font, rows, mode);
    }
}

fn spawn_button_panel(
    commands: &mut Commands,
    font: &crate::UiFont,
    rows: &[&[(&str, Action)]],
    mode: UiMode,
) {
    let mut e = commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                bottom: Val::Px(12.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
            crate::VisibilityInMode(mode),
            ControlsRoot,
        ));
    e.with_children(|panel| {
            for row in rows.iter() {
                panel.spawn(Node { column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                    for (label, action) in row.iter() {
                        // 主要操作给偏暖的深蓝,次要操作给冷灰 —— 有层次
                        let base = match action {
                            Action::Solve | Action::Scramble | Action::CustomInput => {
                                Color::srgb(0.20, 0.26, 0.38)
                            }
                            Action::Step | Action::Undo | Action::BackToInput => {
                                Color::srgb(0.22, 0.29, 0.29)
                            }
                            _ => Color::srgb(0.15, 0.17, 0.22),
                        };
                        nice_button(r, font, label, *action, base);
                    }
                });
            }
        });
}

/// 输入界面顶部:6 个**中心色**按钮 —— 选哪个颜色就是选哪个面
#[allow(dead_code)] // 已改为滑动手势切面
pub fn spawn_face_selector(mut commands: Commands, font: Res<crate::UiFont>) {
    use crate::validate::{color_cn, FACE_COLOR};
    commands
        .spawn((
            Node {
                // 顶部居中:点颜色按钮切换要看/要编辑的面
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                column_gap: Val::Px(6.0),
                row_gap: Val::Px(4.0),
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            crate::VisibilityInMode(UiMode::Input),
            FaceSelector,
        ))
        .with_children(|row| {
            for f in 0..6 {
                let c = FACE_COLOR[f];
                let (base, _) = crate::sticker_color_pair(c);
                // 面按钮:底色就是该面中心色 —— 一眼看出"点它切到哪个面"
                let tint = crate::dim(base, 0.55);
                row.spawn((
                    Button,
                    ActionButton(Action::SelectFace(f)),
                    BtnStyle { base: tint, hover: crate::dim(base, 0.9), press: crate::dim(base, 0.35) },
                    Node {
                        padding: UiRect::axes(Val::Px(11.0), Val::Px(5.0)),
                        border: UiRect::top(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(9.0)),
                        ..default()
                    },
                    BorderColor::all(crate::dim(base, 0.75)),
                    BackgroundColor(tint),
                    Text::new(color_cn(c)),
                    TextFont { font: font.0.clone().into(), font_size: FontSize::Px(12.5), ..default() },
                    TextColor(Color::srgb(0.97, 0.98, 1.0)),
                    TextLayout { linebreak: LineBreak::NoWrap, ..default() },
                ));
            }
        });
}

#[derive(Component)]
#[allow(dead_code)]
pub struct FaceSelector;

#[derive(Component)]
pub struct StepHud;

/// 播放界面:按钮上方显示"第几步 / 共几步 + 当前转动"
pub fn spawn_step_hud(mut commands: Commands, font: Res<crate::UiFont>) {
    // 用容器 + flex 居中(TextLayout 的 justify 在无宽度约束时不起作用 ✗)
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(16.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            crate::VisibilityInMode(UiMode::Play),
        ))
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|col| {
                col.spawn((
                    Text::new(""),
                    TextFont { font: font.0.clone().into(), font_size: FontSize::Px(19.0), ..default() },
                    TextColor(Color::srgb(1.0, 0.93, 0.62)),
                    TextLayout { linebreak: LineBreak::NoWrap, ..default() },
                    StepHud,
                ));
            });
        });
}

pub fn update_step_hud(
    view: Res<SolutionView>,
    pb: Res<Playback>,
    mut q: Query<&mut Text, With<StepHud>>,
) {
    let total = view.moves.len();
    let want = if total == 0 {
        if pb.active.is_some() || !pb.queue.is_empty() {
            "打乱播放中…".to_string()
        } else {
            String::new()
        }
    } else {
        // 正在播 → 显示"正在播的那一步";否则 → **保持显示刚播完的那一步**(不再消失 ✗)
        let (step_no, idx) = if pb.active.is_some() {
            (view.cursor + 1, view.cursor)
        } else {
            (view.cursor, view.cursor.saturating_sub(1))
        };
        let notation = view
            .moves
            .get(idx)
            .map(|m| m.notation())
            .unwrap_or_default();
        let angle = view
            .moves
            .get(idx)
            .map(|m| 90.0 * m.quarter_turns_cw() as f32)
            .unwrap_or(0.0);
        let turning = pb
            .active
            .as_ref()
            .map(|a| format!("   已转 {:.0}°", a.turned_deg.abs()))
            .unwrap_or_else(|| {
                if view.cursor >= total && total > 0 {
                    "   ✅ 已完成".to_string()
                } else {
                    "   (待续)".to_string()
                }
            });
        format!(
            "第 {} / {} 步   当前 {}({:.0}°){}",
            step_no.max(1).min(total),
            total,
            notation,
            angle,
            turning
        )
    };
    for mut t in q.iter_mut() {
        if t.0 != want {
            t.0 = want.clone();
        }
    }
}

/// 日志弹窗(默认隐藏,点「日志」才显示)
pub fn spawn_log_panel(mut commands: Commands, font: Res<crate::UiFont>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(12.0),
                top: Val::Px(290.0),
                width: Val::Px(600.0),
                padding: UiRect::all(Val::Px(10.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.08, 0.11, 0.96)),
            LogPanel,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(""),
                TextFont { font: font.0.clone().into(), font_size: FontSize::Px(12.0), ..default() },
                TextColor(Color::srgb(0.8, 0.86, 0.95)),
                TextLayout { linebreak: LineBreak::AnyCharacter, ..default() },
                LogText,
            ));
        });
}

/// 点击按钮 → 产生动作(与键盘同一条路径)
pub fn button_click(
    q: Query<(&Interaction, &ActionButton), Changed<Interaction>>,
    mut pending: ResMut<PendingAction>,
) {
    for (interaction, btn) in q.iter() {
        if *interaction == Interaction::Pressed {
            pending.0 = Some(btn.0);
        }
    }
}

/// 日志弹窗显隐 + 内容刷新
pub fn update_log_panel(
    log: Res<LogBuffer>,
    mut panels: Query<&mut Node, With<LogPanel>>,
    mut texts: Query<&mut Text, With<LogText>>,
) {
    for mut node in panels.iter_mut() {
        let want = if log.open { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    if log.open {
        let want = log.tail(16);
        for mut t in texts.iter_mut() {
            if t.0 != want {
                t.0 = want.clone();
            }
        }
    }
}

/// 把编辑器的提示/错误**送进日志**(而不是直接铺在编辑器面板上)
pub fn log_editor_messages(
    edit: Res<crate::editor::EditState>,
    mut log: ResMut<LogBuffer>,
    mut last: Local<String>,
) {
    if edit.message != *last && !edit.message.is_empty() {
        log.push(edit.message.clone());
        *last = edit.message.clone();
    }
}

// ───────────────────────── HUD 内容 ─────────────────────────

/// 把长解法按行折行显示(每行 12 步),并高亮当前步
fn format_solution(moves: &[cubr_core::model::Move], cursor: usize) -> String {
    if moves.is_empty() {
        return "  (未求解)".to_string();
    }
    let mut out = String::new();
    for (i, m) in moves.iter().enumerate() {
        let tok = if i == cursor {
            format!("[{}]", m.notation())
        } else if i < cursor {
            format!("({})", m.notation())
        } else {
            m.notation()
        };
        out.push_str(&tok);
        out.push(' ');
        if (i + 1) % 12 == 0 {
            out.push_str("\n  ");
        }
    }
    out
}

/// 转动角度文案:90° / 180° / 270°
fn angle_text(deg: f32) -> String {
    let a = deg.abs().round() as i32;
    match a {
        90 => "90°".to_string(),
        180 => "180°".to_string(),
        270 => "270°(反向 90°)".to_string(),
        _ => format!("{a}°"),
    }
}

#[allow(dead_code)]
pub fn update_hud(
    model: Res<Model>,
    pb: Res<Playback>,
    view: Res<SolutionView>,
    worker: Res<SolverWorker>,
    log: Res<LogBuffer>,
    mut q: Query<&mut Text, With<HudText>>,
) {
    let Ok(mut text) = q.single_mut() else { return };

    let state = model.core.to_state();
    let solved = is_solved(&state);

    let solver_line = match &worker.status {
        SolveStatus::Booting(s) => format!("正在初始化:{s}"),
        SolveStatus::Ready => "就绪 —— 点「求解」".to_string(),
        SolveStatus::Solving { elapsed, note } => format!("搜索中  {note}  已用 {elapsed:.1} 秒"),
        SolveStatus::Done { steps, attempts, ms } => {
            format!("已求出 {steps} 步解({attempts} 次尝试 / {ms} 毫秒)")
        }
        SolveStatus::Failed(e) => format!("求解失败:{e}"),
    };

    // 播放行:带上**这一转的角度**与**当前已转角度**
    let play_line = if let Some(a) = pb.active.as_ref() {
        format!(
            "播放中  {}  目标转动 {}  已转 {:.0}°",
            a.mv.notation(),
            angle_text(a.angle.to_degrees()),
            a.turned_deg.abs()
        )
    } else if pb.paused {
        if pb.queue.is_empty() {
            "已暂停".to_string()
        } else {
            format!("已暂停(待播放 {} 步)—— 点「播放」或「单步」", pb.remaining())
        }
    } else if !pb.queue.is_empty() {
        format!("即将播放 {} 步", pb.queue.len())
    } else if solved {
        "已还原".to_string()
    } else {
        "空闲".to_string()
    };

    // 下一步的角度预告
    let next_hint = match pb.queue.first() {
        Some(m) if pb.active.is_none() => {
            let deg = 90.0 * m.quarter_turns_cw() as f32;
            format!("   下一步 {}({})", m.notation(), angle_text(deg))
        }
        _ => String::new(),
    };

    let progress = if view.moves.is_empty() {
        String::new()
    } else {
        format!("   进度 {}/{}", view.cursor.min(view.moves.len()), view.moves.len())
    };

    let body = format!(
        "魔方求解器  |  Bevy 0.19 + cubr-core(Korf 最优 / 两阶段回退)\n\
         ────────────────────────────────────────────\n\
         魔方:{cube_state:<10}  求解器:{solver_line}\n\
         {play_line}{next_hint}{progress}\n\
         \n\
         解法(不超过 20 步,当前步用 [ ] 标出,已完成用 ( ) 标出):\n  {solution}\n\
         \n\
         ────────────────────────────────────────────\n\
         操作都在左下角按钮上(括号内为快捷键)   日志:{log_state}",
        cube_state = if solved { "已还原" } else { "已打乱" },
        solution = format_solution(&view.moves, view.cursor),
        log_state = if log.open { "显示中" } else { "隐藏(点「日志」)" },
    );

    if text.0 != body {
        text.0 = body;
    }
}

/// 截图落盘(按钮与快捷键共用)
pub fn take_screenshot(commands: &mut Commands, n: u32) {
    let path = format!("rubik-{n}.png");
    println!("保存截图 → {path}");
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
}

/// 截图序号
#[derive(Resource, Default)]
pub struct ShotCounter(pub u32);