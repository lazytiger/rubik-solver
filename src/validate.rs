//! 魔方状态的**合法性校验**与**局部推导**。
//!
//! # 背景:什么是"合法的魔方状态"
//! 不是"54 个格子随便涂"就是一个魔方。一个 3×3×3 状态**可达**(能从还原态转出来)
//! 当且仅当下面 5 条同时成立 —— 这 5 条既必要也充分:
//!
//! 1. **中心固定**:6 个中心块从不移动,所以每个面的中心必须是对应面的标准色。
//!    (中心块定义配色方案;中心错了 ⇒ 这个状态在标准配色下不可达)
//! 2. **数量**:每种颜色恰好 9 格。
//! 3. **块合法**:8 个角位置上的三色组合必须恰好是 8 个合法角块的一个排列;
//!    12 个棱位置上的两色组合必须是 12 个合法棱块的一个排列。(不能重复、不能凭空造块)
//! 4. **朝向**:角块朝向和 ≡ 0 (mod 3);棱块朝向和 ≡ 0 (mod 2)。
//!    (这条拦下"单角翻转 / 单棱翻转"这类拆装错误 —— 它们能通过第 2、3 条)
//! 5. **奇偶**:角块排列的奇偶性 == 棱块排列的奇偶性。
//!    (这条拦下"只交换两个棱"这类错误)
//!
//! # 局部推导(边填边自动确定)
//! 用**块级约束传播**,两条规则:
//! - **顺序约束**:角块三色在角上是**循环有序**的。所以"某个角位上已知 2 色"
//!   连同它们在该角上的**相对顺序**,就能唯一确定是哪个角块(以及朝向)。
//!   例:U 位=W、R 位=R ⇒ 只能是 `W-R-G`(URF);`W-B-R`(UBR) 的循环顺序是
//!   `W→B→R`,不满足 `W→R`,被排除。
//! - **占用约束**:某位置的候选块 = 与已知色相容、且未被其它位置占用的合法块。
//!   候选唯一 ⇒ 该位置的块确定 ⇒ 缺失槽位按朝向自动填色。
//!   (例:还原态挖掉一格,其余块都已被占用 ⇒ 剩下那块唯一)
//! - **配额**:某颜色已放满 9 格 ⇒ 其余格子不能再涂该色。
//! - **对色互斥**(即时拒绝):同一个角/棱上不能出现对面色(白黄 / 红橙 / 蓝绿)。
//!
//! # 为什么不做完整约束传播
//! 完整传播等价于求解魔方(需要搜索),不适合每次点击都跑。这里的策略是:
//! **局部能确定的立刻确定 + 每次修改后跑一次完整校验并给出精确错误**。

use cubr_core::model::StickerColor;

/// 面的顺序:U R F D L B(与 `CubeState` 字段一致)
pub const FACE_COLOR: [StickerColor; 6] = [
    StickerColor::W, // U
    StickerColor::R, // R
    StickerColor::G, // F
    StickerColor::Y, // D
    StickerColor::O, // L
    StickerColor::B, // B
];

/// 中心格在某面内的下标
pub const CENTER_IDX: usize = 4;

/// 8 个角位置,每个 3 个贴纸的**全局**下标(面序 URFDLB,每面 9 格行优先)。
/// 每个角的第一项总是 U 或 D 面 —— 朝向计算依赖这个顺序。
pub const CORNER_FACELETS: [[usize; 3]; 8] = [
    [8, 9, 20],   // URF
    [6, 18, 38],  // UFL
    [0, 36, 47],  // ULB
    [2, 45, 11],  // UBR
    [29, 26, 15], // DFR
    [27, 44, 24], // DLF
    [33, 53, 42], // DBL
    [35, 17, 51], // DRB
];

/// 12 个棱位置,每个 2 个贴纸的全局下标
pub const EDGE_FACELETS: [[usize; 2]; 12] = [
    [5, 10],  // UR
    [7, 19],  // UF
    [3, 37],  // UL
    [1, 46],  // UB
    [32, 16], // DR
    [28, 25], // DF
    [30, 43], // DL
    [34, 52], // DB
    [23, 12], // FR
    [21, 41], // FL
    [50, 39], // BL
    [48, 14], // BR
];

/// 把全局贴纸下标转成 (面, 面内下标)
#[inline]
pub fn split(g: usize) -> (usize, usize) {
    (g / 9, g % 9)
}

/// 对面色:白↔黄,红↔橙,蓝↔绿
pub fn opposite(c: StickerColor) -> StickerColor {
    match c {
        StickerColor::W => StickerColor::Y,
        StickerColor::Y => StickerColor::W,
        StickerColor::R => StickerColor::O,
        StickerColor::O => StickerColor::R,
        StickerColor::B => StickerColor::G,
        StickerColor::G => StickerColor::B,
    }
}

/// 颜色中文名(界面/错误信息用)
pub fn color_cn(c: StickerColor) -> &'static str {
    match c {
        StickerColor::W => "白",
        StickerColor::Y => "黄",
        StickerColor::R => "红",
        StickerColor::O => "橙",
        StickerColor::B => "蓝",
        StickerColor::G => "绿",
    }
}

/// 颜色短名(调试/紧凑输出用)
pub fn name(c: StickerColor) -> &'static str {
    match c {
        StickerColor::W => "W",
        StickerColor::Y => "Y",
        StickerColor::R => "R",
        StickerColor::O => "O",
        StickerColor::B => "B",
        StickerColor::G => "G",
    }
}

/// 某位置在还原态下应该有的颜色集合(即"这一块是什么块")
pub fn solved_corner_piece(i: usize) -> [StickerColor; 3] {
    let f = CORNER_FACELETS[i];
    [FACE_COLOR[f[0] / 9], FACE_COLOR[f[1] / 9], FACE_COLOR[f[2] / 9]]
}
pub fn solved_edge_piece(i: usize) -> [StickerColor; 2] {
    let f = EDGE_FACELETS[i];
    [FACE_COLOR[f[0] / 9], FACE_COLOR[f[1] / 9]]
}

/// 编辑器用的盘面:每格 `None` = 未填
pub type Cells = [[Option<StickerColor>; 9]; 6];

/// 读一格
#[inline]
pub fn get(cells: &Cells, g: usize) -> Option<StickerColor> {
    let (f, i) = split(g);
    cells[f][i]
}
/// 写一格
#[inline]
pub fn set(cells: &mut Cells, g: usize, c: Option<StickerColor>) {
    let (f, i) = split(g);
    cells[f][i] = c;
}

/// 每个颜色已放置的数量
pub fn counts(cells: &Cells) -> [u8; 6] {
    let mut n = [0u8; 6];
    for f in 0..6 {
        for i in 0..9 {
            if let Some(c) = cells[f][i] {
                n[color_idx(c)] += 1;
            }
        }
    }
    n
}

pub fn color_idx(c: StickerColor) -> usize {
    match c {
        StickerColor::W => 0,
        StickerColor::Y => 1,
        StickerColor::R => 2,
        StickerColor::O => 3,
        StickerColor::B => 4,
        StickerColor::G => 5,
    }
}
pub const COLOR_ORDER: [StickerColor; 6] = [
    StickerColor::W,
    StickerColor::Y,
    StickerColor::R,
    StickerColor::O,
    StickerColor::B,
    StickerColor::G,
];

/// 某格是否属于中心(中心不可编辑)
pub fn is_center(g: usize) -> bool {
    g % 9 == CENTER_IDX
}

