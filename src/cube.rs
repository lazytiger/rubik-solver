//! 魔方渲染层:把 `cubr-core` 的整数网格模型映射成 Bevy 实体。
//!
//! 每个小方块(`CoreCubie`)对应一个实体:
//! - 实体自身 = 黑色塑料本体
//! - 子实体   = 该方块外露的彩色贴纸(最多 3 张)
//!
//! ## ⚠️ 一个容易踩的坑
//! `cubr-core` 自带一份 `glam` 依赖,它的 `IVec3` 与 Bevy 的 `IVec3`
//! **是两个不同的类型**(即使版本号相同也不保证是同一个 crate 实例)。
//! 所以这里**只读字段**(`.x/.y/.z`)做转换,不在这两个类型之间互相传递。
//!
//! 逻辑真值永远在 `Model::core`(`CubeCore`)里,渲染只是它的投影 ——
//! 每次转动结束后由 [`sync_transforms`] 整体重算,避免浮点漂移。

use bevy::prelude::*;
use cubr_core::core::{CoreCubie, CubeCore};
use cubr_core::model::StickerColor;

use crate::Model;

/// 小方块边长(留 0.06 缝隙,让每块看得清)
pub const CUBIE_SIZE: f32 = 0.94;
/// 贴纸边长
const STICKER_SIZE: f32 = 0.80;
/// 贴纸厚度
const STICKER_THICK: f32 = 0.04;

/// 实体 ↔ 模型下标 的绑定。
/// `CubeCore::cubies()` 的顺序在转动前后保持不变(`home` 是稳定 id),用下标即可。
#[derive(Component)]
pub struct CubieIndex(pub usize);

/// 贴纸实体:记录它对应模型里哪个方块的第几个贴纸槽。
/// 因为 `CubeCore::paint()` 会**原地改贴纸颜色**(不重建实体),
/// 渲染层必须靠这个映射把材质同步过来 —— 否则 Apply 之后画面还是旧配色。
#[derive(Component)]
pub struct StickerOf {
    pub cubie: usize,
    pub slot: usize,
    /// 该贴纸对应**哪个面格**(g = face*9 + idx)——
    /// 用于在输入态按"编辑器盘面"取色(空格 ⇒ 中性灰)。
    pub facelet: usize,
}

/// 贴纸朝向(局部法线)→ 面编号 U=0 R=1 F=2 D=3 L=4 B=5
fn face_of_normal(n: (i32, i32, i32)) -> usize {
    match n {
        (1, 0, 0) => 1,
        (-1, 0, 0) => 4,
        (0, 1, 0) => 0,
        (0, -1, 0) => 3,
        (0, 0, 1) => 2,
        _ => 5, // (0,0,-1)
    }
}

/// 方块 home 坐标 + 贴纸法线 → 面格编号(行优先、idx 0 在左上)。
/// 规则已用项目里的角块表核对过(例:URF 角 [8,9,20] 三个面格号全部命中)。
fn facelet_of(home: (i32, i32, i32), n: (i32, i32, i32)) -> usize {
    let (x, y, z) = home;
    let idx = match face_of_normal(n) {
        0 => (z + 1) * 3 + (x + 1), // U:行随 -z→+z(F 在下),列随 x
        1 => (1 - y) * 3 + (1 - z), // R:左列贴 F
        2 => (1 - y) * 3 + (x + 1), // F:上排贴 U,右列贴 R
        3 => (1 - z) * 3 + (x + 1), // D:上排贴 F
        4 => (1 - y) * 3 + (z + 1), // L:右列贴 F
        _ => (1 - y) * 3 + (1 - x), // B:左列贴 R
    };
    face_of_normal(n) * 9 + idx as usize
}

/// 整数三元组 → Bevy `Vec3`(见模块文档:跨 glam 实例只读字段)
#[inline]
pub fn v3i(x: i32, y: i32, z: i32) -> Vec3 {
    Vec3::new(x as f32, y as f32, z as f32)
}

/// 六种贴纸颜色的真实魔方配色:白上 / 黄下 / 红右 / 橙左 / 蓝后 / 绿前
pub fn sticker_color(c: StickerColor) -> Color {
    match c {
        StickerColor::W => Color::srgb(0.94, 0.94, 0.96),
        StickerColor::Y => Color::srgb(1.00, 0.84, 0.10),
        StickerColor::R => Color::srgb(0.86, 0.12, 0.12),
        StickerColor::O => Color::srgb(1.00, 0.45, 0.06),
        StickerColor::B => Color::srgb(0.10, 0.32, 0.86),
        StickerColor::G => Color::srgb(0.10, 0.68, 0.26),
    }
}

