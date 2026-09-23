//! 正六边形取色器。
//!
//! 交互(需求原文):点击一个格子之后弹出颜色可供选择,**正六边形,每个顶点显示一个颜色**;
//! 颜色弹出的时候**魔方显示要模糊掉,只留下颜色和中间的格子**。
//!
//! 实现要点:
//! - 全屏遮罩(深色高不透明度)= "虚化掉其余部分",只让中间的六边形与文字清晰 ✓
//!   (真高斯模糊需要后处理管线;深色遮罩 + 只留取色器在视觉上达到同样的"聚焦"效果)
//! - 六边形 = 6 个顶点色块按 60° 均匀分布;中间显示**正在编辑的那一格**的当前颜色
//! - 附带「擦除此格」「取消」

use bevy::prelude::*;
use cubr_core::model::StickerColor;

use crate::editor::EditState;
use crate::{dim, sticker_color_pair};

/// 六边形顶点半径(默认值;实际会在 apply_picker 里按格子投影尺寸自适应)
const R_DEFAULT: f32 = 300.0;
/// 顶点色块直径
const SW: f32 = 66.0;

#[derive(Component)]
pub struct PickerRoot;
#[derive(Component)]
pub struct PickerSwatch(pub StickerColor);
#[derive(Component)]
pub struct PickerCell;
#[derive(Component)]
pub struct PickerCancel;

/// 六个颜色(按调色板顺序)
const COLORS: [StickerColor; 6] = [
    StickerColor::W,
    StickerColor::Y,
    StickerColor::R,
    StickerColor::O,
    StickerColor::B,
    StickerColor::G,
];

fn button_node() -> Node {
    Node {
        padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
        border: UiRect::all(Val::Px(1.0)),
        ..default()
    }
}

pub fn spawn_picker(mut commands: Commands, font: Res<crate::UiFont>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            // 深色遮罩:让背后的魔方/面板"虚化"下去
            BackgroundColor(Color::srgba(0.03, 0.03, 0.05, 0.93)),
            ZIndex(100),
            PickerRoot,
        ))
        .with_children(|overlay| {
            // 六个圆形色块 + 一个格子描边 —— **全部作为遮罩的直接子节点**,
            // 只用一个坐标系(屏幕坐标),避免多层嵌套导致的偏移
            for (i, c) in COLORS.iter().enumerate() {
                let ang = std::f32::consts::TAU * (i as f32) / 6.0 - std::f32::consts::FRAC_PI_2;
                let cx = R_DEFAULT - SW / 2.0 + R_DEFAULT * ang.cos();
                let cy = R_DEFAULT - SW / 2.0 + R_DEFAULT * ang.sin();
                let (base, _) = sticker_color_pair(*c);
                overlay.spawn((
                    Button,
                    PickerSwatch(*c),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(cx),
                        top: Val::Px(cy),
                        width: Val::Px(SW),
                        height: Val::Px(SW),
                        border: UiRect::all(Val::Px(3.0)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.96, 0.97, 1.0)),
                    BackgroundColor(base),
                ));
            }
            overlay.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(0.0),
                    height: Val::Px(0.0),
                    border: UiRect::all(Val::Px(2.5)),
                    ..default()
                },
                BorderColor::all(Color::srgb(0.98, 0.92, 0.55)),
                BackgroundColor(Color::NONE),
                PickerCell,
            ));

            // 底部:擦除 / 取消(绝对定位在遮罩内)
            overlay
                .spawn(Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(96.0),   // 避开底部的"面选择器"一行
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|row| {
                    for (label, is_cancel) in [("取消", true)] {
                        let mut e = row.spawn((
                            Button,
                            button_node(),
                            BorderColor::all(Color::srgb(0.45, 0.5, 0.6)),
                            BackgroundColor(Color::srgb(0.18, 0.2, 0.26)),
                            Text::new(label),
                            TextFont {
                                font: font.0.clone().into(),
                                font_size: FontSize::Px(14.0),
                                ..default()
                            },
                            TextColor(Color::srgb(0.9, 0.93, 1.0)),
                            TextLayout { linebreak: LineBreak::NoWrap, ..default() },
                        ));
                        let _ = is_cancel;
                        e.insert(PickerCancel);
                    }
                });
        });
}

