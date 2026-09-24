//! # 魔方求解器(Bevy 0.19 + cubr-core)
//!
//! 任意初始状态 → **≤20 步**解法 → 3D 动画还原,可暂停 / 单步 / 回退。
//!
//! ## 模块
//! - [`cube`]  把 `cubr-core` 的整数网格模型渲染成实体(27 块 + 贴纸)
//! - [`anim`]  转动动画(旋转轴由模型推导,不猜手性约定)
//! - [`solve`] 后台求解线程 + ≤20 步的两级保证
//! - [`ui`]    左上角 HUD
//!
//! ## 操作
//! | 键 | 作用 |
//! |---|---|
//! | `空格` | 暂停 / 继续 |
//! | `→` / `N` | 单步(暂停时也可用) |
//! | `←` / `P` | 回退一步 |
//! | `S` | 求解当前状态 |
//! | `X` | 随机打乱(25 步) |
//! | `R` | 重置为还原态 |
//! | `A` / `D` | 环绕旋转视角 |
//! | `[` / `]` | 调慢 / 调快动画 |
//! | `Esc` | 退出 |

mod anim;
mod cube;
mod diag;
mod editor;
mod picker;
mod selftest;
mod solve;
mod ui;
mod validate;

use bevy::log::LogPlugin;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::*;
use cubr_core::core::CubeCore;
use cubr_core::model::Move;

use anim::{advance_animation, start_next_move, undo_last, Playback, MAX_DURATION, MIN_DURATION};
use cube::{spawn_cube_entities, CubieIndex};
use solve::{poll_solver, start_worker, SolverWorker};
use editor::{EditState, state_from_cli};
use selftest::SelfTest;

/// 魔方逻辑模型 —— **唯一真值来源**(渲染与动画都是它的投影)
#[derive(Resource)]
pub struct Model {
    pub core: CubeCore,
    /// 已执行过的转动(供回退)
    pub history: Vec<Move>,
}

/// 当前解法的展示状态
#[derive(Resource, Default)]
pub struct SolutionView {
    pub moves: Vec<Move>,
    /// 已执行到第几步(用于高亮当前步)
    pub cursor: usize,
    /// 求解尝试次数(1 = 一次命中)
    pub attempts: u32,
    /// 求解耗时(毫秒)
    pub elapsed_ms: u128,
}

/// 极简 LCG 随机数(避免为一个 demo 引入 rand 依赖)
#[derive(Resource)]
pub struct Rng(pub u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() as usize) % n
    }
}

/// 相机环绕参数
/// 界面状态机:输入 / 播放 —— **两者不共存**(小屏上同时出现编辑器和动画没法操作)
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum UiMode {
    /// 输入:编辑初始状态(单面正交视图 + 取色器 + 打乱/求解)
    #[default]
    Input,
    /// 播放:只看动画(前进 / 后退 / 返回)
    Play,
}

/// 输入态的两种视图:随机打乱后用**播放视角**,自定义输入才用**垂直视角**
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputView {
    /// 垂直正对选中面(仅"自定义输入"时)
    Perpendicular,
    /// 3/4 斜视角 —— **默认就是这个**(与播放一致)
    #[default]
    Perspective,
}

#[derive(Resource, Default)]
pub struct UiState {
    pub mode: UiMode,
    /// 输入界面当前查看的面(0..6 = U R F D L B)
    pub face: usize,
    pub input_view: InputView,
    /// **是否处于"自定义输入"(编辑)状态** —— 只有它为 true 时才允许
    /// 点格子弹取色器、滑动切面;打乱/重置/播放/返回后一律为 false。
    pub editing: bool,
}

/// 所有"操作"统一成动作 —— 键盘与按钮都只负责**产生动作**,
/// 真正的执行集中在 [`apply_actions`],避免两套逻辑走偏。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Solve,
    Scramble,
    Reset,
    TogglePlay,
    Step,
    Undo,
    Slower,
    Faster,
    Screenshot,
    ToggleEditor,
    ToggleLog,
    OrbitLeft,
    OrbitRight,
    Quit,
    /// 选择输入界面上要查看/编辑的面(0..6)
    SelectFace(usize),
    /// 播放界面 → 返回输入界面
    BackToInput,
    /// 输入界面:切到"自定义输入"(垂直视角)
    CustomInput,
}

/// 界面相关的几个资源打包成一个参数 —— `apply_actions` 参数太多会超出 Bevy 的 16 上限
#[derive(bevy::ecs::system::SystemParam)]
pub struct UiCtl<'w> {
    pub ui_state: ResMut<'w, UiState>,
    pub reveal: ResMut<'w, RevealAnim>,
    pub pending: ResMut<'w, PendingPlay>,
    pub toast: ResMut<'w, Toast>,
    pub shots: ResMut<'w, ui::ShotCounter>,
}

/// 待执行动作(键盘/按钮写,`apply_actions` 读)
#[derive(Resource, Default)]
pub struct PendingAction(pub Option<Action>);

/// 界面中文字体(Noto Sans CJK SC 子集,见 assets/fonts/)
#[derive(Resource, Clone)]
pub struct UiFont(pub Handle<Font>);

/// 载入界面字体(必须早于所有 UI 生成系统)。
///
/// 字体**直接嵌进二进制**(`include_bytes!`),所以不需要 assets 目录 ——
/// 单独拷一个 exe 到别处也能正常显示中文。
fn load_ui_font(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    const UI_CJK: &[u8] = include_bytes!("../assets/fonts/ui-cjk.otf");
    let handle = fonts.add(Font::from_bytes(UI_CJK.to_vec()));
    commands.insert_resource(UiFont(handle));
    println!("[font] 已内嵌中文字体 {:.0} KB", UI_CJK.len() as f32 / 1024.0);
}

/// 本次启动是否由外部(`--state` / `RUBIK_STATE`)指定了初始状态
#[derive(Resource, Default)]
pub struct ExternalState(pub bool);

#[derive(Resource)]
pub struct CameraRig {
    pub yaw: f32,
    pub dist: f32,
    pub height: f32,
}

/// 浏览器入口:wasm 模块实例化后自动执行(Bevy 会使用页面里的 <canvas id="game">)
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn wasm_start() {
    main();
}