/// 六色材质句柄表
#[derive(Resource)]
pub struct StickerMaterials {
    pub colors: Vec<(StickerColor, Handle<StandardMaterial>)>,
    /// **空格**用的中性灰(未填写 ⇒ 像没贴贴纸的塑料)
    pub empty: Handle<StandardMaterial>,
}

impl StickerMaterials {
    pub fn get(&self, c: StickerColor) -> Handle<StandardMaterial> {
        self.colors
            .iter()
            .find(|(k, _)| *k == c)
            .map(|(_, h)| h.clone())
            .expect("缺少该颜色的材质")
    }
    pub fn empty(&self) -> Handle<StandardMaterial> {
        self.empty.clone()
    }
}

/// `CoreCubie` 的整数位置/朝向 → Bevy 变换。
///
/// `orient` 是"局部 → 世界"旋转矩阵的**列**,
/// 所以 `Mat3::from_cols(orient[0], orient[1], orient[2])` 就是它的旋转矩阵。
pub fn cubie_transform(c: &CoreCubie) -> Transform {
    let o = c.orient;
    let m = Mat3::from_cols(
        v3i(o[0].x, o[0].y, o[0].z),
        v3i(o[1].x, o[1].y, o[1].z),
        v3i(o[2].x, o[2].y, o[2].z),
    );
    Transform {
        translation: v3i(c.pos.x, c.pos.y, c.pos.z),
        rotation: Quat::from_mat3(&m),
        scale: Vec3::ONE,
    }
}

/// 取某个方块的朝向矩阵(Bevy `Mat3`),供动画推导轴角用
pub fn cubie_rotation(c: &CoreCubie) -> Mat3 {
    let o = c.orient;
    Mat3::from_cols(
        v3i(o[0].x, o[0].y, o[0].z),
        v3i(o[1].x, o[1].y, o[1].z),
        v3i(o[2].x, o[2].y, o[2].z),
    )
}