/// 某格所属的块(角/棱)及其在该块内的槽位;返回 (类型, 块的序号, 槽位)
pub fn piece_of(g: usize) -> Option<(PieceKind, usize, usize)> {
    for (i, corner) in CORNER_FACELETS.iter().enumerate() {
        if let Some(s) = corner.iter().position(|&x| x == g) {
            return Some((PieceKind::Corner, i, s));
        }
    }
    for (i, edge) in EDGE_FACELETS.iter().enumerate() {
        if let Some(s) = edge.iter().position(|&x| x == g) {
            return Some((PieceKind::Edge, i, s));
        }
    }
    None // 中心
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum PieceKind {
    Corner,
    Edge,
}

/// 能否把 `c` 涂到 `g`:即时局部检查(中心 / 对色互斥 / 配额)
pub fn can_paint(cells: &Cells, g: usize, c: StickerColor) -> Result<(), String> {
    if is_center(g) {
        return Err("中心块固定,不可更改".into());
    }
    // 配额:该颜色已满 9 格且这一格不是它 → 不能涂
    let n = counts(cells);
    let (gf, gi) = split(g);
    let empties = (0..54).filter(|&x| get(cells, x).is_none()).count();
    // 只在"填色模式"(盘面还有空格)下硬拦配额。
    // 盘面已满时属于"替换某个贴纸",任何一格都允许改(计数错误交给完整校验提示)。
    if empties > 0 && cells[gf][gi] != Some(c) && n[color_idx(c)] >= 9 {
        let avail = available_colors(cells);
        let avail_cn: Vec<&str> = avail.iter().map(|x| color_cn(*x)).collect();
        return Err(if avail.is_empty() {
            format!("「{}」已放满 9 格", color_cn(c))
        } else {
            format!("「{}」已放满 9 格;还可以用:{}", color_cn(c), avail_cn.join("/"))
        });
    }
    // 对色互斥:同一块上不能出现对面色
    if let Some((kind, pi, _)) = piece_of(g) {
        let slots: &[usize] = match kind {
            PieceKind::Corner => &CORNER_FACELETS[pi],
            PieceKind::Edge => &EDGE_FACELETS[pi],
        };
        for &s in slots {
            if s == g {
                continue;
            }
            if let Some(other) = get(cells, s) {
                if other == opposite(c) {
                    return Err(format!(
                        "同一{}上不能同时有 {} 和 {}(对面色)",
                        if kind == PieceKind::Corner { "角块" } else { "棱块" },
                        name(c),
                        name(other)
                    ));
                }
            }
        }
    }
    Ok(())
}

/// 局部推导:用**块级约束**把能唯一确定的格子填上,返回新确定的全局下标。
///
/// 规则(比"角块 2 色定第 3 色"更强也更正确):
/// - 角块/棱块的颜色顺序是**循环**的,所以给定一个已知色就能定出朝向偏移 `r`,
///   从而把该块其余颜色填到对应槽位;
/// - 某位置的候选块 = 与已知色一致、且**未被其它位置占用**的合法块。
///   候选唯一 ⇒ 该位置的块确定 ⇒ 缺失槽位自动填色。
///
/// ⚠️ 注意:单看"颜色集合"是不够的 —— `W+R` 既可能是 `W-R-G`(URF),
/// 也可能是 `W-B-R`(UBR)。真正起作用的是**循环顺序** + **块是否已被占用**。
pub fn propagate(cells: &mut Cells, derived: &mut [[bool; 9]; 6]) -> Vec<usize> {
    let mut filled: Vec<usize> = Vec::new();

    // 中心恒为各面标准色
    for f in 0..6 {
        cells[f][CENTER_IDX] = Some(FACE_COLOR[f]);
    }

    let mut changed = true;
    while changed {
        changed = false;

        // 1) 已完全确定的块(整块颜色都对得上)→ 标记为"已占用"
        let used_c = used_corners(cells);
        let used_e = used_edges(cells);

        // 2) 角块
        for ci in 0..8 {
            let slots = CORNER_FACELETS[ci];
            if slots.iter().all(|&g| get(cells, g).is_some()) {
                continue;
            }
            let cands: Vec<usize> = (0..8)
                .filter(|&k| !used_c[k] && corner_consistent(cells, ci, k).is_some())
                .collect();
            if cands.len() != 1 {
                continue;
            }
            let k = cands[0];
            if let Some(rot) = corner_consistent(cells, ci, k) {
                let piece = solved_corner_piece(k);
                for (i, &g) in slots.iter().enumerate() {
                    if get(cells, g).is_none() {
                        let c = piece[(i + rot) % 3];
                        set(cells, g, Some(c));
                        let (f, x) = split(g);
                        derived[f][x] = true;
                        filled.push(g);
                        changed = true;
                    }
                }
            }
        }

        // 2.5) 配额规则:某颜色还差 k 个,而全局正好只剩 k 个空格 ⇒ 这些空格全是它
        //      (总和恒等于空格数,所以至多一个颜色满足,不存在歧义)
        let cnt = counts(cells);
        let empties: Vec<usize> = (0..54).filter(|&g| get(cells, g).is_none()).collect();
        if !empties.is_empty() {
            for c in COLOR_ORDER {
                let missing = 9usize.saturating_sub(cnt[color_idx(c)] as usize);
                if missing == empties.len() {
                    for &g in &empties {
                        set(cells, g, Some(c));
                        let (f, i) = split(g);
                        derived[f][i] = true;
                        filled.push(g);
                    }
                    changed = true;
                    break;
                }
            }
            if changed {
                continue;
            }
        }

        // 3) 棱块
        for ei in 0..12 {
            let slots = EDGE_FACELETS[ei];
            if slots.iter().all(|&g| get(cells, g).is_some()) {
                continue;
            }
            let cands: Vec<usize> = (0..12)
                .filter(|&k| !used_e[k] && edge_consistent(cells, ei, k).is_some())
                .collect();
            if cands.len() != 1 {
                continue;
            }
            let k = cands[0];
            if let Some(rot) = edge_consistent(cells, ei, k) {
                let piece = solved_edge_piece(k);
                for (i, &g) in slots.iter().enumerate() {
                    if get(cells, g).is_none() {
                        set(cells, g, Some(piece[(i + rot) % 2]));
                        let (f, x) = split(g);
                        derived[f][x] = true;
                        filled.push(g);
                        changed = true;
                    }
                }
            }
        }
    }
    filled
}

/// 角位 `ci` 放块 `k` 是否与已知色相容?相容则返回朝向偏移 `r`
/// (即 `cells[slot_i] == piece[(i + r) % 3]`)
fn corner_consistent(cells: &Cells, ci: usize, k: usize) -> Option<usize> {
    let slots = CORNER_FACELETS[ci];
    let piece = solved_corner_piece(k);
    let mut rot: Option<usize> = None;
    for (i, &g) in slots.iter().enumerate() {
        if let Some(c) = get(cells, g) {
            let r = (0..3).find(|&r| piece[(i + r) % 3] == c)?;
            match rot {
                None => rot = Some(r),
                Some(r0) if r0 == r => {}
                _ => return None,
            }
        }
    }
    Some(rot.unwrap_or(0))
}

fn edge_consistent(cells: &Cells, ei: usize, k: usize) -> Option<usize> {
    let slots = EDGE_FACELETS[ei];
    let piece = solved_edge_piece(k);
    let mut rot: Option<usize> = None;
    for (i, &g) in slots.iter().enumerate() {
        if let Some(c) = get(cells, g) {
            let r = (0..2).find(|&r| piece[(i + r) % 2] == c)?;
            match rot {
                None => rot = Some(r),
                Some(r0) if r0 == r => {}
                _ => return None,
            }
        }
    }
    Some(rot.unwrap_or(0))
}

/// 哪些角块已被其它位置确定占用
fn used_corners(cells: &Cells) -> [bool; 8] {
    let mut used = [false; 8];
    for ci in 0..8 {
        let cols = [
            get(cells, CORNER_FACELETS[ci][0]),
            get(cells, CORNER_FACELETS[ci][1]),
            get(cells, CORNER_FACELETS[ci][2]),
        ];
        if cols.iter().any(|c| c.is_none()) {
            continue;
        }
        let cols = [cols[0].unwrap(), cols[1].unwrap(), cols[2].unwrap()];
        for k in 0..8 {
            if same_cyclic(&cols, &solved_corner_piece(k)) {
                used[k] = true;
                break;
            }
        }
    }
    used
}

fn used_edges(cells: &Cells) -> [bool; 12] {
    let mut used = [false; 12];
    for ei in 0..12 {
        let a = get(cells, EDGE_FACELETS[ei][0]);
        let b = get(cells, EDGE_FACELETS[ei][1]);
        let (Some(a), Some(b)) = (a, b) else { continue };
        for k in 0..12 {
            let p = solved_edge_piece(k);
            if (a == p[0] && b == p[1]) || (a == p[1] && b == p[0]) {
                used[k] = true;
                break;
            }
        }
    }
    used
}

/// 找出"没有任何合法块可放"的位置(即使用户还没填满,也能立刻提示矛盾)
pub fn contradictions(cells: &Cells) -> Vec<String> {
    let mut out = Vec::new();
    let used_c = used_corners(cells);
    let used_e = used_edges(cells);
    for ci in 0..8 {
        let slots = CORNER_FACELETS[ci];
        if slots.iter().all(|&g| get(cells, g).is_some()) {
            continue;
        }
        let n = (0..8)
            .filter(|&k| !used_c[k] && corner_consistent(cells, ci, k).is_some())
            .count();
        if n == 0 {
            out.push(format!("角位 {} 已无任何合法角块可放(前面某处涂错了)", ci + 1));
        }
    }
    for ei in 0..12 {
        let slots = EDGE_FACELETS[ei];
        if slots.iter().all(|&g| get(cells, g).is_some()) {
            continue;
        }
        let n = (0..12)
            .filter(|&k| !used_e[k] && edge_consistent(cells, ei, k).is_some())
            .count();
        if n == 0 {
            out.push(format!("棱位 {} 已无任何合法棱块可放(前面某处涂错了)", ei + 1));
        }
    }
    out
}

/// 哪些颜色已经放满 9 格
pub fn full_colors(cells: &Cells) -> [bool; 6] {
    let n = counts(cells);
    let mut out = [false; 6];
    for (i, o) in out.iter_mut().enumerate() {
        *o = n[i] >= 9;
    }
    out
}

/// 还有余量的颜色(用于给用户提示"该格应该涂什么")
pub fn available_colors(cells: &Cells) -> Vec<StickerColor> {
    let full = full_colors(cells);
    COLOR_ORDER
        .iter()
        .copied()
        .filter(|c| !full[color_idx(*c)])
        .collect()
}

/// 处于"矛盾"状态的位置(没有任何合法块可放),返回这些位置涉及的面块下标。
/// 用于把出问题的块标红,而不是只给一行文字。
pub fn contradiction_cells(cells: &Cells) -> Vec<usize> {
    let mut out = Vec::new();
    let used_c = used_corners(cells);
    let used_e = used_edges(cells);
    for ci in 0..8 {
        let slots = CORNER_FACELETS[ci];
        if slots.iter().all(|&g| get(cells, g).is_some()) {
            continue;
        }
        let n = (0..8).filter(|&k| !used_c[k] && corner_consistent(cells, ci, k).is_some()).count();
        if n == 0 {
            out.extend_from_slice(&slots);
        }
    }
    for ei in 0..12 {
        let slots = EDGE_FACELETS[ei];
        if slots.iter().all(|&g| get(cells, g).is_some()) {
            continue;
        }
        let n = (0..12).filter(|&k| !used_e[k] && edge_consistent(cells, ei, k).is_some()).count();
        if n == 0 {
            out.extend_from_slice(&slots);
        }
    }
    out
}

/// **可行性判定**:当前这个(可能还没填满的)盘面,是否**还能补成一个合法魔方**?
///
/// 这是涂色时的前置闸门 —— 只要它不通过,那一笔就**不允许涂上去**,
/// 而不是等用户涂完 54 格再报错。
///
/// 判定内容(都是必要条件,合起来足够强):
/// 1. 每种颜色 **不超过** 9 格(超了永远补不回来)
/// 2. 已经确定的块必须是**合法块**(不能出现 WW / GG 这种;角块还要满足循环顺序)
/// 3. 已确定的块**互不重复**(同一个块不可能出现在两个位置)
/// 4. 每个位置都至少有一个候选块
/// 5. **角/棱各自存在完美匹配**(二分图匹配)—— 光看局部不够,这条能挡住
///    "剩下的块配不上剩下的位置"这类全局矛盾
/// 6. 若盘面已填满,再做完整的 5 条不变量校验(朝向和 / 奇偶性)
pub fn is_feasible(cells: &Cells) -> Result<(), String> {
    // 1) 配额
    let n = counts(cells);
    for c in COLOR_ORDER {
        if n[color_idx(c)] > 9 {
            return Err(format!("「{}」会有 {} 格(每种最多 9 格)", color_cn(c), n[color_idx(c)]));
        }
    }

    // 2/3) 已确定的块必须合法且不重复
    let used_c = used_corners(cells);
    let used_e = used_edges(cells);
    for ci in 0..8 {
        if CORNER_FACELETS[ci].iter().all(|&g| get(cells, g).is_some()) {
            let ok = (0..8).any(|k| {
                let cols = [
                    get(cells, CORNER_FACELETS[ci][0]).unwrap(),
                    get(cells, CORNER_FACELETS[ci][1]).unwrap(),
                    get(cells, CORNER_FACELETS[ci][2]).unwrap(),
                ];
                same_cyclic(&cols, &solved_corner_piece(k))
            });
            if !ok {
                let cols = [
                    get(cells, CORNER_FACELETS[ci][0]).unwrap(),
                    get(cells, CORNER_FACELETS[ci][1]).unwrap(),
                    get(cells, CORNER_FACELETS[ci][2]).unwrap(),
                ];
                return Err(format!(
                    "角位 {} 会是 {}{}{},不存在这样一个角块",
                    ci + 1, color_cn(cols[0]), color_cn(cols[1]), color_cn(cols[2])
                ));
            }
        }
    }
    for ei in 0..12 {
        if EDGE_FACELETS[ei].iter().all(|&g| get(cells, g).is_some()) {
            let a = get(cells, EDGE_FACELETS[ei][0]).unwrap();
            let b = get(cells, EDGE_FACELETS[ei][1]).unwrap();
            let ok = a != b && (0..12).any(|k| {
                let p = solved_edge_piece(k);
                (a == p[0] && b == p[1]) || (a == p[1] && b == p[0])
            });
            if !ok {
                return Err(format!("棱位 {} 会是 {}{},不存在这样一个棱块", ei + 1, color_cn(a), color_cn(b)));
            }
        }
    }
    let _ = (used_c, used_e);

    // 4/5) 候选 + 完美匹配
    let corner_cands: Vec<Vec<usize>> = (0..8)
        .map(|ci| (0..8).filter(|&k| corner_consistent(cells, ci, k).is_some()).collect())
        .collect();
    if corner_cands.iter().any(|v| v.is_empty()) {
        let bad = corner_cands.iter().position(|v| v.is_empty()).unwrap() + 1;
        return Err(format!("角位 {bad} 已经没有任何角块可放"));
    }
    if !has_perfect_matching(&corner_cands, 8) {
        return Err("剩余的角块无法覆盖剩余角位(块对不上)".into());
    }

    let edge_cands: Vec<Vec<usize>> = (0..12)
        .map(|ei| (0..12).filter(|&k| edge_consistent(cells, ei, k).is_some()).collect())
        .collect();
    if edge_cands.iter().any(|v| v.is_empty()) {
        let bad = edge_cands.iter().position(|v| v.is_empty()).unwrap() + 1;
        return Err(format!("棱位 {bad} 已经没有任何棱块可放"));
    }
    if !has_perfect_matching(&edge_cands, 12) {
        return Err("剩余的棱块无法覆盖剩余棱位(块对不上)".into());
    }

    // 6) 填满了就做完整校验(朝向 / 奇偶)
    if (0..54).all(|g| get(cells, g).is_some()) {
        validate(cells).map_err(|e| e.first().cloned().unwrap_or_else(|| "状态不合法".into()))?;
    }
    Ok(())
}

/// 把编辑器盘面转成 `CubeState`,**空格用该面的中心色补上**。
///
/// 这样"清空"在魔方上就表现为**六个纯色面**(一眼看出"还没填"),
/// 而涂过的格子会如实显示 —— 输入态的魔方渲染就靠它。
pub fn to_cube_state_lenient(cells: &Cells) -> cubr_core::model::CubeState {
    // 以还原态为模板,然后**逐格**取用户盘面的颜色;空格用该面中心色 ⇒ "纯色面 = 还没填"
    let mut st = cubr_core::core::CubeCore::solved().to_state();
    for f in 0..6 {
        let face = match f {
            0 => &mut st.U,
            1 => &mut st.R,
            2 => &mut st.F,
            3 => &mut st.D,
            4 => &mut st.L,
            _ => &mut st.B,
        };
        for i in 0..9 {
            face[i] = cells[f][i].unwrap_or(FACE_COLOR[f]);
        }
    }
    st
}

/// 某个格子**当前允许填哪些颜色**(逐色试涂 + 可行性判定)。
/// 取色器用它把"不能选的色"直接置灰 —— 避免用户点了没反应还不知道为什么。
pub fn allowed_colors(cells: &Cells, g: usize) -> [bool; 6] {
    let mut out = [false; 6];
    for (i, c) in COLOR_ORDER.iter().enumerate() {
        if can_paint(cells, g, *c).is_err() {
            continue;
        }
        let mut probe = *cells;
        set(&mut probe, g, Some(*c));
        out[i] = is_feasible(&probe).is_ok();
    }
    out
}

/// 还剩几个空格
pub fn empty_count(cells: &Cells) -> usize {
    (0..54).filter(|&g| get(cells, g).is_none()).count()
}

/// 快填满时(空格 ≤ 4)**穷举所有补全方案**,返回一个合法解。
///
/// 这是"唯一确定的颜色直接填上"的强力版本:不仅看局部推导,
/// 而是把**可解性**也当作约束 —— 只要剩下几格存在唯一/可行的合法补法,就直接填好。
///
/// - 返回 `Some(完整盘面)`:存在合法补全(已自动填上)
/// - 返回 `None` 且空格数 > 0:说明**怎么补都不合法**(盘面卡死了,需要 [`repair`])
pub fn find_completion(cells: &Cells) -> Option<Cells> {
    let empties: Vec<usize> = (0..54).filter(|&g| get(cells, g).is_none()).collect();
    if empties.is_empty() {
        return if validate(cells).is_ok() { Some(*cells) } else { None };
    }
    if empties.len() > 4 {
        return None; // 只在接近填满时才穷举(6^4 = 1296 种,毫秒级)
    }

    fn rec(cur: &mut Cells, empties: &[usize], i: usize, counts: &mut [u8; 6]) -> bool {
        if i == empties.len() {
            return validate(cur).is_ok();
        }
        let g = empties[i];
        for c in COLOR_ORDER {
            let ci = color_idx(c);
            if counts[ci] >= 9 {
                continue; // 配额剪枝
            }
            counts[ci] += 1;
            set(cur, g, Some(c));
            if rec(cur, empties, i + 1, counts) {
                return true;
            }
            counts[ci] -= 1;
            set(cur, g, None);
        }
        false
    }

    let mut cur = *cells;
    let mut cnt = counts(cells);
    if rec(&mut cur, &empties, 0, &mut cnt) {
        Some(cur)
    } else {
        None
    }
}

/// 盘面的"块级分析"结果(要求已填满且块合法)
pub struct Analysis {
    pub corner_of: [usize; 8],
    pub corner_ori: [u8; 8],
    pub edge_of: [usize; 12],
    pub edge_ori: [u8; 12],
}

/// 把已填满的盘面拆成"块 + 朝向";发现非法块或重复块时返回 Err
pub fn analyze(cells: &Cells) -> Result<Analysis, String> {
    let mut corner_of = [usize::MAX; 8];
    let mut corner_ori = [0u8; 8];
    let mut used_c = [false; 8];
    for ci in 0..8 {
        let cols = [
            get(cells, CORNER_FACELETS[ci][0]).ok_or("角位未填满")?,
            get(cells, CORNER_FACELETS[ci][1]).ok_or("角位未填满")?,
            get(cells, CORNER_FACELETS[ci][2]).ok_or("角位未填满")?,
        ];
        let mut hit = None;
        for k in 0..8 {
            let p = solved_corner_piece(k);
            if let Some(r) = (0..3).find(|&r| (0..3).all(|i| cols[i] == p[(i + r) % 3])) {
                hit = Some((k, r));
                break;
            }
        }
        match hit {
            None => {
                return Err(format!(
                    "角位 {} 的配色 {}{}{} 不是任何合法角块",
                    ci + 1, color_cn(cols[0]), color_cn(cols[1]), color_cn(cols[2])
                ))
            }
            Some((k, r)) => {
                if used_c[k] {
                    return Err(format!("角块出现了两次(序号 {})", k + 1));
                }
                used_c[k] = true;
                corner_of[ci] = k;
                corner_ori[ci] = r as u8;
            }
        }
    }

    let mut edge_of = [usize::MAX; 12];
    let mut edge_ori = [0u8; 12];
    let mut used_e = [false; 12];
    for ei in 0..12 {
        let a = get(cells, EDGE_FACELETS[ei][0]).ok_or("棱位未填满")?;
        let b = get(cells, EDGE_FACELETS[ei][1]).ok_or("棱位未填满")?;
        let mut hit = None;
        for k in 0..12 {
            let p = solved_edge_piece(k);
            if a == p[0] && b == p[1] {
                hit = Some((k, 0u8));
                break;
            }
            if a == p[1] && b == p[0] {
                hit = Some((k, 1u8));
                break;
            }
        }
        match hit {
            None => {
                return Err(format!(
                    "棱位 {} 的配色 {}{} 不是任何合法棱块",
                    ei + 1, color_cn(a), color_cn(b)
                ))
            }
            Some((k, o)) => {
                if used_e[k] {
                    return Err(format!("棱块出现了两次(序号 {})", k + 1));
                }
                used_e[k] = true;
                edge_of[ei] = k;
                edge_ori[ei] = o;
            }
        }
    }

    Ok(Analysis { corner_of, corner_ori, edge_of, edge_ori })
}

/// **强制修好**:把盘面变成"填满且合法"。
///
/// 与 [`repair`] 的区别:`repair` 只找**一处**最小改动,而朝向/奇偶是**三个独立**的不变量:
///
/// | 不变量 | 缺口 | 一次改动能否修 |
/// |---|---|---|
/// | 角扭转和 ≡ 0 (mod 3) | 可能非 0 | 扭转一个角 |
/// | 棱翻转和 ≡ 0 (mod 2) | 可能为奇 | 翻转一个棱 |
/// | 角排列奇偶 == 棱排列奇偶 | 可能不等 | 交换两个同类块 |
///
/// **同时**坏掉两个时,一处改动无论如何都修不好 —— 这正是"点修正没用"的原因。
/// 本函数改为:**先把空格按配额补齐 → 算出不变量缺口 → 逐个定向修好 → 复验**。
pub fn force_fix(cells: &mut Cells, derived: &mut [[bool; 9]; 6]) -> Result<String, String> {
    let empties: Vec<usize> = (0..54).filter(|&g| get(cells, g).is_none()).collect();
    if empties.len() > 4 {
        return Err(format!("空格较多({} 格),请先继续涂到接近填满", empties.len()));
    }

    // 枚举所有"颜色数量正好"的补齐方案,逐个尝试:
    //   a) 直接合法 → 采用
    //   b) 否则在补齐后的盘面上做**定向修正**(扭转角/翻转棱/交换同类块)→ 合法则采用
    let mut best: Option<(Cells, Vec<String>)> = None;
    let base = *cells;
    let mut cur = base;
    let mut cnt = counts(&base);

    fn rec(
        cur: &mut Cells,
        empties: &[usize],
        i: usize,
        cnt: &mut [u8; 6],
        best: &mut Option<(Cells, Vec<String>)>,
    ) {
        if best.is_some() {
            return;
        }
        if i == empties.len() {
            if let Some((fixed, log)) = try_fix_complete(cur) {
                *best = Some((fixed, log));
            }
            return;
        }
        let g = empties[i];
        for c in COLOR_ORDER {
            let ci = color_idx(c);
            if cnt[ci] >= 9 {
                continue;
            }
            cnt[ci] += 1;
            set(cur, g, Some(c));
            rec(cur, empties, i + 1, cnt, best);
            cnt[ci] -= 1;
            set(cur, g, None);
        }
    }
    rec(&mut cur, &empties, 0, &mut cnt, &mut best);

    let (fixed, log) = best.ok_or("把所有补齐方案都试过了,仍无法凑出合法魔方")?;
    *cells = fixed;
    for f in 0..6 {
        for i in 0..9 {
            if !cells[f][i].is_some() {
                derived[f][i] = true;
            }
        }
    }
    derived[0][CENTER_IDX] = false; // 中心不属于"推导"
    Ok(if log.is_empty() {
        "重新补成合法状态".to_string()
    } else {
        log.join(" + ")
    })
}

/// 在**已填满**的盘面上做定向修正,返回修好的盘面与说明
fn try_fix_complete(cells: &Cells) -> Option<(Cells, Vec<String>)> {
    if validate(cells).is_ok() {
        return Some((*cells, Vec::new()));
    }
    let mut fixes: Vec<String> = Vec::new();
    let mut cur = *cells;

    for _round in 0..8 {
        if validate(&cur).is_ok() {
            return Some((cur, fixes));
        }
        let a = match analyze(&cur) {
            Ok(a) => a,
            Err(_) => return None, // 块本身非法 → 这个补齐方案不行
        };
        let twist_sum: u32 = a.corner_ori.iter().map(|&x| x as u32).sum();
        let flip_sum: u32 = a.edge_ori.iter().map(|&x| x as u32).sum();
        let parity_ok = parity(&a.corner_of) == parity(&a.edge_of);

        // ① 奇偶:交换两个角位(**并自动选好朝向**,保证仍是合法块)
        if !parity_ok {
            if swap_corners_valid(&mut cur, 0, 1) {
                fixes.push("交换两个角位".into());
                continue;
            }
            if swap_edges_valid(&mut cur, 0, 1) {
                fixes.push("交换两个棱位".into());
                continue;
            }
            return None;
        }
        // ② 角扭转和
        if twist_sum % 3 != 0 {
            let need = ((3 - twist_sum % 3) % 3) as usize;
            let f = CORNER_FACELETS[0];
            let v: Vec<_> = f.iter().map(|&g| get(&cur, g).unwrap()).collect();
            for (k, &g) in f.iter().enumerate() {
                set(&mut cur, g, Some(v[(k + need) % 3]));
            }
            fixes.push("扭转一个角".into());
            continue;
        }
        // ③ 棱翻转和
        if flip_sum % 2 != 0 {
            let f = EDGE_FACELETS[0];
            let (x, y) = (get(&cur, f[0]).unwrap(), get(&cur, f[1]).unwrap());
            set(&mut cur, f[0], Some(y));
            set(&mut cur, f[1], Some(x));
            fixes.push("翻转一个棱".into());
            continue;
        }
        return None;
    }
    None
}

/// 交换两个角位的**块**,并为两边各自选出合法朝向(任一角块都能放到任一角位)
fn swap_corners_valid(cells: &mut Cells, i: usize, j: usize) -> bool {
    let (fi, fj) = (CORNER_FACELETS[i], CORNER_FACELETS[j]);
    let vi: Vec<_> = fi.iter().map(|&g| get(cells, g).unwrap()).collect();
    let vj: Vec<_> = fj.iter().map(|&g| get(cells, g).unwrap()).collect();
    // 找出 vi 属于哪个角块(循环匹配),同理 vj
    let find = |v: &[StickerColor]| -> Option<usize> {
        (0..8).find(|&k| {
            let p = solved_corner_piece(k);
            (0..3).any(|r| (0..3).all(|x| v[x] == p[(x + r) % 3]))
        })
    };
    let (Some(ki), Some(kj)) = (find(&vi), find(&vj)) else { return false };
    // 把 kj 放到位置 i:选旋转 r 使颜色对齐;再把 ki 放到位置 j
    let before = *cells;
    for r in 0..3 {
        let pj = solved_corner_piece(kj);
        for (x, &g) in fi.iter().enumerate() {
            set(cells, g, Some(pj[(x + r) % 3]));
        }
        for r2 in 0..3 {
            let pi = solved_corner_piece(ki);
            for (x, &g) in fj.iter().enumerate() {
                set(cells, g, Some(pi[(x + r2) % 3]));
            }
            if analyze(cells).is_ok() {
                return true;
            }
        }
    }
    *cells = before;
    false
}

/// 交换两个棱位的块(棱只有两个朝向,任一朝向都合法)
fn swap_edges_valid(cells: &mut Cells, i: usize, j: usize) -> bool {
    let f0 = EDGE_FACELETS[i];
    let f1 = EDGE_FACELETS[j];
    let v0 = (get(cells, f0[0]).unwrap(), get(cells, f0[1]).unwrap());
    let v1 = (get(cells, f1[0]).unwrap(), get(cells, f1[1]).unwrap());
    set(cells, f0[0], Some(v1.0));
    set(cells, f0[1], Some(v1.1));
    set(cells, f1[0], Some(v0.0));
    set(cells, f1[1], Some(v0.1));
    analyze(cells).is_ok()
}

/// **一步修正**:找出一个最小改动,让当前"卡死"的盘面重新变得可解。
///
/// 适用场景:用户涂到只剩最后一两格,却发现怎么涂都被拒 ——
/// 因为前面某处已经造成了**朝向/奇偶**层面的矛盾(例如单个棱被翻转),
/// 这种约束是全局的,只有在快填满时才暴露出来。
///
/// 依次尝试(找到第一个可行就返回说明):
/// 1. **翻转某个棱**(交换它两个贴纸)—— 修"棱翻转数为奇数"
/// 2. **扭转某个角**(三个贴纸轮换 1 或 2 次)—— 修"角扭转和不为 3 的倍数"
/// 3. **交换两个同类块的内容** —— 修"角/棱排列奇偶性不一致"
///
/// 成功的判据:`is_feasible` 通过,且自动推导补全后 `validate` 也通过。
pub fn repair(cells: &mut Cells, derived: &mut [[bool; 9]; 6]) -> Result<String, String> {
    // 本来就合法(且已填满)→ 无事可做,别乱改用户的输入
    if empty_count(cells) == 0 && validate(cells).is_ok() {
        return Ok("当前盘面本来就合法,无需修正".into());
    }

    let snap = *cells;
    let snap_d = *derived;

    // 1) 翻转棱
    for ei in 0..12 {
        let (a, b) = (EDGE_FACELETS[ei][0], EDGE_FACELETS[ei][1]);
        let (Some(ca), Some(cb)) = (get(cells, a), get(cells, b)) else { continue };
        set(cells, a, Some(cb));
        set(cells, b, Some(ca));
        if try_accept(cells, derived) {
            return Ok(format!("已翻转棱位 {} 的两个贴纸(原输入该处朝向不合法)", ei + 1));
        }
        *cells = snap;
        *derived = snap_d;
    }

    // 2) 扭转角
    for ci in 0..8 {
        let f = CORNER_FACELETS[ci];
        let (Some(c0), Some(c1), Some(c2)) = (get(cells, f[0]), get(cells, f[1]), get(cells, f[2])) else {
            continue;
        };
        for rot in 1..3 {
            set(cells, f[0], Some(if rot == 1 { c1 } else { c2 }));
            set(cells, f[1], Some(if rot == 1 { c2 } else { c0 }));
            set(cells, f[2], Some(if rot == 1 { c0 } else { c1 }));
            if try_accept(cells, derived) {
                return Ok(format!("已扭转角位 {}(原输入该处朝向不合法)", ci + 1));
            }
            *cells = snap;
            *derived = snap_d;
        }
    }

    // 3) 交换两个同类块(修奇偶)
    let swap_group = |cells: &mut Cells, slots: &[&[usize]], i: usize, j: usize| {
        let a = slots[i];
        let b = slots[j];
        let va: Vec<_> = a.iter().map(|&g| get(cells, g)).collect();
        let vb: Vec<_> = b.iter().map(|&g| get(cells, g)).collect();
        for (k, &g) in a.iter().enumerate() {
            set(cells, g, vb[k]);
        }
        for (k, &g) in b.iter().enumerate() {
            set(cells, g, va[k]);
        }
    };
    let corners: Vec<&[usize]> = CORNER_FACELETS.iter().map(|x| x.as_slice()).collect();
    let edges: Vec<&[usize]> = EDGE_FACELETS.iter().map(|x| x.as_slice()).collect();
    for (slots, label) in [(&corners, "角"), (&edges, "棱")] {
        let n = slots.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if slots[i].iter().any(|&g| get(cells, g).is_none())
                    || slots[j].iter().any(|&g| get(cells, g).is_none())
                {
                    continue;
                }
                swap_group(cells, slots, i, j);
                if try_accept(cells, derived) {
                    return Ok(format!("已交换{label}位 {} 与 {}(原输入块顺序不合法)", i + 1, j + 1));
                }
                *cells = snap;
                *derived = snap_d;
            }
        }
    }

    Err("没找到一步可修正的方案 —— 请用「擦除」清掉几格重涂,或点「清空」重来".into())
}