/// 取色器显隐 / 定位(**以选中格子为中心**)/ 颜色可选性 / 提示
pub fn apply_picker(
    edit: Res<EditState>,
    window: Query<&Window>,
    ui_scale: Res<UiScale>,
    cam: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut roots: Query<&mut Node, (With<PickerRoot>, Without<PickerCell>, Without<PickerSwatch>)>,
    mut cellout: Query<&mut Node, (With<PickerCell>, Without<PickerRoot>, Without<PickerSwatch>)>,
    mut swatches: Query<(&PickerSwatch, &mut BackgroundColor, &mut Node), (Without<PickerRoot>, Without<PickerCell>)>,
) {
    let open = edit.picker.is_some();
    for mut node in roots.iter_mut() {
        let want = if open { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    if !open {
        return;
    }
    let g = edit.picker.unwrap();
    let (f, i) = crate::validate::split(g);

    // 允许填的颜色正常显示,放不下的置灰
    let allowed = crate::validate::allowed_colors(&edit.cells, g);
    for (sw, mut bg, _n) in swatches.iter_mut() {
        let idx = COLORS.iter().position(|c| *c == sw.0).unwrap_or(0);
        let (base, _) = sticker_color_pair(sw.0);
        let want = if allowed[idx] { base } else { dim(base, 0.22) };
        if bg.0 != want {
            bg.0 = want;
        }
    }

    // 投影出这一格的屏幕位置与边长
    let n = crate::FACE_NORMAL[f];
    let right = if n.y.abs() > 0.5 { Vec3::X } else { Vec3::Y.cross(n).normalize() };
    let up = n.cross(right).normalize();
    let (col, row) = ((i % 3) as f32, (i / 3) as f32);
    let center = n * 1.5 + right * (col - 1.0) + up * (1.0 - row);
    let Ok(_win) = window.single() else { return };
    let Ok((camera, cam_tf)) = cam.single() else { return };
    let Ok(p) = camera.world_to_viewport(cam_tf, center) else { return };
    // ⚠️ 关键换算:world_to_viewport 给的是**屏幕逻辑像素**,
    //    而 UI 的 left/top 会再乘以 UiScale(手机上 ≈0.42)⇒ 必须除掉,
    //    否则 PC(UiScale=1)正常、手机错位(之前就是这么错的 ✗)
    let s = ui_scale.0.max(0.01);
    let p = p / s;
    // 圆圈同样反向补偿:手机上按钮物理尺寸不缩水(触摸目标够大)
    let f = (1.0 / s).clamp(1.0, 2.4);
    let sw = SW * f;
    let side = match camera.world_to_viewport(cam_tf, center + right * 0.5) {
        Ok(edge) => (((edge.x - p.x * s).abs() * 2.0) / s).max(20.0),
        Err(_) => 110.0,
    };
    let r = (side * 1.45 + sw * 0.35).clamp(130.0, 460.0);

    // 六个圆圈:直接按屏幕坐标摆(以格子中心为圆心)
    for (k, (_sw, _bg, mut node)) in swatches.iter_mut().enumerate() {
        let ang = std::f32::consts::TAU * (k as f32) / 6.0 - std::f32::consts::FRAC_PI_2;
        node.left = Val::Px(p.x + r * ang.cos() - sw / 2.0);
        node.top = Val::Px(p.y + r * ang.sin() - sw / 2.0);
        node.width = Val::Px(sw);
        node.height = Val::Px(sw);
    }
    // 描边:正好套住这一格
    for mut node in cellout.iter_mut() {
        node.width = Val::Px(side);
        node.height = Val::Px(side);
        node.left = Val::Px(p.x - side / 2.0);
        node.top = Val::Px(p.y - side / 2.0);
    }
}

/// 点顶点色块 → 给正在编辑的格子涂色;「擦除此格」→ 清空;「取消」→ 关闭
pub fn picker_click(
    swatches: Query<(&Interaction, &PickerSwatch), Changed<Interaction>>,
    cancel: Query<&Interaction, (Changed<Interaction>, With<PickerCancel>)>,
    mut edit: ResMut<EditState>,
) {
    let Some(g) = edit.picker else { return };
    let (f, i) = crate::validate::split(g);

    for (interaction, sw) in swatches.iter() {
        if *interaction == Interaction::Pressed {
            edit.paint(f, i, sw.0);
            edit.picker = None; // 选完即关
            edit.dirty = true;
        }
    }
    if cancel.iter().any(|i| *i == Interaction::Pressed) {
        edit.picker = None;
        edit.dirty = true;
    }
}