/// 把贴纸材质同步成模型里的当前颜色。
///
/// 为什么需要:**转动**时贴纸是方块的子实体,会跟着一起转 ✓ 不用管;
/// 但 `CubeCore::paint()`(编辑器 Apply / 载入外部状态 / 重置)是**原地改颜色**,
/// 实体和材质都不会动 —— 早期就漏了这一步,于是 Apply 后画面仍是旧配色
/// ("魔方像被重置了"),而模型其实已经变了(所以求解正常、但动画过程是乱的)。
pub fn sync_sticker_materials(
    model: Res<Model>,
    mats: Res<StickerMaterials>,
    edit: Res<crate::editor::EditState>,
    ui_state: Res<crate::UiState>,
    reveal: Res<crate::RevealAnim>,
    mut q: Query<(&StickerOf, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let debug = std::env::var("RUBIK_DEBUG").is_ok();
    let mut fixed = 0usize;
    for (s, mut mat) in q.iter_mut() {
        let Some(cubie) = model.core.cubies().get(s.cubie) else { continue };
        // 输入态:以**编辑器盘面**为准(空格渲染成中性灰);
        // 播放态:以模型为准(动画转的就是它)。
        // 只有"编辑中且没有转动在跑"时才按编辑器盘面渲染(能看到灰格/涂色);
        // 其余时刻(打乱转动中、重置、播放)一律按**模型**渲染 ——
        // 否则会出现"开始转动时颜色变灰、转完才回来"的错误 ✗
        let use_editor = ui_state.mode == crate::UiMode::Input
            && ui_state.editing
            && !reveal.active;
        let want = if use_editor {
            let (f, i) = crate::validate::split(s.facelet);
            // "离面心距离"决定扩散/收缩的顺序:0=面心,1=边,√2=角
            let (col, row) = ((i % 3) as f32 - 1.0, (i / 3) as f32 - 1.0);
            let ring = (col * col + row * row).sqrt();
            if !reveal.shown(ring) {
                // 还没轮到它 → 先保持"未填"的灰色
                mats.empty()
            } else {
                match edit.cells[f][i] {
                    Some(c) => mats.get(c),
                    None => mats.empty(),
                }
            }
        } else {
            let Some((_, color)) = cubie.stickers.get(s.slot) else { continue };
            mats.get(*color)
        };
        if mat.0 != want {
            mat.0 = want;
            fixed += 1;
        }
    }
    // 渲染层失配告警:正常情况下只有"刚 paint 过"的那一帧会修,
    // 其余时候 fixed > 0 就说明有路径改了模型却没让渲染跟上(曾经的 Apply bug)
    if debug && fixed > 0 {
        warn!("[render] 本帧修正了 {fixed} 个贴纸材质(模型与画面曾经不一致)");
    }
}

/// 同上,但查询里带 `Entity`(动作层/动画层用这种查询)
pub fn sync_transforms3(core: &CubeCore, q: &mut Query<(Entity, &CubieIndex, &mut Transform)>) {
    for (_e, idx, mut tf) in q.iter_mut() {
        if let Some(c) = core.cubies().get(idx.0) {
            *tf = cubie_transform(c);
        }
    }
}

/// 从模型整体重算所有方块实体的变换(转动结束后调用,作为权威校正)
pub fn sync_transforms(core: &CubeCore, q: &mut Query<(&CubieIndex, &mut Transform)>) {
    for (idx, mut tf) in q.iter_mut() {
        if let Some(c) = core.cubies().get(idx.0) {
            *tf = cubie_transform(c);
        }
    }
}

/// 生成 27 个小方块实体(含贴纸),并按当前模型摆好位置
pub fn spawn_cube_entities(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    model: Res<Model>,
) {
    let body_mesh = meshes.add(Cuboid::new(CUBIE_SIZE, CUBIE_SIZE, CUBIE_SIZE));
    let sticker_mesh = meshes.add(Cuboid::new(STICKER_SIZE, STICKER_SIZE, STICKER_THICK));
    let body_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.06, 0.06, 0.07),
        perceptual_roughness: 0.75,
        metallic: 0.05,
        ..default()
    });

    // 六色材质表
    let mut table = Vec::new();
    for c in StickerColor::ALL {
        table.push((
            c,
            materials.add(StandardMaterial {
                base_color: sticker_color(c),
                // ⚠️ 贴纸要"哑光":以前 0.35 ≈ 半亮面,一旦光接近视线方向,
                //    高光正好反射回镜头 ⇒ 整个面泛起白 sheen、颜色发白(实测反馈)。
                //    0.80 ≈ 塑料贴纸的哑光质感,既没有高光斑,又保留一点方向性明暗。
                perceptual_roughness: 0.80,
                metallic: 0.0,
                ..default()
            }),
        ));
    }
    // 空格用中性灰(未填 ⇒ 像没贴贴纸的塑料)
    let empty_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.30, 0.30, 0.33),
        perceptual_roughness: 0.85,
        metallic: 0.0,
        ..default()
    });
    let mats = StickerMaterials { colors: table, empty: empty_mat };
    let handle_of = |c: StickerColor| mats.get(c);
    commands.insert_resource(StickerMaterials {
        colors: mats.colors.clone(),
        empty: mats.empty.clone(),
    });

    for (i, cubie) in model.core.cubies().iter().enumerate() {
        commands
            .spawn((
                Mesh3d(body_mesh.clone()),
                MeshMaterial3d(body_mat.clone()),
                cubie_transform(cubie),
                CubieIndex(i),
            ))
            .with_children(|parent| {
                for (slot, (normal, color)) in cubie.stickers.iter().enumerate() {
                    let n = v3i(normal.x, normal.y, normal.z);
                    let home = (cubie.home.x, cubie.home.y, cubie.home.z);
                    let fl = facelet_of(home, (normal.x, normal.y, normal.z));
                    parent.spawn((
                        Mesh3d(sticker_mesh.clone()),
                        MeshMaterial3d(handle_of(*color)),
                        StickerOf { cubie: i, slot, facelet: fl },
                        // 贴纸贴在表面朝外:把本地 +Z 旋到法线方向
                        Transform::from_translation(
                            n * (CUBIE_SIZE * 0.5 + STICKER_THICK * 0.5 - 0.01),
                        )
                        .with_rotation(Quat::from_rotation_arc(Vec3::Z, n)),
                    ));
                }
            });
    }
}