/// 试接受:`is_feasible` 通过 + 自动推导后(若填满)完整校验也通过
fn try_accept(cells: &mut Cells, derived: &mut [[bool; 9]; 6]) -> bool {
    if is_feasible(cells).is_err() {
        return false;
    }
    let mut c2 = *cells;
    let mut d2 = *derived;
    propagate(&mut c2, &mut d2);
    let left = empty_count(&c2);
    if left == 0 {
        // 填满了:必须完整合法
        if validate(&c2).is_err() {
            return false;
        }
    } else if left <= 4 {
        // 没填满也必须**存在合法补全**,否则这个"修正"等于没修
        // (早期版本漏了这条:可能接受一个仍然补不完的方案,用户按了修正也没用)
        if find_completion(&c2).is_none() {
            return false;
        }
    }
    *cells = c2;
    *derived = d2;
    true
}

/// 二分图完美匹配(Kuhn 算法;规模只有 8 / 12,递归深度很小)
fn has_perfect_matching(cands: &[Vec<usize>], n_right: usize) -> bool {
    fn try_kuhn(v: usize, cands: &[Vec<usize>], used: &mut [bool], mate: &mut [usize]) -> bool {
        for &to in &cands[v] {
            if used[to] {
                continue;
            }
            used[to] = true;
            if mate[to] == usize::MAX || try_kuhn(mate[to], cands, used, mate) {
                mate[to] = v;
                return true;
            }
        }
        false
    }
    let mut mate = vec![usize::MAX; n_right];
    for v in 0..cands.len() {
        let mut used = vec![false; n_right];
        if !try_kuhn(v, cands, &mut used, &mut mate) {
            return false;
        }
    }
    true
}

