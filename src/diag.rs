//! 旋转动画自检:验证"每次转动的轴角推导"永远得到**单层轴 + 90°/180°/270°**。
//!
//! 这是给一个真实 bug 加的回归测试:
//! 早期实现用 `CubeCore::paint()` 压平出来的副本当"转动后"状态,而 `paint()`
//! 会把所有方块朝向重置为单位矩阵 → 推出的 Δ = `R_move · R_真实⁻¹`,
//! 可能是体对角线轴(如 0.577,−0.577,−0.577)转 240°,动画就会把层里的方块
//! 甩出立方体、露出内部。
//!
//! 现在动画与自检共用 [`crate::anim::derive_axis_angle`],所以这个测试直接覆盖线上路径。

use cubr_core::core::CubeCore;
use cubr_core::model::Move;

use crate::anim::{axis_aligned, derive_axis_angle};

/// 全部 18 种转动
const ALL_MOVES: [&str; 18] = [
    "U", "U'", "U2", "R", "R'", "R2", "F", "F'", "F2", "D", "D'", "D2", "L", "L'", "L2", "B",
    "B'", "B2",
];

pub fn run() -> bool {
    println!("== 旋转动画自检(轴角推导)==");

    // 先在多个不同的打乱状态下测:朝向越复杂越容易暴露问题
    let scrambles = [
        "",                                     // 还原态
        "R",                                    // 单步
        "R U2 F' L D B2 R' F U D2 L2",          // 10 步
        "L B' F2 R F2 L2 R2 D2 U' B2 U L F D2 U' L2 R2", // 20 步
    ];

    let mut pass = 0;
    let mut fail = 0;
    let mut worst: Vec<String> = Vec::new();

    for (si, sc) in scrambles.iter().enumerate() {
        let mut core = CubeCore::solved();
        for t in sc.split_whitespace() {
            core.apply(Move::parse(t).unwrap());
        }
        let label = if sc.is_empty() { "还原态".to_string() } else { format!("打乱{}", si) };

        for (i, name) in ALL_MOVES.iter().enumerate() {
            let mv = Move::parse(name).unwrap();
            // 每转一次就重新打乱一次,保证 18 种转动都在"有复杂朝向"的状态上测
            let mut c = CubeCore::solved();
            if !sc.is_empty() {
                for t in sc.split_whitespace() {
                    c.apply(Move::parse(t).unwrap());
                }
            }
            // 再叠几转,进一步让朝向复杂化(第 i 个转动前先转 i%4 次)
            for k in 0..(i % 4) {
                c.apply(Move::parse(ALL_MOVES[k]).unwrap());
            }

            let (axis, angle) = derive_axis_angle(&mut c, mv);
            let deg = angle.to_degrees();
            let ok_axis = axis_aligned(axis);
            // 允许 90/180/270(角度以 [0,360) 表示)
            let ok_angle = [90.0, 180.0, 270.0].iter().any(|t| (deg - t).abs() < 0.5);

            if ok_axis && ok_angle {
                pass += 1;
            } else {
                fail += 1;
                let msg = format!(
                    "{label} 转 {name}:轴({:+.3},{:+.3},{:+.3}) 角 {deg:.1}°",
                    axis.x, axis.y, axis.z
                );
                worst.push(msg);
            }
        }
    }

    println!("  用例:{} 个打乱态 × 18 种转动 = {} 组", scrambles.len(), scrambles.len() * 18);
    if worst.is_empty() {
        println!("  ✅ 全部轴对齐(单分量 ±1),角度均为 90/180/270");
    } else {
        for w in worst.iter().take(6) {
            println!("  ❌ {w}");
        }
    }
    println!("  旋转动画自检:{pass} 通过 / {fail} 失败");
    fail == 0
}