fn main() {
    // RUBIK_VALIDATE_TEST=1:无图形环境下跑不变量自检后退出
    if std::env::var("RUBIK_VALIDATE_TEST").is_ok() {
        println!("== 魔方状态不变量自检 ==");
        let ok_state = validate::run_selftest();
        println!();
        let ok_anim = diag::run();
        std::process::exit(if ok_state && ok_anim { 0 } else { 1 });
    }

    // RUBIK_INPUT_FUZZ=<次数>:无图形环境下跑「随机打乱 → 逐格输入」回归测试后退出
    //   例:RUBIK_INPUT_FUZZ=1000 cargo run
    //       RUBIK_INPUT_FUZZ=1000 RUBIK_INPUT_FUZZ_SEED=7 cargo run
    if let Ok(v) = std::env::var("RUBIK_INPUT_FUZZ") {
        let trials: usize = v.trim().parse().unwrap_or(200);
        let seed: u64 = std::env::var("RUBIK_INPUT_FUZZ_SEED")
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);
        let ok = editor::run_input_fuzz(trials, seed);
        std::process::exit(if ok { 0 } else { 1 });
    }

    // 外部初始状态:只解析一次(命令行 --state 优先于 RUBIK_STATE)
    let initial = state_from_cli();
    if let Some(st) = &initial {
        println!("[state] 已载入外部初始状态(已通过合法性校验)");
        let _ = st;
    }

    App::new()
        .add_plugins(DefaultPlugins.set(LogPlugin {
            // 兜底:ICU4X 在缺少 CJK 词典数据时会打一条 warn
            // (见 README「ICU4X 警告」)。根因已用 LineBreak::AnyCharacter 规避,
            // 这里再把 icu_provider 的日志静音,避免任何路径下的刷屏。
            filter: "wgpu=error,naga=warn,icu_provider=off,icu_segmenter=off".into(),
            ..default()
        }).set(WindowPlugin {
            primary_window: Some(Window {
                // web:直接用页面里的 <canvas id="game">,不要 Bevy 新建画布
                #[cfg(target_arch = "wasm32")]
                canvas: Some("#game".into()),
                // web:让 canvas 跟随父容器(body)尺寸 —— 否则 Bevy 会把 canvas
                // 内联尺寸设成下面的 resolution(1280x720),覆盖 CSS,于是手机上
                // 画面既不是全屏也不居中。
                #[cfg(target_arch = "wasm32")]
                fit_canvas_to_parent: true,
                // 标题栏由系统合成器绘制,**用的是系统字体,我们无法替换** ——
                // 所以按平台选:Windows 的字体有中文;WSLg/Weston 等环境常常没有,
                // 就退回 ASCII,免得出现方块。
                title: if cfg!(target_os = "windows") {
                    "魔方求解器 — Bevy 0.19 + cubr-core(20 步内 · 可暂停)"
                } else {
                    "Rubik's Cube Solver - Bevy 0.19 + cubr-core (<=20 moves, pausable)"
                }
                .into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource({
            let mut core = CubeCore::solved();
            if let Some(st) = &initial {
                core.paint(st);
            }
            Model { core, history: Vec::new() }
        })
        .insert_resource(EditState::default())
        .insert_resource(editor::HintPulse::default())
        .insert_resource(ui::LogBuffer::default())
        .insert_resource(ui::ShotCounter::default())
        .insert_resource(PendingAction::default())
        .insert_resource(UiState::default())
        .insert_resource(CamAnim::default())
        .insert_resource(RevealAnim::default())
        .insert_resource(PendingPlay::default())
        .insert_resource(Toast::default())
        .insert_resource(ScrambleSeq::default())
        .insert_resource(ExternalState(initial.is_some()))
        .insert_resource(SolutionView::default())
        .insert_resource(Rng(0x2545_F491_4F6C_DD1D))
        .insert_resource(CameraRig { yaw: 0.62, dist: 10.5, height: 6.4 })
        .insert_resource(Playback::default())
        // 浏览器(WebGL2)上平行光的表现与桌面不完全一致,环境光给足兜底,避免发黑
        .insert_resource(GlobalAmbientLight {
            brightness: if cfg!(target_arch = "wasm32") { 900.0 } else { 130.0 },
            ..default()
        })
        .insert_resource(start_worker())
        // RUBIK_SELFTEST=<路径> 时进入无人值守自检模式(打乱→求解→播放→截图→退出)
        .add_systems(Startup, setup_selftest)
        // 字体必须在各 spawn 之前插好 ⇒ 用 PreStartup/Startup 两个阶段,
        // 不依赖 .chain()(它与 Bevy 曲线的 chain 方法同名,会误解析)
        .add_systems(PreStartup, load_ui_font)
        .add_systems(
            Startup,
            (
                setup_scene,
                spawn_cube_entities,
                ui::spawn_controls,
                picker::spawn_picker,
                ui::spawn_log_panel,
                spawn_toast,
                ui::spawn_step_hud,
            ),
        )
        // 调试:把几格标成冲突,检查品红边框渲染(RUBIK_DEBUG_CONFLICT=1)
        .add_systems(
            Startup,
            editor::debug_mark_conflict
                .run_if(|| std::env::var("RUBIK_DEBUG_CONFLICT").is_ok()),
        )
        // 定时截图(RUBIK_SHOT_AT=<秒> RUBIK_SHOT_PATH=<路径>)
        .add_systems(Startup, selftest::setup_shot_at)
        // 调试:把几格标成冲突,检查品红边框渲染(RUBIK_DEBUG_CONFLICT=1)
        .add_systems(
            Startup,
            editor::debug_mark_conflict
                .run_if(|| std::env::var("RUBIK_DEBUG_CONFLICT").is_ok()),
        )
        // ── Update:拆成若干小分组(每个 ≤6 个系统,避免 Bevy 元组上限与 chain 歧义)──
        .add_systems(Update, keyboard_input)
        .add_systems(
            Update,
            (ui::button_click, ui::button_feedback, ui::scale_buttons, ui::update_step_hud),
        )
        .add_systems(Update, apply_actions)
        .add_systems(Update, (poll_solver, start_next_move, advance_animation))
        .add_systems(
            Update,
            (
                cube::sync_sticker_materials,
                sync_input_cube,
                fit_ui_scale,
                animate_camera,
                follow_light,
                run_scramble_seq,
                tick_reveal,
            ),
        )
        .add_systems(
            Update,
            (cube_view, cube_pick, apply_mode_visibility, orbit_camera, swipe_turn),
        )
        .add_systems(
            Update,
            (
                start_pending_play,
                readback_after_scramble,
                update_toast,
                ui::update_log_panel,
            ),
        )
        .add_systems(
            Update,
            (
                editor::cell_click,
                editor::palette_click,
                editor::eraser_click,
                editor::repair_click,
                editor::editor_buttons,
                editor::refresh_cells,
            ),
        )
        .add_systems(
            Update,
            (
                editor::sync_visibility,
                editor::layout_single_face,
                editor::pulse_hints,
                editor::update_status,
                picker::apply_picker,
            ),
        )
        // 取色器点击必须在所有底层点击系统之后处理；否则 Bevy 可能并行
        // 执行系统，关闭取色器的同一指针事件会继续落到底层控件。
        .add_systems(
            Update,
            picker::picker_click
                .after(editor::cell_click)
                .after(editor::palette_click)
                .after(cube_pick)
                .after(swipe_turn),
        )
        // 取色器等"指针抬起"才真正关闭(点击穿透的根治,见 picker::finish_close);
        // 环境光按视角切换:垂直单面视图要亮而均匀,3/4 视角要保留立体感。
        .add_systems(Update, (picker::finish_close, tune_ambient_for_view))
        .add_systems(Update, ui::log_editor_messages)
        .add_systems(
            Update,
            (
                selftest::selftest_system.run_if(resource_exists::<SelfTest>),
                selftest::shot_at_system.run_if(resource_exists::<selftest::ShotAt>),
                apply_demo.run_if(|| std::env::var("RUBIK_APPLY_DEMO").is_ok()),
                clear_demo.run_if(|| std::env::var("RUBIK_EDITOR_CLEAR").is_ok()),
                open_log_demo.run_if(|| std::env::var("RUBIK_LOG_OPEN").is_ok()),
            ),
        )
        .add_systems(
            Update,
            (
                picker_demo.run_if(|| std::env::var("RUBIK_PICKER_DEMO").is_ok()),
                play_demo.run_if(|| std::env::var("RUBIK_PLAY_DEMO").is_ok()),
                undo_demo.run_if(|| std::env::var("RUBIK_UNDO_DEMO").is_ok()),
            ),
        )
        .run();
}

/// 若设置了 `RUBIK_SELFTEST`,插入自检资源
fn setup_selftest(mut commands: Commands) {
    if let Ok(path) = std::env::var("RUBIK_SELFTEST") {
        println!("[selftest] 已启用自检模式,截图将写入 {path}");
        commands.insert_resource(SelfTest::new(path));
    }
}



/// 相机与光照
fn setup_scene(mut commands: Commands) {
    let (yaw, dist, height) = (0.62_f32, 10.5_f32, 6.4_f32);
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(yaw.sin() * dist, height, yaw.cos() * dist)
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // 主光:跟随相机左上方,负责明暗与立体感(位置由 follow_light 每帧更新)
    commands.spawn((
        KeyLight,
        DirectionalLight {
            illuminance: 5_800.0,
            // 手机 GPU 上阴影很贵:wasm(浏览器/移动端)默认关掉
            shadow_maps_enabled: !cfg!(target_arch = "wasm32"),
            ..default()
        },
        Transform::from_xyz(7.0, 13.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // 补光:跟随相机右下方,专门把右侧/下侧的面提亮,避免死黑(不投影,代价很低)。
    // ⚠️ WebGL2 设备通常**只支持 1 盏平行光**(多出来的会被丢弃并告警),
    //    浏览器版改为不生成补光,靠"主光靠近相机轴 + 提高环境光"达到同样效果。
    #[cfg(not(target_arch = "wasm32"))]
    commands.spawn((
        FillLight,
        DirectionalLight { illuminance: 2_200.0, shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(-6.0, -4.0, 9.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// 生成 25 步随机打乱(避免连续转同一面)
fn random_scramble(rng: &mut Rng, len: usize) -> Vec<Move> {
    let mut seq = Vec::with_capacity(len);
    // 注意:axis() 返回的是 cubr-core 那份 glam 的 IVec3,与 Bevy 的不是同一类型,
    // 所以这里转成 (i32,i32,i32) 再比较。
    let mut last = (99, 99, 99);
    while seq.len() < len {
        let m = Move::ALL[rng.below(Move::ALL.len())];
        let a = m.axis();
        let cur = (a.x, a.y, a.z);
        if cur == last {
            continue; // 同一面连续转没有意义 → 重抽
        }
        last = cur;
        seq.push(m);
    }
    seq
}

/// 键盘:只负责把按键翻译成 [`Action`]
fn keyboard_input(keys: Res<ButtonInput<KeyCode>>, mut pending: ResMut<PendingAction>) {
    if pending.0.is_some() {
        return; // 本帧已有动作待执行
    }
    let mut push = |a: Action| pending.0 = Some(a);
    if keys.just_pressed(KeyCode::Space) {
        push(Action::TogglePlay);
    } else if keys.just_pressed(KeyCode::ArrowRight) || keys.just_pressed(KeyCode::KeyN) {
        push(Action::Step);
    } else if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::KeyP) {
        push(Action::Undo);
    } else if keys.just_pressed(KeyCode::KeyS) {
        push(Action::Solve);
    } else if keys.just_pressed(KeyCode::KeyX) {
        push(Action::Scramble);
    } else if keys.just_pressed(KeyCode::KeyR) {
        push(Action::Reset);
    } else if keys.just_pressed(KeyCode::KeyE) {
        push(Action::ToggleEditor);
    } else if keys.just_pressed(KeyCode::KeyC) {
        push(Action::CustomInput);
    } else if keys.just_pressed(KeyCode::KeyB) {
        push(Action::BackToInput);
    } else if keys.just_pressed(KeyCode::KeyL) {
        push(Action::ToggleLog);
    } else if keys.just_pressed(KeyCode::F12) {
        push(Action::Screenshot);
    } else if keys.just_pressed(KeyCode::Escape) {
        push(Action::Quit);
    } else if keys.pressed(KeyCode::KeyA) {
        push(Action::OrbitLeft);
    } else if keys.pressed(KeyCode::KeyD) {
        push(Action::OrbitRight);
    } else if keys.pressed(KeyCode::BracketLeft) {
        push(Action::Slower);
    } else if keys.pressed(KeyCode::BracketRight) {
        push(Action::Faster);
    }
}

/// **动作执行**:键盘与按钮共用这一条路径
#[allow(clippy::too_many_arguments)]
fn apply_actions(
    mut pending: ResMut<PendingAction>,
    mut model: ResMut<Model>,
    mut pb: ResMut<Playback>,
    mut view: ResMut<SolutionView>,
    mut worker: ResMut<SolverWorker>,
    mut rng: ResMut<Rng>,
    mut rig: ResMut<CameraRig>,
    mut log: ResMut<ui::LogBuffer>,
    mut edit: ResMut<editor::EditState>,
    mut ctl: UiCtl,
    mut seq_state: ResMut<ScrambleSeq>,
    mut commands: Commands,
    mut q: Query<(Entity, &CubieIndex, &mut Transform)>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(action) = pending.0.take() else { return };
    match action {
        Action::TogglePlay => {
            pb.paused = !pb.paused;
            if !pb.paused {
                pb.step_budget = 0;
            }
            log.push(if pb.paused { "已暂停" } else { "开始播放" });
        }
        Action::Step => {
            pb.step_budget += 1;
            pb.paused = true; // 单步后保持暂停
        }
        Action::Undo => {
            undo_last(&mut model, &mut pb, &mut view, &mut q);
            log.push("回退一步");
        }
        Action::Solve => {
            // 自定义输入必须**填完整**才能求解;否则给一个显眼的提示
            let left = validate::empty_count(&edit.cells);
            if left > 0 {
                ctl.toast.text = format!("还有 {left} 格没填 —— 请把魔方填完整再求解");
                ctl.toast.t = 3.0;
                log.push(format!("求解被拒:还有 {left} 格未填"));
                return;
            }
            if let Err(e) = validate::validate(&edit.cells) {
                let why = e.first().cloned().unwrap_or_else(|| "状态不合法".into());
                ctl.toast.text = format!("当前状态不合法:{why}");
                ctl.toast.t = 3.0;
                log.push(format!("求解被拒:{why}"));
                return;
            }
            if worker.is_ready() {
                // 先把编辑器里的盘面**应用**到模型(否则求解用的还是旧状态)
                editor::apply_to_model(&mut edit, &mut model, &mut pb, &mut view, &mut q);
                // 需求:从自定义输入点求解时,先转回到 3D 视角
                ctl.ui_state.input_view = InputView::Perspective;
                ctl.ui_state.editing = false; // 求解 = 离开编辑状态
                worker.request(model.core.to_state());
                // 需求:点「求解」后自动进入播放界面
                ctl.ui_state.mode = UiMode::Play;
                log.push("已提交求解请求 —— 切到播放界面");
            } else {
                log.push("求解器还在初始化,稍候再试");
            }
        }
        Action::CustomInput => {
            ctl.ui_state.mode = UiMode::Input;
            ctl.ui_state.input_view = InputView::Perpendicular;
            ctl.ui_state.editing = true; // ← 进入编辑状态(点格子/滑动才生效)
            // 需求:**按下自定义输入就默认清空所有颜色**(只留中心块)
            // 这样任意格子想填什么颜色都行,不会出现"点了没反应"。
            ctl.ui_state.face = 0;
            // 需求:先**重置**为各面同色,再 clear;
            // 颜色变化 = 从边缘往中心收缩,最后只剩中心那一格
            edit.reset_to_solid();
            *ctl.reveal = RevealAnim { t: 0.0, active: true, outward: false, dur: 1.1 };
            edit.clear();
            log.push("自定义输入:已清空(只留中心块),点格子选颜色");
        }
        Action::Scramble => {
            worker.cancel();
            let seq = random_scramble(&mut rng, 25);
            view.moves.clear();
            view.cursor = 0;
            view.attempts = 0;
            view.elapsed_ms = 0;
            // 需求:先**重置**(各面同色,要能看见)→ 再 clear + 扩散 → 最后才开始转
            ctl.ui_state.mode = UiMode::Input;
            ctl.ui_state.input_view = InputView::Perspective; // 相机转到播放视角(带动画)
            ctl.ui_state.editing = false; // 打乱 = 回到主界面,不允许编辑/滑动
            model.core = CubeCore::solved();
            model.history.clear();
            pb.queue.clear();
            pb.active = None;
            edit.reset_to_solid(); // ① 立刻重置
            seq_state.stage = 1;
            seq_state.t = 0.0;
            seq_state.moves = seq;
            ctl.pending.armed = false;
            ctl.pending.readback = false;
            log.push("随机打乱:先重置 → 清空 → 从中心扩散 → 开始转动");
        }
        Action::Reset => {
            worker.cancel();
            // **幂等**:不论之前是半成品/打乱/清空,结果永远是"各面同色"
            model.core = CubeCore::solved();
            model.history.clear();
            pb.queue.clear();
            pb.active = None;
            pb.paused = false;
            pb.step_budget = 0;
            view.moves.clear();
            view.cursor = 0;
            cube::sync_transforms3(&model.core, &mut q);
            // ⚠️ 编辑器也必须一起重置,否则 sync_input_cube 会用旧盘面把模型覆盖回去
            edit.reset_to_solid();
            ctl.ui_state.input_view = InputView::Perspective;
            ctl.ui_state.editing = false; // 重置 = 回到主界面
            ctl.toast.text = "已重置为各面同色".into();
            ctl.toast.t = 2.0;
            log.push("已重置(各面同色,幂等)");
        }
        Action::Slower => {
            pb.duration = (pb.duration + 0.05).min(MAX_DURATION);
        }
        Action::Faster => {
            pb.duration = (pb.duration - 0.05).max(MIN_DURATION);
        }
        Action::OrbitLeft => rig.yaw -= 0.04,
        Action::OrbitRight => rig.yaw += 0.04,
        Action::ToggleEditor => {
            edit.visible = !edit.visible;
            log.push(if edit.visible { "显示状态编辑器" } else { "隐藏状态编辑器" });
        }
        Action::ToggleLog => {
            log.open = !log.open;
        }
        Action::SelectFace(f) => {
            ctl.ui_state.face = f.min(5);
            edit.message = format!("查看 {} 面(中心色 {})", "上右前下左后".chars().nth(ctl.ui_state.face).unwrap(), validate::color_cn(validate::FACE_COLOR[ctl.ui_state.face]));
        }
        Action::BackToInput => {
            ctl.ui_state.mode = UiMode::Input;
            ctl.ui_state.input_view = InputView::Perspective;
            ctl.ui_state.editing = false; // 回到主界面(要编辑再点「自定义输入」)
            log.push("返回输入界面");
        }
        Action::Screenshot => {
            // wasm 上没有文件系统:`save_to_disk` 会失败/panic,直接改为提示
            if cfg!(target_arch = "wasm32") {
                log.push("浏览器版不支持截图(请用系统截图)");
            } else {
                let n = ctl.shots.0;
                ctl.shots.0 += 1;
                ui::take_screenshot(&mut commands, n);
                log.push(format!("已保存截图 rubik-{n}.png"));
            }
        }
        Action::Quit => {
            exit.write(AppExit::Success);
        }
    }
}

/// 调试:把编辑器设为已知打乱态,并走 **Apply 的真实路径**(用于验证 3D 是否同步)
/// 用 `Local<bool>` 保证只在第一帧执行一次(即"实体都建好之后")
fn apply_demo(
    mut once: Local<bool>,
    mut edit: ResMut<editor::EditState>,
    mut model: ResMut<Model>,
    mut pb: ResMut<Playback>,
    mut view: ResMut<SolutionView>,
    mut q: Query<(Entity, &CubieIndex, &mut Transform)>,
) {
    if *once {
        return;
    }
    *once = true;
    let mut core = CubeCore::solved();
    for t in "R U2 F' L D B2 R' F U D2".split_whitespace() {
        core.apply(cubr_core::model::Move::parse(t).unwrap());
    }
    edit.from_cube(&core.to_state());
    editor::apply_to_model(&mut edit, &mut model, &mut pb, &mut view, &mut q);
    println!("[demo] 已走 Apply 路径应用打乱态 —— {}", edit.message);
    println!("[demo] 模型状态串(前 18 格):{:?}", model.core.to_state().U);
}

/// 调试:验证"播放过程中随时可回退"
/// 时间线:0.3s 打乱 → 0.9/1.7/2.5s 各回退一次(此时打乱还在播)
#[allow(clippy::too_many_arguments)]
fn undo_demo(
    time: Res<Time>,
    mut t: Local<f32>,
    mut fired: Local<u32>,
    mut pending: ResMut<PendingAction>,
    model: Res<Model>,
    pb: Res<Playback>,
    mut log: ResMut<ui::LogBuffer>,
) {
    *t += time.delta_secs();
    let step = *fired;
    let due = match step {
        0 => 0.3,
        1 => 0.9,
        2 => 1.7,
        3 => 2.5,
        _ => return,
    };
    if *t < due {
        return;
    }
    *fired += 1;
    if step == 0 {
        pending.0 = Some(Action::Scramble);
        log.push("[演示] 打乱 25 步(开始播放)");
    } else {
        pending.0 = Some(Action::Undo);
        log.push(format!(
            "[演示] 第 {step} 次回退 —— 此刻 history={} queue={} 正在播={}",
            model.history.len(),
            pb.queue.len(),
            pb.active.is_some()
        ));
    }
}

/// 调试:直接进入播放态(验证"编辑与动画不共存")
fn play_demo(
    mut once: Local<bool>,
    mut ui: ResMut<UiState>,
    mut view: ResMut<SolutionView>,
    mut pb: ResMut<Playback>,
) {
    if *once {
        return;
    }
    *once = true;
    ui.mode = UiMode::Play;
    // 造一份假解法,用于验证"第几步"指示器(真实流程由求解器填充)
    view.moves = (0..20).map(|i| cubr_core::model::Move::ALL[i % 18]).collect();
    view.cursor = 7;
    pb.enqueue(vec![cubr_core::model::Move::ALL[2]]);
    pb.paused = true;
    pb.step_budget = 1;
}

/// 调试:直接打开六边形取色器(用于截图验证)
fn picker_demo(mut once: Local<bool>, mut edit: ResMut<editor::EditState>, mut ui: ResMut<UiState>) {
    if *once {
        return;
    }
    *once = true;
    ui.face = 2; // 前面
    edit.picker = Some(2 * 9 + 0); // 前面的左上角(普通格)
}

/// 调试:启动就打开日志弹窗
fn open_log_demo(mut once: Local<bool>, mut log: ResMut<ui::LogBuffer>) {
    if *once {
        return;
    }
    *once = true;
    log.open = true;
    log.push("这是一条示例日志:所有操作与提示都会记录在这里");
    log.push("点左下角「日志 (L)」可以随时开关这个弹窗");
}

/// 调试:清空编辑器盘面(只剩中心),便于观察"可填位置"的闪烁绿框
fn clear_demo(mut once: Local<bool>, mut pending: ResMut<PendingAction>) {
    if *once {
        return;
    }
    *once = true;
    // 走动作层 ⇒ 会带上相机过渡 + 颜色收缩动画
    pending.0 = Some(Action::CustomInput);
    println!("[demo] 触发自定义输入(含相机与颜色动画)");
}

/// 小屏幕(手机)适配:Bevy 界面是固定像素布局,按窗口宽度整体缩放,
/// 否则 390px 宽的手机上右侧编辑器与左下按钮会挤出屏幕。
fn fit_ui_scale(
    window: Query<&Window>,
    mut scale: ResMut<UiScale>,
    mut last_w: Local<f32>,
) {
    let Ok(w) = window.single().map(|win| win.width()) else { return };
    if (w - *last_w).abs() < 1.0 {
        return; // 尺寸没变就不折腾
    }
    *last_w = w;
    // 1280 宽时 = 1.0;窄屏按比例缩小;下限 0.42 保证还能看清
    // (RUBIK_UI_SCALE 可强制指定,便于在 PC 上验证手机布局)
    let want = match std::env::var("RUBIK_UI_SCALE").ok().and_then(|v| v.parse::<f32>().ok()) {
        Some(v) => v.clamp(0.2, 2.0),
        None => (w / 1280.0).clamp(0.42, 1.0),
    };
    if (scale.0 - want).abs() > 0.005 {
        scale.0 = want;
    }
}

/// 输入态:把**编辑器盘面画到魔方上**(空格显示为该面中心色 = 纯色面)。
/// 这是"直接在魔方上操作"的关键:两套数据(编辑器 / 模型)必须联动,
/// 否则会出现"点了自定义输入,颜色没清空"的错觉。
fn sync_input_cube(
    ui_state: Res<UiState>,
    edit: Res<editor::EditState>,
    pending: Res<PendingPlay>,
    pb: Res<Playback>,
    mut model: ResMut<Model>,
    mut last: Local<Option<validate::Cells>>,
) {
    if ui_state.mode != UiMode::Input || !ui_state.editing {
        return;
    }
    // ⚠️ 打乱排队中 / 正在播放时**不要**用编辑器覆盖模型 ——
    //    否则"随机之前先重置"会在同一帧被旧盘面覆盖掉(曾经的 bug)。
    if pending.armed || pending.readback || pb.active.is_some() || !pb.queue.is_empty() {
        return;
    }
    if last.as_ref() == Some(&edit.cells) {
        return;
    }
    *last = Some(edit.cells);
    let st = validate::to_cube_state_lenient(&edit.cells);
    model.core.paint(&st);
    model.history.clear();
}

/// 相机过渡动画:切视角/切面时**转动+平移**过去,而不是瞬移
#[derive(Resource)]
pub struct CamAnim {
    // 用**球坐标**插值(yaw/pitch/dist)⇒ 相机绕着魔方转,而不是直线穿过去
    pub from_yaw: f32,
    pub from_pitch: f32,
    pub from_dist: f32,
    pub to_yaw: f32,
    pub to_pitch: f32,
    pub to_dist: f32,
    pub t: f32,
    pub active: bool,
    pub dur: f32,
}

/// 由球坐标算相机位置(始终看向原点)
fn orbit_pos(yaw: f32, pitch: f32, dist: f32) -> Vec3 {
    Vec3::new(
        dist * pitch.cos() * yaw.sin(),
        dist * pitch.sin(),
        dist * pitch.cos() * yaw.cos(),
    )
}

impl Default for CamAnim {
    fn default() -> Self {
        Self {
            from_yaw: 0.0,
            from_pitch: 0.0,
            from_dist: 1.0,
            to_yaw: 0.0,
            to_pitch: 0.0,
            to_dist: 1.0,
            t: 1.0,
            active: false,
            dur: 0.9,
        }
    }
}

/// 打乱的**分段顺序**:① 重置(可见)→ ② clear + 颜色从内到外扩 → ③ 开始转动
#[derive(Resource, Default)]
pub struct ScrambleSeq {
    pub stage: u8, // 0=空闲 1=已重置待展示 2=已clear待扩散 3=待开转
    pub t: f32,
    pub moves: Vec<cubr_core::model::Move>,
}

fn run_scramble_seq(
    time: Res<Time>,
    mut st: ResMut<ScrambleSeq>,
    mut edit: ResMut<editor::EditState>,
    mut reveal: ResMut<RevealAnim>,
    mut pending: ResMut<PendingPlay>,
    mut log: ResMut<ui::LogBuffer>,
) {
    if st.stage == 0 {
        return;
    }
    st.t += time.delta_secs();
    match st.stage {
        // ① 先把"各面同色"露一下脸(用户要看到重置这一步)
        1 if st.t >= 0.45 => {
            edit.clear();
            *reveal = RevealAnim { t: 0.0, active: true, outward: true, dur: 1.2 };
            st.stage = 2;
            st.t = 0.0;
        }
        // ② 颜色扩散完成后才真正开始随机转动
        2 if st.t >= 1.35 => {
            pending.moves = std::mem::take(&mut st.moves);
            pending.armed = true;
            pending.readback = false;
            st.stage = 0;
            log.push("打乱开始(已先重置并清空)");
        }
        _ => {}
    }
}

/// 待播放的打乱序列:等**相机到位 + 颜色填好**之后再开始转
#[derive(Resource, Default)]
pub struct PendingPlay {
    pub moves: Vec<cubr_core::model::Move>,
    pub armed: bool,
    /// 打乱播完后要把结果回读到编辑器(否则编辑的还是打乱前的状态)
    pub readback: bool,
}

fn start_pending_play(
    cam: Res<CamAnim>,
    reveal: Res<RevealAnim>,
    mut pending: ResMut<PendingPlay>,
    mut pb: ResMut<Playback>,
) {
    if !pending.armed || cam.active || reveal.active {
        return; // 先转到位、颜色填好,再开转
    }
    pending.armed = false;
    pending.readback = true;
    let mv = std::mem::take(&mut pending.moves);
    pb.enqueue(mv);
    pb.paused = false;
}

/// 打乱播完 → 回读为可编辑状态
fn readback_after_scramble(
    mut pending: ResMut<PendingPlay>,
    pb: Res<Playback>,
    model: Res<Model>,
    mut edit: ResMut<editor::EditState>,
) {
    if !pending.readback || pb.active.is_some() || !pb.queue.is_empty() {
        return;
    }
    pending.readback = false;
    edit.from_cube(&model.core.to_state());
    edit.dirty = true;
}

/// 颜色"扩散/收缩"动画:自定义输入 ⇄ 打乱/3D 展示时,
/// 颜色按**离面心的距离**依次出现(从中心往外)或消失(从边框往中间收缩)。
#[derive(Resource, Default)]
pub struct RevealAnim {
    pub t: f32,
    pub active: bool,
    /// true = 从中心往外填回来;false = 从边框向中间收缩
    pub outward: bool,
    pub dur: f32,
}

impl RevealAnim {
    /// 某个贴纸此刻是否已经"显色"(ring = 离面心距离 0/1/√2)
    pub fn shown(&self, ring: f32) -> bool {
        if !self.active {
            return true;
        }
        const MAXD: f32 = 1.4143;
        if self.outward {
            // 从中心往外**出现**:面心最先(k=0),角最后(k=1)
            self.t >= ring / MAXD
        } else {
            // 从边框向中间**收缩**:角最先消失(k=0),面心最后(k=1)
            self.t < (MAXD - ring) / MAXD
        }
    }
}

fn tick_reveal(time: Res<Time>, mut r: ResMut<RevealAnim>) {
    if !r.active {
        return;
    }
    r.t = (r.t + time.delta_secs() / r.dur.max(0.05)).min(1.0);
    if r.t >= 1.0 {
        r.active = false;
    }
}

/// 平行光跟随相机:相机转到哪一面,光就从哪个方向照过去
/// (原来光是世界坐标固定的 ⇒ 背面的面转过来是黑的 ✗)
fn follow_light(
    ui_state: Res<UiState>,
    cam: Query<&Transform, (With<Camera3d>, Without<DirectionalLight>)>,
    mut key: Query<&mut Transform, (With<KeyLight>, Without<FillLight>, Without<Camera3d>)>,
    mut fill: Query<&mut Transform, (With<FillLight>, Without<KeyLight>, Without<Camera3d>)>,
) {
    let Ok(c) = cam.single() else { return };
    // 用**相机自身**的右/上轴构造偏移 —— 每个面看到的光方向一致。
    let back = c.translation.normalize_or_zero(); // 魔方 → 相机
    let right = c.right().normalize_or_zero();
    let up = c.up().normalize_or_zero();

    // ★两种视角用**两套不同方向的光**:
    // - 垂直单面视图(自定义输入):目的是"把贴纸颜色认准",所以光几乎沿视线正射
    //   (headlight),整面亮度均匀 —— 没有斜射造成的半面偏暗/高光,取色更可靠;
    // - 3/4 斜视角(默认/播放):左上方斜射 + 右下方补光,塑造立体感。
    // wasm(WebGL2)只支持一盏平行光 ⇒ 单灯也要偏轴,保证任何朝向的可见面都不发黑。
    let perpendicular =
        ui_state.mode == UiMode::Input && matches!(ui_state.input_view, InputView::Perpendicular);

    let key_dir = if perpendicular {
        // 正对 + 极小偏移:留一点点方向性,避免完全平光看着"糊"
        (back * 0.985 + up * 0.12 - right * 0.09).normalize_or_zero() * 12.0
    } else if cfg!(target_arch = "wasm32") {
        (back * 0.94 + up * 0.24 - right * 0.20).normalize_or_zero() * 12.0
    } else {
        (back * 0.70 + up * 0.52 - right * 0.48).normalize_or_zero() * 12.0
    };
    let key_want = Transform::from_translation(key_dir).looking_at(Vec3::ZERO, Vec3::Y);
    for mut l in key.iter_mut() {
        if l.translation.distance(key_want.translation) > 0.01 {
            *l = key_want;
        }
    }

    let fill_dir = if perpendicular {
        // 垂直视图:补光从另一侧近轴轻补,消掉残余的方向性渐变
        (back * 0.99 - up * 0.10 + right * 0.08).normalize_or_zero() * 12.0
    } else {
        (back * 0.55 - up * 0.35 + right * 0.72).normalize_or_zero() * 12.0
    };
    let fill_want = Transform::from_translation(fill_dir).looking_at(Vec3::ZERO, Vec3::Y);
    for mut l in fill.iter_mut() {
        if l.translation.distance(fill_want.translation) > 0.01 {
            *l = fill_want;
        }
    }
}

/// 环境光随视角调整:垂直单面视图提亮(减少方向性明暗、颜色更接近真实贴纸),
/// 3/4 视角恢复基础值(保留立体感)。wasm 基础值本来就高,按比例提。
fn tune_ambient_for_view(ui_state: Res<UiState>, mut amb: ResMut<GlobalAmbientLight>) {
    let base = if cfg!(target_arch = "wasm32") { 900.0 } else { 130.0 };
    let perpendicular =
        ui_state.mode == UiMode::Input && matches!(ui_state.input_view, InputView::Perpendicular);
    let want = if perpendicular { base * 1.9 } else { base };
    if (amb.brightness - want).abs() > 0.5 {
        amb.brightness = want;
    }
}

/// 每帧推进相机过渡(smoothstep 缓动)
fn animate_camera(
    time: Res<Time>,
    mut anim: ResMut<CamAnim>,
    // 与 follow_light 的 Transform 查询必须互斥,否则 Bevy 报 B0001
    mut q: Query<&mut Transform, (With<Camera3d>, Without<DirectionalLight>)>,
) {
    if !anim.active {
        return;
    }
    anim.t = (anim.t + time.delta_secs() / anim.dur.max(0.05)).min(1.0);
    let k = anim.t * anim.t * (3.0 - 2.0 * anim.t); // smoothstep:两端速度为 0
    // 三个球坐标分量同步插值 ⇒ 相机沿球面"水平/垂直旋转"过去
    let yaw = anim.from_yaw + (anim.to_yaw - anim.from_yaw) * k;
    let pitch = anim.from_pitch + (anim.to_pitch - anim.from_pitch) * k;
    let dist = anim.from_dist + (anim.to_dist - anim.from_dist) * k;
    if let Ok(mut tf) = q.single_mut() {
        *tf = Transform::from_translation(orbit_pos(yaw, pitch, dist)).looking_at(Vec3::ZERO, Vec3::Y);
    }
    if anim.t >= 1.0 {
        anim.active = false;
    }
}

/// **3D(播放)视角的相机位姿 —— 输入态与播放态共用同一组值**,切换时不会有跳变
pub const CAM_3D_POS: Vec3 = Vec3::new(6.5, 6.4, 8.2);
pub const CAM_3D_LOOK: Vec3 = Vec3::new(0.0, 0.6, 0.0);

/// 顶部居中的临时提示(比日志显眼,不藏在弹窗里)
#[derive(Resource, Default)]
pub struct Toast {
    pub text: String,
    pub t: f32,
}
#[derive(Component)]
pub struct ToastText;

pub fn spawn_toast(mut commands: Commands, font: Res<UiFont>) {
    commands.spawn((
        Text::new(""),
        TextFont { font: font.0.clone().into(), font_size: FontSize::Px(17.0), ..default() },
        TextColor(Color::srgb(1.0, 0.85, 0.45)),
        TextLayout { linebreak: LineBreak::AnyCharacter, ..default() },
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(64.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            display: Display::None,
            justify_content: JustifyContent::Center,
            ..default()
        },
        ToastText,
    ));
}

pub fn update_toast(
    time: Res<Time>,
    mut toast: ResMut<Toast>,
    mut q: Query<(&mut Text, &mut Node), With<ToastText>>,
) {
    if toast.t > 0.0 {
        toast.t -= time.delta_secs();
        if toast.t <= 0.0 {
            toast.text.clear();
        }
    }
    for (mut t, mut node) in q.iter_mut() {
        let want = if toast.t > 0.0 { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
        if t.0 != toast.text {
            t.0 = toast.text.clone();
        }
    }
}

#[derive(Component)]
pub struct KeyLight;
#[derive(Component)]
pub struct FillLight;

/// 六个面的法线(U R F D L B)
const FACE_NORMAL: [Vec3; 6] = [
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(1.0, 0.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
    Vec3::new(0.0, -1.0, 0.0),
    Vec3::new(-1.0, 0.0, 0.0),
    Vec3::new(0.0, 0.0, -1.0),
];

/// 输入态:相机改为**正交**并正对选中面(这就是"每次展示一面,正交显示");
/// 播放态:回到透视相机 + 环绕视角。
fn cube_view(
    ui_state: Res<UiState>,
    mut anim: ResMut<CamAnim>,
    mut last: Local<Option<(UiMode, usize, InputView)>>,
    cam: Query<&Transform, With<Camera3d>>,
) {
    let key = (ui_state.mode, ui_state.face, ui_state.input_view);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);

    let (to_pos, _to_look) = match ui_state.mode {
        UiMode::Play => (CAM_3D_POS, CAM_3D_LOOK),
        UiMode::Input => {
            const DIST: f32 = 11.5;
            let n = FACE_NORMAL[ui_state.face.min(5)];
            match ui_state.input_view {
                // 自定义输入:沿面法线正对 ⇒ 该面是正方形,其余面侧对不可见
                InputView::Perpendicular => (n * DIST, Vec3::ZERO),
                // 默认/打乱后:与播放一致的 3/4 视角
                InputView::Perspective => (CAM_3D_POS, CAM_3D_LOOK),
            }
        }
    };

    // 当前球坐标(从相机实际位置反解)
    let cur = cam.single().map(|t| t.translation).unwrap_or(to_pos);
    let cur_dist = cur.length().max(0.001);
    let from_yaw = cur.x.atan2(cur.z);
    let from_pitch = (cur.y / cur_dist).asin();

    // 目标球坐标(由目标位置反解)
    let to_dist = to_pos.length();
    let mut to_yaw = to_pos.x.atan2(to_pos.z);
    let to_pitch = (to_pos.y / to_dist.max(0.001)).asin();
    // yaw 取**最短旋转方向**(避免绕远路)
    while to_yaw - from_yaw > std::f32::consts::PI {
        to_yaw -= std::f32::consts::TAU;
    }
    while to_yaw - from_yaw < -std::f32::consts::PI {
        to_yaw += std::f32::consts::TAU;
    }

    anim.from_yaw = from_yaw;
    anim.from_pitch = from_pitch;
    anim.from_dist = cur_dist;
    anim.to_yaw = to_yaw;
    anim.to_pitch = to_pitch;
    anim.to_dist = to_dist;
    anim.t = 0.0;
    anim.active = true;
}

/// 滑动切面:**向右滑 = 魔方右转 90°**(切到右侧那个面),上下左右同理。
/// 目标面用**相机自身的右/上轴**推出,和光照保持同一套坐标,避免世界坐标带来的不一致。
fn swipe_turn(
    mut drag: Local<Option<Vec2>>,
    buttons: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    window: Query<&Window>,
    cam: Query<&Transform, (With<Camera3d>, Without<DirectionalLight>)>,
    mut ui_state: ResMut<UiState>,
    edit: Res<editor::EditState>,
    mut log: ResMut<ui::LogBuffer>,
) {
    // 取色器打开时必须是模态交互，不能把按下/释放解释为切面滑动。
    if ui_state.mode != UiMode::Input || !ui_state.editing || edit.picker.is_some() {
        // 丢弃打开取色器前遗留的按下位置，避免关闭后下一次释放被误判为滑动。
        *drag = None;
        return; // 非"自定义输入"状态:禁止滑动切面
    }
    // 起点:鼠标左键按下 或 触摸开始
    let start_now = touches
        .iter_just_pressed()
        .next()
        .map(|t| t.position())
        .or_else(|| {
            buttons
                .just_pressed(MouseButton::Left)
                .then(|| window.single().ok().and_then(|w| w.cursor_position()))
                .flatten()
        });
    if let Some(p) = start_now {
        *drag = Some(p);
    }
    // 终点:松开时结算
    let end_now = touches
        .iter_just_released()
        .next()
        .map(|t| t.position())
        .or_else(|| {
            buttons
                .just_released(MouseButton::Left)
                .then(|| window.single().ok().and_then(|w| w.cursor_position()))
                .flatten()
        });
    let Some(end) = end_now else { return };
    let Some(start) = drag.take() else { return };

    let d = end - start;
    if d.length() < 60.0 {
        return; // 太短,不算滑动(也避免和"点格子"冲突)
    }
    let (dx, dy) = if d.x.abs() > d.y.abs() {
        (if d.x > 0.0 { 1.0 } else { -1.0 }, 0.0)
    } else {
        (0.0, if d.y > 0.0 { 1.0 } else { -1.0 })
    };
    // 屏幕 y 向下 ⇒ 向上滑是 -y
    let Ok(c) = cam.single() else { return };
    // 拖动方向 = 魔方**转动**方向:向左滑 ⇒ 魔方左转 ⇒ 把**左侧**的面转过来
    // (之前按"把拖向那一侧的面转过来"实现,体感正好相反 ✗)
    let target = if dx != 0.0 {
        c.right() * -dx
    } else {
        c.up() * dy
    };
    // 找到与该方向最接近的那个面
    let best = (0..6)
        .max_by(|a, b| {
            FACE_NORMAL[*a]
                .dot(target)
                .partial_cmp(&FACE_NORMAL[*b].dot(target))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    if FACE_NORMAL[best].dot(target) > 0.6 {
        ui_state.face = best;
        ui_state.input_view = InputView::Perpendicular;
        log.push(format!("滑动切面 → {}", "上右前下左后".chars().nth(best).unwrap()));
    }
}

/// 直接在魔方上点格子:正交相机下把光标投到该面所在平面,换算成 (面, 格号) 后打开取色器
fn cube_pick(
    buttons: Res<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    window: Query<&Window>,
    cam: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    ui_state: Res<UiState>,
    mut edit: ResMut<editor::EditState>,
) {
    if ui_state.mode != UiMode::Input || !ui_state.editing || edit.picker.is_some() {
        return; // 非"自定义输入"状态:点魔方不弹取色器
    }
    // 鼠标左键 / 触摸(手机上触摸会被映射成鼠标,但保险起见两个都收)
    let tapped = buttons.just_pressed(MouseButton::Left) || touches.any_just_pressed();
    if !tapped {
        return;
    }
    let Ok(win) = window.single() else { return };
    // 位置:优先用**触摸点**自己的坐标 —— 不依赖浏览器的"触摸模拟鼠标",
    // 否则手机上 cursor_position() 可能一直是 None,点了没反应。
    let pos = touches
        .iter_just_pressed()
        .next()
        .map(|t| t.position())
        .or_else(|| win.cursor_position());
    let Some(pos) = pos else { return };
    let Ok((camera, cam_tf)) = cam.single() else { return };
    let Ok(ray) = camera.viewport_to_world(cam_tf, pos) else { return };

    let f = ui_state.face.min(5);
    let n = FACE_NORMAL[f];
    // 与"该面所在平面"(法线方向 1.5)求交
    let denom = ray.direction.dot(n);
    if denom.abs() < 1e-6 {
        return;
    }
    let t = (1.5 - ray.origin.dot(n)) / denom;
    if t <= 0.0 {
        return;
    }
    let hit = ray.origin + ray.direction * t;

    // 平面内的两个切向轴 → 3×3 网格
    let right = if n.y.abs() > 0.5 { Vec3::X } else { Vec3::Y.cross(n).normalize() };
    let up = n.cross(right).normalize();
    let u = hit.dot(right);
    let v = hit.dot(up);
    if u.abs() > 1.5 || v.abs() > 1.5 {
        return; // 点在魔方外面
    }
    let col = ((u + 1.5) / 1.0).floor().clamp(0.0, 2.0) as usize;
    let row = ((1.5 - v) / 1.0).floor().clamp(0.0, 2.0) as usize;
    let idx = row * 3 + col;
    edit.picker = Some(f * 9 + idx);
}

/// 按状态机切换界面元素的显隐:**输入与播放不同时存在**
fn apply_mode_visibility(
    ui_state: Res<UiState>,
    mut q: Query<(&VisibilityInMode, &mut Node)>,
) {
    for (v, mut node) in q.iter_mut() {
        let want = match v.0 {
            UiMode::Input => ui_state.mode == UiMode::Input,
            UiMode::Play => ui_state.mode == UiMode::Play,
        };
        let d = if want { Display::Flex } else { Display::None };
        if node.display != d {
            node.display = d;
        }
    }
}

/// 只在某个状态下显示
#[derive(Component)]
pub struct VisibilityInMode(pub UiMode);

/// 按窗口宽高比调整相机距离:
/// 竖屏手机(FOV 是竖直方向的)会让魔方显得又小又靠边,
/// 这里按 min(1, aspect) 拉远一点,保证魔方完整可见且居中偏上。

/// 相机始终看向魔方中心
fn orbit_camera(
    ui_state: Res<UiState>,
    rig: Res<CameraRig>,
    mut q: Query<&mut Transform, (With<Camera3d>, Without<DirectionalLight>)>,
) {
    // 输入态由 cube_view 接管相机(正交、正对选中面),这里绝不能插手,
    // 否则每帧都会被覆盖回透视视角 ⇒ 魔方"消失"。
    if ui_state.mode == UiMode::Input {
        return;
    }
    for mut tf in q.iter_mut() {
        *tf = Transform::from_xyz(rig.yaw.sin() * rig.dist, rig.height, rig.yaw.cos() * rig.dist)
            .looking_at(Vec3::ZERO, Vec3::Y);
    }
}
/// 取某个贴纸色的"基准色"(供 UI 使用)
pub fn sticker_color_pair(c: cubr_core::model::StickerColor) -> (Color, ()) {
    use cubr_core::model::StickerColor::*;
    let col = match c {
        W => Color::srgb(0.93, 0.93, 0.93),
        Y => Color::srgb(1.00, 0.84, 0.10),
        R => Color::srgb(0.88, 0.14, 0.14),
        O => Color::srgb(0.95, 0.48, 0.09),
        B => Color::srgb(0.10, 0.32, 0.86),
        G => Color::srgb(0.10, 0.68, 0.26),
    };
    (col, ())
}

/// 把颜色压暗(0..1,factor 越小越暗)
pub fn dim(c: Color, factor: f32) -> Color {
    match c {
        Color::Srgba(x) => Color::srgb(x.red * factor, x.green * factor, x.blue * factor),
        other => other,
    }
}