/// 完整校验:5 条不变量。`Ok(())` ⇒ 状态**合法可达**。
pub fn validate(cells: &Cells) -> Result<(), Vec<String>> {
    let mut errs: Vec<String> = Vec::new();

    // 0) 是否填满
    let unknown: Vec<usize> = (0..54).filter(|&g| get(cells, g).is_none()).collect();
    if !unknown.is_empty() {
        errs.push(format!("还有 {} 格未填", unknown.len()));
    }

    // 1) 中心固定
    for f in 0..6 {
        if let Some(c) = cells[f][CENTER_IDX] {
            if c != FACE_COLOR[f] {
                errs.push(format!(
                    "第 {} 面中心必须是 {}(当前 {})",
                    "URFDLB".chars().nth(f).unwrap(),
                    name(FACE_COLOR[f]),
                    name(c)
                ));
            }
        }
    }

    // 2) 数量各 9
    let n = counts(cells);
    if n.iter().any(|&x| x != 9) {
        let parts: Vec<String> = COLOR_ORDER
            .iter()
            .map(|&c| format!("{}:{}", name(c), n[color_idx(c)]))
            .collect();
        if n.iter().all(|&x| x <= 9) {
            errs.push(format!("颜色数量不对(每种需 9):{}", parts.join(" ")));
        } else {
            errs.push(format!("有颜色超过 9 格:{}", parts.join(" ")));
        }
    }

    // 局部矛盾:即使还没填满也能立刻发现"这块没地方放"
    errs.extend(contradictions(cells));

    if !unknown.is_empty() {
        return Err(errs); // 没填满时,后面的块/朝向检查没有意义
    }

    // 3) 块合法 + 收集排列
    let mut corner_of: Vec<Option<usize>> = vec![None; 8];
    let mut used_corner = vec![false; 8];
    for ci in 0..8 {
        let cols = [
            get(cells, CORNER_FACELETS[ci][0]).unwrap(),
            get(cells, CORNER_FACELETS[ci][1]).unwrap(),
            get(cells, CORNER_FACELETS[ci][2]).unwrap(),
        ];
        let mut hit = None;
        for k in 0..8 {
            if same_cyclic(&cols, &solved_corner_piece(k)) {
                hit = Some(k);
                break;
            }
        }
        match hit {
            None => errs.push(format!(
                "角位 {} 的配色 {}{}{} 不是任何合法角块",
                ci + 1,
                name(cols[0]),
                name(cols[1]),
                name(cols[2])
            )),
            Some(k) => {
                if used_corner[k] {
                    errs.push(format!("角块 {}{}{} 出现了两次", name(solved_corner_piece(k)[0]), name(solved_corner_piece(k)[1]), name(solved_corner_piece(k)[2])));
                }
                used_corner[k] = true;
                corner_of[ci] = Some(k);
            }
        }
    }

    let mut edge_of: Vec<Option<usize>> = vec![None; 12];
    let mut used_edge = vec![false; 12];
    for ei in 0..12 {
        let cols = [
            get(cells, EDGE_FACELETS[ei][0]).unwrap(),
            get(cells, EDGE_FACELETS[ei][1]).unwrap(),
        ];
        let mut hit = None;
        for k in 0..12 {
            let p = solved_edge_piece(k);
            if (cols[0] == p[0] && cols[1] == p[1]) || (cols[0] == p[1] && cols[1] == p[0]) {
                hit = Some(k);
                break;
            }
        }
        match hit {
            None => errs.push(format!("棱位 {} 的配色 {}{} 不是任何合法棱块", ei + 1, name(cols[0]), name(cols[1]))),
            Some(k) => {
                if used_edge[k] {
                    errs.push(format!("棱块 {}{} 出现了两次", name(solved_edge_piece(k)[0]), name(solved_edge_piece(k)[1])));
                }
                used_edge[k] = true;
                edge_of[ei] = Some(k);
            }
        }
    }

    if !errs.is_empty() {
        return Err(errs);
    }

    // 4) 朝向
    let mut co_sum = 0u32;
    for ci in 0..8 {
        let cols = [
            get(cells, CORNER_FACELETS[ci][0]).unwrap(),
            get(cells, CORNER_FACELETS[ci][1]).unwrap(),
            get(cells, CORNER_FACELETS[ci][2]).unwrap(),
        ];
        if let Some(k) = corner_of[ci] {
            let canon = solved_corner_piece(k);
            // 朝向 = 需要把哪个槽位的颜色转到第一个槽位(U/D 面)
            let ori = (0..3).find(|&s| cols[s] == canon[0]).unwrap_or(0);
            co_sum += ori as u32;
        }
    }
    if co_sum % 3 != 0 {
        errs.push(format!("角块朝向和不合法(和为 {},必须能被 3 整除)—— 典型:单个角块被扭转", co_sum));
    }

    let mut eo_sum = 0u32;
    for ei in 0..12 {
        let cols = [
            get(cells, EDGE_FACELETS[ei][0]).unwrap(),
            get(cells, EDGE_FACELETS[ei][1]).unwrap(),
        ];
        if let Some(k) = edge_of[ei] {
            let canon = solved_edge_piece(k);
            if cols[0] != canon[0] {
                eo_sum += 1;
            }
        }
    }
    if eo_sum % 2 != 0 {
        errs.push(format!("棱块朝向和不合法(翻转数 {})—— 典型:单个棱块被翻转", eo_sum));
    }

    // 5) 奇偶一致
    let cp: Vec<usize> = corner_of.iter().map(|x| x.unwrap()).collect();
    let ep: Vec<usize> = edge_of.iter().map(|x| x.unwrap()).collect();
    if parity(&cp) != parity(&ep) {
        errs.push("角块与棱块排列奇偶性不一致 —— 典型:只交换了两个块".into());
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

/// 两个 3 元组是否互为循环移位(角块朝向的判定)
fn same_cyclic(a: &[StickerColor; 3], b: &[StickerColor; 3]) -> bool {
    (0..3).any(|s| (0..3).all(|i| a[i] == b[(i + s) % 3]))
}

/// 排列奇偶性(true = 偶)
fn parity(p: &[usize]) -> bool {
    let mut seen = vec![false; p.len()];
    let mut odd = false;
    for i in 0..p.len() {
        if seen[i] {
            continue;
        }
        let mut len = 0;
        let mut j = i;
        while !seen[j] {
            seen[j] = true;
            j = p[j];
            len += 1;
        }
        if len % 2 == 0 {
            odd = !odd;
        }
    }
    !odd
}

/// 从 `CubeState` 载入盘面
pub fn from_cube_state(s: &cubr_core::model::CubeState) -> Cells {
    let faces = [s.U, s.R, s.F, s.D, s.L, s.B];
    let mut cells: Cells = [[None; 9]; 6];
    for f in 0..6 {
        for i in 0..9 {
            cells[f][i] = Some(faces[f][i]);
        }
    }
    cells
}

/// 组装成 `CubeState`(要求已填满)
pub fn to_cube_state(cells: &Cells) -> Option<cubr_core::model::CubeState> {
    let mut faces = [[StickerColor::W; 9]; 6];
    for f in 0..6 {
        for i in 0..9 {
            faces[f][i] = cells[f][i]?;
        }
    }
    Some(cubr_core::model::CubeState {
        U: faces[0],
        R: faces[1],
        F: faces[2],
        D: faces[3],
        L: faces[4],
        B: faces[5],
    })
}
// ─────────────────────── 自动化测试(不变量自检)───────────────────────

/// 跑一组校验用例;返回是否全部通过。
/// 由 `RUBIK_VALIDATE_TEST=1` 触发(见 main.rs),可在无图形环境下验证不变量。
pub fn run_selftest() -> bool {
    use cubr_core::core::CubeCore;
    use cubr_core::model::Move;

    let solved = cubr_core::model::CubeState::solved();

    // 调试:打印 cubr-core 实际的面块布局(用于核对我的下标表)
    if std::env::var("RUBIK_DEBUG").is_ok() {
        let ss = |st: &cubr_core::model::CubeState| -> String {
            let faces = [st.U, st.R, st.F, st.D, st.L, st.B];
            let mut out = String::new();
            for f in faces { for c in f { out.push_str(name(c)); } }
            out
        };
        println!("DBG solved : {}", ss(&solved));
        for mv in ["R", "U", "F", "D", "L", "B"] {
            let mut c = CubeCore::solved();
            c.apply(Move::parse(mv).unwrap());
            println!("DBG {mv}      : {}", ss(&c.to_state()));
        }
        println!("DBG 期望 R  : UUFUUFUUFRRRRRRRRRFFDFFDFFDDDBDDBDDBLLLLLLLLLUBBUBBUBB");
        println!("DBG 期望 U  : UUUUUUUUUBBBRRRRRRRRRFFFFFFDDDDDDDDDFFFLLLLLLLLLBBBBBB");
    }

    let mut pass = 0;
    let mut fail = 0;
    let mut check = |name: &str, got_valid: bool, want_valid: bool, detail: String| {
        let ok = got_valid == want_valid;
        if ok { pass += 1 } else { fail += 1 }
        println!(
            "  {} {:<42} 期望 {} 实际 {}  {}",
            if ok { "✅" } else { "❌" },
            name,
            if want_valid { "合法" } else { "非法" },
            if got_valid { "合法" } else { "非法" },
            detail
        );
    };

    // 1) 还原态
    let c = from_cube_state(&solved);
    check("还原态", validate(&c).is_ok(), true, String::new());

    // 2) 正常打乱(用求解器模型转出来的一定合法)
    let mut core = CubeCore::solved();
    for t in "R U2 F' L D B2 R' F U D2 L2".split_whitespace() {
        core.apply(Move::parse(t).unwrap());
    }
    let scrambled = core.to_state();
    let c = from_cube_state(&scrambled);
    let r = validate(&c);
    if std::env::var("RUBIK_DEBUG").is_ok() {
        let ss = |st: &cubr_core::model::CubeState| -> String {
            let faces = [st.U, st.R, st.F, st.D, st.L, st.B];
            let mut out = String::new();
            for f in faces { for c in f { out.push_str(name(c)); } }
            out
        };
        println!("DBG 打乱串  : {}", ss(&scrambled));
        println!("DBG 打乱态逐棱:");
        for ei in 0..12 {
            let cols = [get(&c, EDGE_FACELETS[ei][0]).unwrap(), get(&c, EDGE_FACELETS[ei][1]).unwrap()];
            let mut hit = None;
            for k in 0..12 {
                let p = solved_edge_piece(k);
                if (cols[0] == p[0] && cols[1] == p[1]) || (cols[0] == p[1] && cols[1] == p[0]) { hit = Some(k); break; }
            }
            println!("  棱{:>2} 面块{:>2},{:>2} 颜色 {}{} → 匹配 {:?} (规范 {}{})",
                ei, EDGE_FACELETS[ei][0], EDGE_FACELETS[ei][1], name(cols[0]), name(cols[1]),
                hit, hit.map(|k| name(solved_edge_piece(k)[0])).unwrap_or("?"), hit.map(|k| name(solved_edge_piece(k)[1])).unwrap_or("?"));
        }
    }
    check("10 步打乱态", r.is_ok(), true, r.err().map(|e| e.join(" | ")).unwrap_or_default());

    // 3) 单角扭转 → 角朝向和不合法
    let mut c = from_cube_state(&scrambled);
    let (a, b, d) = (c[0][8], c[1][0], c[2][2]); // URF 角
    c[0][8] = b; c[1][0] = d; c[2][2] = a;
    let r = validate(&c);
    check("单角扭转", r.is_ok(), false, r.err().map(|e| e[0].clone()).unwrap_or_default());

    // 4) 单棱翻转 → 棱朝向和不合法
    let mut c = from_cube_state(&scrambled);
    c[0][5] = Some(scrambled.R[1]); // UR 棱的两个贴纸互换
    c[1][1] = Some(scrambled.U[5]);
    let r = validate(&c);
    check("单棱翻转", r.is_ok(), false, r.err().map(|e| e[0].clone()).unwrap_or_default());

    // 5) 只交换两个棱 → 奇偶性不一致
    let mut c = from_cube_state(&scrambled);
    for (fa, ia, fb, ib) in [(0usize, 5usize, 0usize, 7usize), (1, 1, 2, 1)] {
        let t = c[fa][ia];
        c[fa][ia] = c[fb][ib];
        c[fb][ib] = t;
    }
    let r = validate(&c);
    check("只交换两个棱(奇偶)", r.is_ok(), false, r.err().map(|e| e[0].clone()).unwrap_or_default());

    // 6) 中心涂错色
    let mut c = from_cube_state(&solved);
    c[0][CENTER_IDX] = Some(StickerColor::R);
    let r = validate(&c);
    check("中心块涂错色", r.is_ok(), false, r.err().map(|e| e[0].clone()).unwrap_or_default());

    // 7) 未填满
    let mut c = from_cube_state(&solved);
    c[3][7] = None;
    let r = validate(&c);
    check("有未填格子", r.is_ok(), false, r.err().map(|e| e[0].clone()).unwrap_or_default());

    // 8) 即时拒绝:中心不可改
    let c = from_cube_state(&solved);
    let blocked = can_paint(&c, CENTER_IDX, StickerColor::R).is_err();
    check("涂色时拒绝改中心", blocked, true, String::new());

    // 9) 即时拒绝:对面色同块
    let mut c = from_cube_state(&solved);
    c[0][8] = None; // 清掉 URF 的 U 贴纸,再试图放白色对面(黄)旁边
    let blocked = can_paint(&c, 0 * 9 + 8, StickerColor::Y).is_err();
    check("拒绝对面色同块", blocked, true, String::new());

    // 10) 自动推导:还原态挖掉 URF 角的 F3 一格 ⇒ 由"其余块已占用"唯一确定应为 G
    let mut c = from_cube_state(&solved);
    let (f20, i20) = split(20);
    c[f20][i20] = None;
    let mut der = [[false; 9]; 6];
    propagate(&mut c, &mut der);
    let got = get(&c, 20);
    check(
        "挖空一格后自动推出 F3=G",
        got == Some(StickerColor::G),
        true,
        format!("推出 F3={:?}", got.map(name)),
    );

    // 10b) 只给 1 色 ⇒ 4 个候选块,不该乱猜,保持未填
    let mut c: Cells = [[None; 9]; 6];
    for f in 0..6 { c[f][CENTER_IDX] = Some(FACE_COLOR[f]); }
    set(&mut c, 8, Some(StickerColor::W)); // 只有 URF 的 U 位是白
    let mut der = [[false; 9]; 6];
    propagate(&mut c, &mut der);
    check(
        "信息不足时不猜(保持未填)",
        get(&c, 20).is_none() && get(&c, 9).is_none(),
        true,
        format!("R1={:?} F3={:?}", get(&c, 9).map(name), get(&c, 20).map(name)),
    );

    // 10c) 2 色 + 顺序 ⇒ 唯一确定(这正是"顺序约束"的价值)
    let mut c: Cells = [[None; 9]; 6];
    for f in 0..6 { c[f][CENTER_IDX] = Some(FACE_COLOR[f]); }
    set(&mut c, 8, Some(StickerColor::W));
    set(&mut c, 9, Some(StickerColor::R));
    let mut der = [[false; 9]; 6];
    propagate(&mut c, &mut der);
    check(
        "2 色 + 相对顺序 ⇒ 推出 F3=G",
        get(&c, 20) == Some(StickerColor::G),
        true,
        format!("F3={:?}", get(&c, 20).map(name)),
    );

    // 10e) 配额规则:构造"只剩 1 格空着"的局面,必须自动补上最缺的颜色
    {
        let mut c = from_cube_state(&solved);
        // 把 U 面的一个白贴纸(U9→index 8)与 F 面的绿贴纸(F3→index 20)都挖掉,
        // 再把 R1(白)涂成绿 —— 于是 W 缺 1、G 缺 1,但空格只有 2 个,尚未确定;
        // 再补涂一格让它变成唯一
        let (f8, i8) = split(8);
        c[f8][i8] = None; // 少一个白
        let mut der = [[false; 9]; 6];
        propagate(&mut c, &mut der);
        let left = (0..54).filter(|&g| get(&c, g).is_none()).count();
        let got = get(&c, 8);
        check(
            "少一格白 ⇒ 自动补回白",
            left == 0 && got == Some(StickerColor::W),
            true,
            format!("空格 {} 补回 {:?}", left, got.map(name)),
        );
    }

    // 10f) 盘面已满(替换模式):涂已满颜色不应被拦
    let c = from_cube_state(&solved);
    let allow_replace = can_paint(&c, 0 * 9 + 8, StickerColor::R).is_ok(); // 把 U9(白)改成红
    check("满盘时允许替换贴纸", allow_replace, true, String::new());

    // 10g) 填色模式:涂已满颜色应被拦(并提示还可用哪些色)
    let mut c = from_cube_state(&solved);
    let (fg, ig) = split(20); // F3 = 绿
    c[fg][ig] = None; // 制造一个空格(清掉的是绿,所以白仍是 9 满)
    let r = can_paint(&c, 1 * 9 + 0, StickerColor::W); // R1 涂白 → 白会变 10,应被拦
    check(
        "填色模式下拦截已满颜色",
        r.is_err(),
        true,
        r.err().unwrap_or_default(),
    );

    // 12) 可行性闸门:不该允许"涂进去再说"的局面
    {
        // (a) 会把某色涂成 10 格 → 必须拒绝
        let mut c = from_cube_state(&solved);
        let (f, i) = split(20); // F3 = 绿
        c[f][i] = None;         // 现在绿=8、其它=9
        let (f2, i2) = split(21); // F4 = 绿
        c[f2][i2] = Some(StickerColor::O); // 把绿改成橙 → 橙会变 10
        let r = is_feasible(&c);
        check("拒绝:某色超过 9 格", r.is_ok(), false, r.err().unwrap_or_default());
    }
    {
        // (b) 造出不存在的角块(两个白) → 必须拒绝
        let mut c: Cells = [[None; 9]; 6];
        for f in 0..6 { c[f][CENTER_IDX] = Some(FACE_COLOR[f]); }
        set(&mut c, 8, Some(StickerColor::W)); // URF: U9 = 白
        set(&mut c, 9, Some(StickerColor::W)); // R1 = 白  ← 同角两块都是白,不存在
        let r = is_feasible(&c);
        check("拒绝:角块两格同色(不存在该块)", r.is_ok(), false, r.err().unwrap_or_default());
    }
    {
        // (c) 同一个块出现两次 → 必须拒绝
        let mut c: Cells = [[None; 9]; 6];
        for f in 0..6 { c[f][CENTER_IDX] = Some(FACE_COLOR[f]); }
        // URF 角 = 白/红/绿
        set(&mut c, 8, Some(StickerColor::W));
        set(&mut c, 9, Some(StickerColor::R));
        set(&mut c, 20, Some(StickerColor::G));
        // 另一个角位(UBR: U3/B1/R3)也涂成 白/红/绿
        set(&mut c, 2, Some(StickerColor::W));
        set(&mut c, 45, Some(StickerColor::R));
        set(&mut c, 11, Some(StickerColor::G));
        let r = is_feasible(&c);
        check("拒绝:同一个角块用了两次", r.is_ok(), false, r.err().unwrap_or_default());
    }
    {
        // (d) 合法的一笔 → 必须放行
        let mut c: Cells = [[None; 9]; 6];
        for f in 0..6 { c[f][CENTER_IDX] = Some(FACE_COLOR[f]); }
        set(&mut c, 8, Some(StickerColor::W));
        set(&mut c, 9, Some(StickerColor::R));
        set(&mut c, 20, Some(StickerColor::G));
        let r = is_feasible(&c);
        check("放行:合法的一笔", r.is_ok(), true, r.err().unwrap_or_default());
    }
    {
        // (e) 还原态挖掉一格 → 仍然可行(稍后会被自动推导补回)
        let mut c = from_cube_state(&solved);
        let (f, i) = split(20);
        c[f][i] = None;
        let r = is_feasible(&c);
        check("放行:挖掉一格仍可行", r.is_ok(), true, r.err().unwrap_or_default());
    }
    {
        // (f) 填满后朝向不对(单角扭转) → 必须拒绝
        let mut c = from_cube_state(&solved);
        let (a, b, d) = (c[0][8], c[1][0], c[2][2]);
        c[0][8] = b; c[1][0] = d; c[2][2] = a;
        let r = is_feasible(&c);
        check("拒绝:填满后单角扭转", r.is_ok(), false, r.err().unwrap_or_default());
    }

    {
        // (g) 走编辑器真实入口:非法的一笔必须被拒绝**且盘面不变**(回滚生效)
        let mut ed = super::editor::EditState::default(); // 还原态
        let before = ed.cells;
        ed.paint(2, 2, StickerColor::O); // F3(绿)想涂成橙,而橙已满 9
        let rolled_back = ed.cells == before;
        check(
            "编辑器入口:非法涂色被拒且已回滚",
            rolled_back && ed.message.contains("不能涂"),
            true,
            format!("盘面未变={rolled_back} 提示={}", ed.message),
        );

        // (h) 擦除不会让状态变得不可行;擦掉的格子还能涂回原色
        let mut ed = super::editor::EditState::default(); // 还原态
        ed.erase(2, 2); // F3(绿)
        let still_feasible = is_feasible(&ed.cells).is_ok();
        ed.paint(2, 2, StickerColor::G); // 涂回绿
        let repainted = !ed.message.contains("不能涂");
        check(
            "擦除后仍可行,且能涂回原色",
            still_feasible && repainted,
            true,
            format!("擦后可行={still_feasible} 涂回成功={repainted}"),
        );

        // (i) URF 角上已知 W(U9)+R(R1) 时,第 3 格**只能**是绿 —— 涂别的色必须拒绝
        //     (这是个很好的例子:合法性不只是"颜色数量",还受**手性/朝向**约束)
        let mut ed = super::editor::EditState::default();
        ed.erase(2, 2); // F3 = 绿,先清掉
        ed.paint(2, 2, StickerColor::B); // 想涂成蓝 → W,R,B 这个角块手性不对
        let rejected_b = ed.message.contains("不能涂");
        ed.paint(2, 2, StickerColor::G); // 涂成绿 → 唯一正确解
        let accepted_g = !ed.message.contains("不能涂");
        check(
            "URF 第 3 色只能是绿(手性约束)",
            rejected_b && accepted_g,
            true,
            format!("涂蓝被拒={rejected_b} 涂绿放行={accepted_g}"),
        );
    }

    // 13) 一步修正:把"卡死"的盘面救回来
    {
        // 场景 A:整盘填满但单个棱被翻转 → 无解
        let mut c = from_cube_state(&solved);
        let (a, b) = (EDGE_FACELETS[0][0], EDGE_FACELETS[0][1]);
        let (ca, cb) = (get(&c, a).unwrap(), get(&c, b).unwrap());
        set(&mut c, a, Some(cb));
        set(&mut c, b, Some(ca));
        let invalid_before = validate(&c).is_err();
        let mut der = [[false; 9]; 6];
        let msg = repair(&mut c, &mut der);
        let ok_after = validate(&c).is_ok();
        check(
            "修正:单棱翻转被自动修好",
            invalid_before && msg.is_ok() && ok_after,
            true,
            format!("修正前非法={invalid_before} 修正后合法={ok_after} / {}", msg.unwrap_or_default()),
        );
    }
    {
        // 场景 B:单个角被扭转 → 无解
        let mut c = from_cube_state(&solved);
        let f = CORNER_FACELETS[0];
        let (c0, c1, c2) = (c[f[0] / 9][f[0] % 9], c[f[1] / 9][f[1] % 9], c[f[2] / 9][f[2] % 9]);
        c[f[0] / 9][f[0] % 9] = c1;
        c[f[1] / 9][f[1] % 9] = c2;
        c[f[2] / 9][f[2] % 9] = c0;
        let invalid_before = validate(&c).is_err();
        let mut der = [[false; 9]; 6];
        let msg = repair(&mut c, &mut der);
        let ok_after = validate(&c).is_ok();
        check(
            "修正:单角扭转被自动修好",
            invalid_before && msg.is_ok() && ok_after,
            true,
            format!("修正前非法={invalid_before} 修正后合法={ok_after} / {}", msg.unwrap_or_default()),
        );
    }
    {
        // 场景 C:只交换两个棱(奇偶不对) → 无解
        let mut c = from_cube_state(&solved);
        let (a, b) = (EDGE_FACELETS[0].to_vec(), EDGE_FACELETS[1].to_vec());
        let va: Vec<_> = a.iter().map(|&g| get(&c, g)).collect();
        let vb: Vec<_> = b.iter().map(|&g| get(&c, g)).collect();
        for (k, &g) in a.iter().enumerate() { set(&mut c, g, vb[k]); }
        for (k, &g) in b.iter().enumerate() { set(&mut c, g, va[k]); }
        let invalid_before = validate(&c).is_err();
        let mut der = [[false; 9]; 6];
        let msg = repair(&mut c, &mut der);
        let ok_after = validate(&c).is_ok();
        check(
            "修正:两块交换(奇偶)被自动修好",
            invalid_before && msg.is_ok() && ok_after,
            true,
            format!("修正前非法={invalid_before} 修正后合法={ok_after} / {}", msg.unwrap_or_default()),
        );
    }
    {
        // 场景 D:合法的完整盘面不该被"修正"动到
        let mut c = from_cube_state(&scrambled);
        let before = c;
        let mut der = [[false; 9]; 6];
        let r = repair(&mut c, &mut der);
        check(
            "修正:合法盘面保持不变",
            r.is_ok() && c == before,
            true,
            format!("{r:?}"),
        );
    }
    {
        // 场景 E(复现用户遇到的情况):单棱翻转 + 还剩 2 格没填
        //   → find_completion 应当判定"补不了"(卡死)
        //   → repair 修好之后,find_completion 应当能补完
        let mut c = from_cube_state(&solved);
        let (a, b) = (EDGE_FACELETS[0][0], EDGE_FACELETS[0][1]);
        let (ca, cb) = (get(&c, a).unwrap(), get(&c, b).unwrap());
        set(&mut c, a, Some(cb));
        set(&mut c, b, Some(ca));
        // 挖掉两个绿格(模拟"还剩 2 格没填")
        let g1 = CORNER_FACELETS[0][2]; // F3
        let g2 = 21; // F4
        set(&mut c, g1, None);
        set(&mut c, g2, None);

        let stuck = empty_count(&c) == 2 && find_completion(&c).is_none();
        let mut der = [[false; 9]; 6];
        let msg = repair(&mut c, &mut der);
        let fixed = find_completion(&c);
        check(
            "卡死盘面:判定→修正→自动补全",
            stuck && msg.is_ok() && fixed.is_some(),
            true,
            format!(
                "判定卡死={stuck} 修正={} 补全={}",
                msg.unwrap_or_default(),
                fixed.is_some()
            ),
        );
    }
    {
        // 场景 E2(复现用户截图):单个角被扭转 + 还剩 2 格(缺蓝、缺绿)
        let mut c = from_cube_state(&solved);
        // 扭转 URF 角
        let f = CORNER_FACELETS[0];
        let (c0, c1, c2) = (get(&c, f[0]).unwrap(), get(&c, f[1]).unwrap(), get(&c, f[2]).unwrap());
        set(&mut c, f[0], Some(c1));
        set(&mut c, f[1], Some(c2));
        set(&mut c, f[2], Some(c0));
        // 挖掉一个蓝格(B1)与一个绿格(F1)
        set(&mut c, 45, None);
        set(&mut c, 18, None);
        let cnt = counts(&c);
        let stuck = find_completion(&c).is_none();
        let mut der = [[false; 9]; 6];
        let msg = repair(&mut c, &mut der);
        let fixed = find_completion(&c);
        let fixed_ok = fixed.as_ref().map(|d| validate(d).is_ok()).unwrap_or(false);
        check(
            "卡死(角扭转+缺2格)能被修正并补全",
            stuck && msg.is_ok() && fixed_ok,
            true,
            format!(
                "计数 B{} G{} 判定卡死={stuck} 修正={} 补全合法={fixed_ok}",
                cnt[4], cnt[5],
                msg.unwrap_or_default()
            ),
        );
    }

    {
        // 场景 F:正常只差 2 格 → 应该直接补全并合法
        let mut c = from_cube_state(&solved);
        set(&mut c, 20, None); // F3
        set(&mut c, 21, None); // F4
        let done = find_completion(&c);
        let ok = done.as_ref().map(|d| validate(d).is_ok()).unwrap_or(false);
        check("只差 2 格时能自动补出合法解", ok, true, format!("补全={}", done.is_some()));
    }

    {
        // 场景 G(端到端,复现用户截图):从还原态出发,扭转一个角,
        // 再挖掉一个蓝格和一个绿格 ⇒ 剩下 2 格怎么涂都会被拒;
        // 走 EditState 的真实入口,应当**自动修正 + 自动补全**
        let mut ed = super::editor::EditState::default();
        // 直接改盘面(绕过逐笔校验,模拟"涂错了才发现")
        let f = CORNER_FACELETS[0];
        let (c0, c1, c2) = (get(&ed.cells, f[0]).unwrap(), get(&ed.cells, f[1]).unwrap(), get(&ed.cells, f[2]).unwrap());
        set(&mut ed.cells, f[0], Some(c1));
        set(&mut ed.cells, f[1], Some(c2));
        set(&mut ed.cells, f[2], Some(c0));
        set(&mut ed.cells, 45, None); // 蓝
        set(&mut ed.cells, 18, None); // 绿
        ed.derived = [[false; 9]; 6];
        ed.rederive_public(); // 触发推导 + 自动修正
        let filled = empty_count(&ed.cells) == 0;
        let valid_now = validate(&ed.cells).is_ok();
        check(
            "端到端:卡死盘面自动修正并补全",
            filled && valid_now,
            true,
            format!("填满={filled} 合法={valid_now} 提示={}", ed.message),
        );
    }

    {
        // 场景 H(复现用户情况):**同时**坏两个不变量 —— 角被扭转 + 棱被翻转,还缺 2 格
        //   → 一处改动修不好(repair 必然失败)
        //   → force_fix 必须能定向修好
        let mut c = from_cube_state(&solved);
        // (1) 扭转 URF 角
        let f = CORNER_FACELETS[0];
        let (c0, c1, c2) = (get(&c, f[0]).unwrap(), get(&c, f[1]).unwrap(), get(&c, f[2]).unwrap());
        set(&mut c, f[0], Some(c1));
        set(&mut c, f[1], Some(c2));
        set(&mut c, f[2], Some(c0));
        // (2) 翻转 UR 棱
        let e = EDGE_FACELETS[0];
        let (e0, e1) = (get(&c, e[0]).unwrap(), get(&c, e[1]).unwrap());
        set(&mut c, e[0], Some(e1));
        set(&mut c, e[1], Some(e0));
        // (3) 挖掉一个蓝格 + 一个绿格
        set(&mut c, 45, None);
        set(&mut c, 18, None);

        let mut der = [[false; 9]; 6];
        let one_shot = repair(&mut c, &mut der); // 一处修正:应当失败
        let failed = one_shot.is_err();
        // 注意 repair 失败时会还原快照,所以 c 仍是被破坏的状态
        let mut der2 = [[false; 9]; 6];
        let deep = force_fix(&mut c, &mut der2);
        let ok_after = validate(&c).is_ok();
        check(
            "同时坏两个不变量:一处修正失败 → 定向修正成功",
            failed && deep.is_ok() && ok_after,
            true,
            format!(
                "一处修正={:?} 定向修正={} 结果合法={ok_after}",
                one_shot.err(),
                deep.unwrap_or_default()
            ),
        );
    }

    {
        // 场景 I(端到端,复现用户最终情况):角扭转 + 棱翻转 + 缺 2 格
        //   走编辑器入口,必须自动修好并补满
        let mut ed = super::editor::EditState::default();
        let f = CORNER_FACELETS[0];
        let (c0, c1, c2) = (get(&ed.cells, f[0]).unwrap(), get(&ed.cells, f[1]).unwrap(), get(&ed.cells, f[2]).unwrap());
        set(&mut ed.cells, f[0], Some(c1));
        set(&mut ed.cells, f[1], Some(c2));
        set(&mut ed.cells, f[2], Some(c0));
        let e = EDGE_FACELETS[0];
        let (e0, e1) = (get(&ed.cells, e[0]).unwrap(), get(&ed.cells, e[1]).unwrap());
        set(&mut ed.cells, e[0], Some(e1));
        set(&mut ed.cells, e[1], Some(e0));
        set(&mut ed.cells, 45, None);
        set(&mut ed.cells, 18, None);
        ed.derived = [[false; 9]; 6];
        ed.rederive_public();
        let filled = empty_count(&ed.cells) == 0;
        let valid_now = validate(&ed.cells).is_ok();
        check(
            "端到端:双违规盘面自动修好并补满",
            filled && valid_now,
            true,
            format!("填满={filled} 合法={valid_now} 提示={}", ed.message),
        );
    }

    {
        // 编辑器层面:清空后也不能随便涂 —— 配额与块合法性必须仍然生效
        let mut ed = super::editor::EditState::default();
        ed.clear();
        // U 面 9 格全涂白(合法:每色 9 格)
        for i in 0..9 {
            ed.paint(0, i, StickerColor::W);
        }
        let after_nine = (0..9).filter(|&i| ed.cells[0][i] == Some(StickerColor::W)).count();
        // 第 10 个白必然超配额 ⇒ 必须被拒
        ed.paint(1, 0, StickerColor::W);
        let ten_rejected = ed.cells[1][0] != Some(StickerColor::W);
        // 非法的角块组合也必须被拒(URF 三个面格不能是 白白白)
        ed.clear();
        ed.paint(0, 8, StickerColor::W); // URF 的 U 面
        ed.paint(1, 0, StickerColor::W); // URF 的 R 面
        ed.paint(2, 2, StickerColor::W); // URF 的 F 面 —— 白角块不存在 ⇒ 必须被拒
        let bad_corner_rejected = ed.cells[0][8] != Some(StickerColor::W)
            || ed.cells[1][0] != Some(StickerColor::W)
            || ed.cells[2][2] != Some(StickerColor::W);
        check(
            "编辑器:配额与块合法性约束仍然生效",
            after_nine == 9 && ten_rejected && bad_corner_rejected,
            true,
            format!("9格填满={after_nine} 第10格被拒={ten_rejected} 非法角被拒={bad_corner_rejected}"),
        );
    }

    // 11) 文本解析:合法串
    let ok = super::editor::parse_facelets("UUUUUUUUURRRRRRRRRFFFFFFFFFDDDDDDDDDLLLLLLLLLBBBBBBBBB").is_ok();
    check("解析还原态字符串", ok, true, String::new());

    // 12) 文本解析:非法串(中心错)应被拒
    let bad = "RUUUUUUUURRRRRRRRRFFFFFFFFFDDDDDDDDDLLLLLLLLLBBBBBBBBB";
    let rejected = super::editor::parse_facelets(bad).is_err();
    check("拒绝非法状态字符串", rejected, true, String::new());

    println!("\n  不变量自检:{pass} 通过 / {fail} 失败");
    fail == 0
}
