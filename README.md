# 魔方求解器(Bevy 0.19 + cubr-core)

任意初始状态的 3×3×3 魔方 → **≤20 步**解法 → **3D 动画还原**,全程可暂停 / 单步 / 回退。

```
┌──────────────────────────────────────────────┐
│ 魔方: 已打乱        求解器: ✅ 解法 20 步      │
│ 播放: ▶ 播放中 R2   进度: 3/20                │
│                                              │
│ 解法(≤20 步,当前步用 [ ] 标出):              │
│   (R2) (U2) [F'] R B2 U B' L D B' U F' …      │
│                                              │
│ [空格]暂停/继续 [→]单步 [←]回退 [S]求解       │
└──────────────────────────────────────────────┘
```

---

## 特性

| 特性 | 说明 |
|---|---|
| **≤20 步保证** | 由 God's Number = 20 保证;求解器实现见下文"两级保证" |
| **任意初始状态** | 不需要"从还原态打乱",任何合法局面都能求解(非法局面会被检测并拒绝) |
| **3D 动画还原** | 每一步都演出"某一层绕轴转 90°×n",带缓动 |
| **暂停 / 单步 / 回退** | 空格暂停;`→` 单步;`←` 回退(执行逆转动) |
| **不卡 UI** | 求解在后台线程;首次启动生成模式数据库(约 15 秒)时界面依然响应 |
| **实时调速** | `[` / `]` 调整每步动画时长(0.08 ~ 2.0 秒) |
| **视角环绕** | `A` / `D` 绕魔方旋转观察 |

---

## 快速开始

```bash
# 需要 Rust 1.85+(本项目在 1.97 上验证)
cd rubik-bevy
cargo run --release
```

> ⚠️ **务必用 `--release`**:debug 下魔方搜索会慢到不可用。
> 如果只想快速检查编译:`cargo check`。

### 平台备注

| 平台 | 说明 |
|---|---|
| **Windows** | 直接 `cargo run --release` |
| **WSL2(WSLg)** | 默认走 Vulkan;若报显卡后端错误,试 `WGPU_BACKEND=gl cargo run --release` |
| **Linux** | 需要 `libudev` / `libasound2` 等 Bevy 常规依赖 |

**首次运行**会在磁盘缓存 Korf 模式数据库(约 15 秒生成,之后秒开)。
缓存位置由 `cubr-core` 决定(见其 `solver::cache` 模块),删除缓存会触发重新生成。

---

## 🎮 如何输入初始状态(三种方式)

| 方式 | 怎么用 | 适用场景 |
|---|---|---|
| **① 图形化展开图编辑器** | 右侧面板点格子改色 → `Apply` | 交互式,照着实物涂 |
| **② 命令行参数** | `cargo run --release -- --state <54字符>` | 脚本 |
| **③ 环境变量** | `RUBIK_STATE=<54字符> cargo run --release` | CI / 自检 |
| ④ 随机打乱(附带) | 按 `X` | 快速试玩 |

### ① 展开图编辑器(默认显示,按 `E` 隐藏)

```
        U
    L   F   R   B
        D
```

- 默认盘面是**还原态**配色,可直接微调;`Clear` 得到全白空白盘
- 先在下方**六色调色板**选颜色(选中项描白边),再点格子涂色
- 状态行实时校验:必须**每种颜色各 9 格**才能 `Apply`,否则提示缺多少
- `From cube` 把当前魔方读进编辑器;`Apply` 应用到魔方(会清空当前解法)
- 应用后按 `S` 求解

### 合法性保证(重要)

魔方状态**不是 54 个格子随便涂**。程序按 5 条**充要**不变量校验(见 `src/validate.rs`):

| # | 不变量 | 违反时的提示 |
|---|---|---|
| 1 | **中心固定** —— 6 个中心块永不移动,必须是各面标准色 | `第 U 面中心必须是 W(当前 R)` |
| 2 | **数量** —— 每种颜色恰好 9 格 | `有颜色超过 9 格:W:8 R:10 …` |
| 3 | **块合法** —— 8 角块 / 12 棱块必须恰好是合法块集合的一个排列(不能重复、不能造块) | `角位 3 的配色 ROB 不是任何合法角块` |
| 4 | **朝向** —— 角和 ≡ 0 (mod 3)、棱和 ≡ 0 (mod 2) | `角块朝向和不合法(和为 8…)—— 典型:单个角块被扭转` |
| 5 | **奇偶** —— 角排列奇偶 == 棱排列奇偶 | `角块与棱块排列奇偶性不一致 —— 典型:只交换了两个块` |

**三层防护**:

| 层级 | 时机 | 做什么 |
|---|---|---|
| **即时拒绝** | 点击瞬间 | 中心不可改;同一角/棱上不能出现**对面色**;某色已满 9 格不能再涂 |
| **不可行就回滚** ⭐ | 点击瞬间 | 试探性应用 → **可行性判定** → 不可行则**整笔回滚**(见下) |
| **自动推导** | 每次涂色后 | 用**块级约束传播**填出能唯一确定的格子(青色边框 + 压暗显示) |
| **整体校验** | 每次修改后 | 跑完 5 条不变量;不通过则状态行给出**具体原因**,`Apply` 被拒绝 |

### 「不合理的情况当初就不允许填入」

这一条是**硬要求**:不能等用户涂完 54 格才告诉他"这状态不合法"。做法是每次点击都:

```
① 试探性把这一笔涂上去
② 跑 is_feasible():这个盘面还能不能补成一个合法魔方?
③ 不能 → 整笔回滚(盘面、推导标记全部还原),并说明原因
```

`is_feasible()` 的判定内容:

| # | 条件 | 拦住什么 |
|---|---|---|
| 1 | 每种颜色 **≤ 9 格** | 涂出 10 个橙色 |
| 2 | 已确定的块必须是**合法块** | `RWW` / `GG` 这种不存在的块 |
| 3 | 已确定的块**互不重复** | 同一个块出现两次 |
| 4 | 每个位置至少有 1 个候选块 | 某位置已经没块可放 |
| 5 | 角/棱各自存在**完美匹配**(Kuhn 二分图匹配) | "剩下的块配不上剩下的位置"这类**全局**矛盾 |
| 6 | 填满后再跑完整 5 条不变量 | 单角扭转 / 单棱翻转 / 奇偶错误 |

实测拒绝示例:

```
【不能涂】「橙」会有 10 格(每种最多 9 格)
【不能涂】角位 1 已经没有任何角块可放
【不能涂】剩余的角块无法覆盖剩余角位(块对不上)
【不能涂】同一角块上不能同时有 O 和 R(对面色)
【不能涂】角块朝向和不合法(和为 2,必须能被 3 整除)—— 典型:单个角块被扭转
```

> 💡 **一个有意思的例子**:在 URF 角上如果 U 位已定白、R 位已定红,那么第三格**只能是绿**
> —— 涂蓝会被拒绝,因为 `白蓝红` 这个角块的**手性**不对(角块能换位置,但手性不能翻转)。
> 校验器连这种事都管住了(已作为测试用例:见 26 项自检里的「URF 第 3 色只能是绿」)。

### 边框颜色速查(为什么有的格子带框、有的不带)

| 边框 | 含义 |
|---|---|
| **金色(粗)** | **中心块** —— 固定不可改(它定义配色方案) |
| **青色** | **程序自动推导**填出来的格子(不是你手涂的);底色也会压暗一档 |
| **灰色** | **你手涂的**格子 |
| **品红(3px 粗)** | 该位置**已矛盾**(没有任何合法块可放)。刻意避开 6 种贴纸色 —— 原来用红色会和「红色贴纸」混淆 |

面板顶部有对应的**色块图例**。推导格会在你改动其它格子后自动重算 ——
所以同一格可能从"手涂"变成"推导"(或反过来),边框颜色也跟着变。

### 快填满时会自动补全 / 自动修正(**不需要你操作**)

空格少(≤ 4 格)时,每次改动后都会自动走一遍收尾逻辑:

| 情况 | 处理 |
|---|---|
| 没填满,但**存在合法补全** | **直接补完**(唯一合法解),不用你再点 |
| 已填满但**非法** | 判定为卡死 → **自动修正一处** → 再补全 |
| 没填满且**怎么补都不合法** | 判定为卡死 → **自动修正一处** → 再补全 |

修正完成后状态行会说明改了什么,例如:

```
盘面原本无解(全局约束,涂的过程中查不出)—— 已扭转角位 1(原来那格朝向不对);已全部补全
```

> ⚠️ **三个踩过的坑**(都已修 + 有回归测试):
> 1. **顺序陷阱**:`propagate` 可能**先把剩余格子填满**(填成非法状态),
>    所以不能只看"还剩几格"就决定要不要修正 —— 必须区分"已填满但非法"这种情况。
> 2. **修正的接受条件**:早期版本会接受"修完仍然补不完"的方案,
>    用户按了「修正」却依然填不进去。现在要求:修正后**必须存在合法补全**(或已完整合法)。
> 3. **一处改动修不好两个不变量**(最坑的一个):朝向/奇偶是**三个独立**的不变量:
>
>    | 不变量 | 缺口 | 一次改动 |
>    |---|---|---|
>    | 角扭转和 ≡ 0 (mod 3) | 可能非 0 | 扭转一个角 |
>    | 棱翻转和 ≡ 0 (mod 2) | 可能为奇 | 翻转一个棱 |
>    | 角排列奇偶 == 棱排列奇偶 | 可能不等 | 交换两个同类块 |
>
>    **同时坏掉两个**时(例如"角被扭转 + 棱被翻转"),`repair` 搜遍所有**单处**改动都不可能成功
>    —— 表现就是提示"没找到一步可修正的方案",用户无路可走。
>
>    所以加了 **`force_fix`(定向全修)**:枚举补齐方案 → 用 [`analyze`] 算出三个不变量的缺口
>    → **逐个定向修好**(扭转角 / 翻转棱 / 交换同类块,并自动为移动过的块选好朝向)
>    → 复验。实测能修好"双违规"甚至更多违规的局面。

### 「修正」按钮 —— 卡死时的出路

朝向/奇偶这类约束是**全局**的,只有快填满时才暴露 ——
例如"单个棱被翻转"时,最后两格无论涂什么都会被拒,看起来就是"填不进去"。

这时点 **「修正」**,程序依次尝试**一步最小改动**:

1. **翻转某个棱**(交换它的两个贴纸)→ 修"棱翻转数为奇数"
2. **扭转某个角**(三个贴纸轮换)→ 修"角扭转和不是 3 的倍数"
3. **交换两个同类块** → 修"角/棱排列奇偶性不一致"

找到后立刻应用并自动补全,状态行会说明改了什么:

```
已修正:已翻转棱位 1 的两个贴纸(原输入该处朝向不合法)
```

> 实测覆盖(见自检):单棱翻转 / 单角扭转 / 两块交换 / 卡死盘面(棱翻转+缺2格)/
> **卡死盘面(角扭转+缺2格)** / **端到端:卡死盘面自动修正并补全** / 只差 2 格自动补全 /
> 合法盘面不被改动。

### 橡皮擦(让"替换"仍然可行)

严格校验带来一个副作用:盘面填满时想改一个贴纸,直接涂会让某色变成 10 格 → 被拒。
所以调色板旁加了 **「擦除」** 按钮:选中后点格子即清除该贴纸,腾出空位后再涂就合法了。
擦除**不会**触发自动推导(否则会立刻被填回去),方便做两个贴纸的交换。

**自动推导能推出什么**(规则比直觉更强):

- ✅ **角位已知 2 色 + 它们的相对顺序 ⇒ 唯一确定整块**
  例:`U位=W、R位=R` ⇒ 只能是 `W-R-G`(URF);`W-B-R`(UBR) 的循环顺序是 `W→B→R`,不满足 `W→R`,被排除
- ✅ **块被占用 ⇒ 剩余位置确定**:还原态挖掉一格,其余 7 角 11 棱都已占用 ⇒ 剩下的块唯一
- ❌ **信息不足时不猜**:只给 1 色时有 4 个候选块,保持未填

> 完整约束传播等价于求解魔方(需要搜索),不适合每次点击都跑。这里的取舍是:
> **局部能确定的立刻确定 + 每次修改后给精确错误**。

### 自检

```bash
RUBIK_VALIDATE_TEST=1 cargo run --release
```
```
  ✅ 还原态                    ✅ 10 步打乱态
  ✅ 单角扭转(检出朝向错)      ✅ 单棱翻转(检出朝向错)
  ✅ 只交换两个棱(检出奇偶)    ✅ 中心块涂错色
  ✅ 有未填格子                ✅ 涂色时拒绝改中心
  ✅ 拒绝对面色同块            ✅ 挖空一格后自动推出
  ✅ 少一格 ⇒ 自动补回         ✅ 信息不足时不猜
  ✅ 2 色+顺序 ⇒ 推出          ✅ 满盘时允许替换贴纸
  ✅ 填色模式下拦截已满颜色     ✅ 解析/拒绝状态字符串
  不变量自检:17 通过 / 0 失败

== 旋转动画自检(轴角推导)==
  用例:4 个打乱态 × 18 种转动 = 72 组
  ✅ 全部轴对齐(单分量 ±1),角度均为 90/180/270
  旋转动画自检:72 通过 / 0 失败
```

### 输入回归测试(随机打乱 → 逐格输入)

回答一个问题:**拿一个真实的打乱魔方,按「自定义输入」逐格输入,能不能顺利输入完?**

```bash
cargo test -- --nocapture                   # 默认 80 次打乱 × 4 种输入顺序
RUBIK_INPUT_FUZZ=1000 cargo run --release   # 无图形环境跑 1000 次
RUBIK_INPUT_FUZZ=1000 RUBIK_INPUT_FUZZ_SEED=7 cargo run --release
```

每次的做法:① 随机打乱 25 步,生成一个**真·合法**盘面;② 用「自定义输入」的空盘面起手
(与 `Action::CustomInput` 一致 → `edit.clear()`),按 4 种顺序(行优先 / 逆序 / 随机 / 逐面)
逐格调用**真实的** `EditState::paint()`(内部走 `rederive → propagate → is_feasible`);
③ 判定四类"输入失败":

| 失败类型 | 含义(用户视角) |
|---|---|
| 取色器拒绝真实颜色 | 色块被置灰 / 点了没反应 |
| `paint()` 被拒 | 状态行出现「不能涂」 |
| 自动推导/补全填错 | 程序把颜色填成了别的(用户看不到,但后续输入会被连累) |
| 结束时盘面不符 | 输入完的盘面和自己的魔方不一样 |

失败时会打印**可直接复现**的 54 字符盘面(配合 `RUBIK_STATE=<字符串> cargo run`)。

> **这个测试抓出了一个真实 bug(2026-09,已修)**:`validate::propagate` 在角块/棱块
> **朝向尚未确定**(整块一个已知贴纸都没有)时就猜了一个朝向填色;错色占掉颜色配额后,
> 用户输入真实颜色会被「已放满 9 格」拒绝 —— 实测 **800 个会话里 187 个被污染、83 次
> 直接拒掉真实颜色**。加守卫后归零(见 `src/validate.rs` 中 propagate 的注释)。
> 同类问题还有"剩 ≤4 格时自动补全"会静默补成**另一个魔方**(约 23% 会话),
> 现在只有**合法解唯一**时才补全(`validate::find_unique_completion`)。

### ②③ 文本状态串格式

**54 个字符,面序 `U R F D L B`,每面 9 格按行优先。**
字母可用面名 `U/R/F/D/L/B`,也可用颜色名 `W/R/G/Y/O/B`;大小写与空白都会被忽略。

```bash
# 还原态
RUBIK_STATE=UUUUUUUUURRRRRRRRRFFFFFFFFFDDDDDDDDDLLLLLLLLLBBBBBBBBB cargo run --release
# 或
cargo run --release -- --state "FFUBUFURBLLRURDRBDFFDRFBBDUULFLDDRLBDRLRLUFBLBULFBDRUD"
```

文本输入与编辑器**走同一套校验** —— 非法状态串在载入时就被拒绝,并打印具体原因:

```
$ RUBIK_STATE=RUUUUUUUURRRRRRRRRFFFFFFFFFDDDDDDDDDLLLLLLLLLBBBBBBBBB cargo run --release
[state] 解析失败: 状态不合法: 有颜色超过 9 格:W:8 Y:9 R:10 O:9 B:9 G:9; 角位 3 的配色 ROB 不是任何合法角块
```

### 端到端验证(外部给定状态)

```bash
RUBIK_STATE=FFUBUFURBLLRURDRBDFFDRFBBDUULFLDDRLBDRLRLUFBLBULFBDRUD \
RUBIK_SELFTEST=/tmp/shot.png cargo run --release
```
```
[state]    已从命令行/环境变量载入初始状态
[selftest] 使用外部给定的初始状态(跳过内置打乱)
[selftest] ✅ 解法 19 步(尝试 1 次 / 4362 ms),开始播放…
[selftest] ✅ 全部 19 步播放完毕,终态已还原(动画驱动的是真实模型)
```

---

## 界面(2026-09 改版)

| 区域 | 内容 |
|---|---|
| **左上 HUD** | 魔方状态 / 求解器状态 / **播放状态 + 转动角度** / 解法列表(当前步 `[ ]`、已完成 `( )`) |
| **右上 编辑器** | 初始状态编辑(展开图 + 调色板 + 橡皮擦 + 应用/修正/读取/清空/还原) |
| **左下 按钮面板** | **所有操作都有按钮**(标签里带快捷键),不用记键盘 |
| **日志弹窗** | 默认隐藏,点「日志 (L)」弹出最近 16 条记录 |

### 转动角度会显示出来

播放时 HUD 会写清楚**这一转的目标角度**和**已经转了多少**:

```
播放中  L  目标转动 90°  已转 76°   进度 7/20
```

未播放时还会预告下一步:`下一步 R2(180°)`。角度按 90°/180°/270° 标注
(`270°` 会同时写成「反向 90°」,因为转 270° 顺时针 = 转 90° 逆时针)。

### 回退:**任何时刻都能用**

早期版本有个限制:`pb.is_busy()` 时直接忽略回退 ⇒ **只有全部播完才能回退**。
现在改成:

1. **若动画正在播** → 先把这一步**瞬间播完**(模型其实在动画开始时就已经推进过了,
   只需补上计数 + 把方块摆到终点),所以状态始终是明确的
2. 从历史里弹出最后一步,并**放回队列头** ⇒ 之后按「播放」就能**再往前播**,
   **回退 / 重做是对称的**,不会"退一步就再也前进不了"
3. 立刻播放它的**逆转动** —— 暂停中也会播(这是对按钮的直接响应)

> 实测(`RUBIK_UNDO_DEMO=1`,打乱后每 0.8 秒回退一次):
> ```
> [0.9s] 第 1 次回退 —— 此刻 history=2 queue=23 正在播=true
> [1.7s] 第 2 次回退 —— 此刻 history=3 queue=22 正在播=true
> [2.5s] 第 3 次回退 —— 此刻 history=4 queue=21 正在播=true
> ```
> 三次都在 `正在播=true` 时成功,且 queue 逐步减少(步骤被放回队列,可重做)。

### 求解后**不会自动播放**

求解完成只是把解法排进队列并保持**暂停**,由你决定:
点「播放/暂停」开始,或点「单步」一步步看。

### 选一个颜色 → 所有能填的格子**闪烁绿框**

在调色板里选中某色后,程序会对 54 格逐格试探("涂上去还可不可行"),
把**能合法放这个颜色**的格子用**亮薄荷绿呼吸闪烁**标出来。

- 绿色本身是贴纸色之一,所以靠"**动起来**"和静态贴纸区分(和品红冲突框同思路)
- 编辑器状态行显示 `可填 N 格`;中心块永远不可填(所以最多 48 格)
- 切换颜色 / 橡皮擦 / 每次涂色后都会重算

### 日志走弹窗

所有提示与错误(涂色被拒的原因、修正做了什么、求解进度…)**不再铺在画面上**,
而是写进日志缓冲;点「日志」按钮或按 `L` 弹出查看,再点一次关闭。

## 操作

| 键 | 作用 |
|---|---|
| `空格` | 暂停 / 继续 |
| `→` 或 `N` | 单步前进(暂停状态下也可用,播完自动保持暂停) |
| `←` 或 `P` | 回退一步 |
| `S` | 求解当前状态 |
| `X` | 随机打乱 25 步 |
| `R` | 重置为还原态 |
| `E` | 显示 / 隐藏状态编辑器 |
| `A` / `D` | 环绕旋转视角 |
| `[` / `]` | 调慢 / 调快动画 |
| `Esc` | 退出 |

---

## 架构

```
src/
├── main.rs    App 装配、资源、键盘输入、相机
├── cube.rs    模型 → 实体:27 个小方块 + 外露贴纸
├── anim.rs    转动动画:轴角推导、缓动、暂停/单步
├── solve.rs   后台求解线程 + ≤20 步的两级保证
└── ui.rs      左上角 HUD
```

### 数据流(单向,逻辑永远是权威)

```
       键盘输入
          │
          ▼
   ┌─────────────┐   唯一真值
   │  CubeCore   │◄──────────────┐
   │ (cubr-core) │               │
   └──────┬──────┘               │ 转动结束:整体重算
          │ 状态投影              │
          ▼                      │
   ┌─────────────┐  每帧插值  ┌──┴──────────┐
   │ 27 个实体    │◄──────────│ 动画(轴角)  │
   └─────────────┘            └─────────────┘
```

**关键设计**:动画不自己维护"转了多少度"的账本,而是

1. 转动开始前,记录受影响方块的起始变换 `(t0, r0)`;
2. 从**模型本身**推导这次转动的轴与角 —— 取该方块转动前/后的朝向矩阵 `R0`、`R1`,`Δ = R1·R0ᵀ` 的轴角即答案(**不依赖任何手性约定**,不会出现"转反了");
3. 每帧用 `Δ(t) = Quat::from_axis_angle(axis, angle·ease(t))` 插值;
4. 转动结束时,从 `CubeCore` **整体重算**所有实体变换 —— 浮点误差不会累积,视觉与逻辑永不失配。

### ≤20 步的两级保证(`solve.rs`)

| 级别 | 做法 | 实测结果 |
|---|---|---|
| **1** | `cubr-core` 混合求解器:Korf 最优 IDA*(默认 4 秒预算)→ 超时回退近优两阶段 | 随机打乱(25/40 步)一律 **20 步 / 约 4.3 秒** |
| **2** | 若第 1 级返回 >20 步:把 Korf 预算提到 1 小时,跑**完整最优搜索** | 由 God's Number = 20 保证必然 ≤20 |

**为什么需要第 2 级**:实测发现极少数"距上帝之数最近"的状态(如经典的 **superflip**):

| 配置 | superflip 结果 |
|---|---|
| 默认 4 秒预算 | 22 步 |
| 25 秒预算,重试 17 次 | 最好 **21 步**(两阶段回退**永远到不了 20**) |
| 完整最优搜索 | **20 步**(耗时可达分钟级) |

所以第 2 级是必要的 —— 它慢,但它是"任意状态 ≤20 步"这个承诺的唯一兑现方式。
第 2 级运行期间 HUD 会显示进度,并且随时可以用 `X` / `R` 取消。

---

## 依赖

```toml
bevy = "0.19"
cubr-core = "0.2.0"   # 纯 Rust 3×3 模型 + Korf 最优求解器(含两阶段回退)
```

`cubr-core` 提供:
- `core::CubeCore` —— 整数网格模型,`cubies()` 给出每个方块的 `pos`/`orient`/`stickers`,`layer(move)` 给出某次转动影响哪些方块
- `model::{CubeState, Move, StickerColor, Face, Turn}`
- `solver::{build_or_load_pdbs, Solver, SolveError}`

### 关于求解预算

`cubr-core` 的 Korf 预算可通过环境变量覆盖:

```bash
CUBR_KORF_BUDGET_MS=600000 cargo run --release   # 10 分钟预算
```

程序内部会在需要时用 `std::env::set_var` 调整它(见 `solve.rs::set_korf_budget`)。

---

## 旋转动画的一个真实 bug(已修 + 已加回归测试)

**现象**:转动时把方块"翻出内部",看成斜着甩出去。

**根因**:`cubr-core` 的 `paint()` **不是"重建置换/朝向",而是"把还原态的块原地重新上色"** ——
它会把每个方块的 `pos` 归位、`orient` 重置为单位矩阵:

```rust
pub fn paint(&mut self, state: &CubeState) {
    for cubie in &mut self.cubies {
        cubie.pos    = cubie.home;    // 归位
        cubie.orient = identity();    // ⚠️ 朝向清零
        for (n, color) in &mut cubie.stickers { *color = state.face(f)[idx]; }
    }
}
```

早期实现拿 `paint()` 出来的副本当"转动之后"的状态,于是推出的增量不是"这一转",而是
`R_move · R_真实⁻¹` —— 可能是 **体对角线轴**(如 `(0.577, −0.577, −0.577)`)转 240°,
方块自然被甩出立方体。

**修法**:轴角推导改成**在同一份真实模型上取"转动前 / 转动后"**
(`anim::derive_axis_angle`),并加了一层防御:结果若不是轴对齐就退回"面法线 + 90°×n"。

**为什么原来的自检抓不到**:每次转动**结束**时都会 `sync_transforms()` 从模型整体重算,
所以**最终画面永远正确**,只有中间帧是错的。
现在补了两道回归防线:

1. **静态自检**(`RUBIK_VALIDATE_TEST=1`):4 个打乱态 × 18 种转动 = **72 组**,
   断言轴必须单分量 ±1、角度必须是 90/180/270 → **72/72 通过**
2. **运行时自检**(`RUBIK_DEBUG=1`):每次转动开始时比较"插值终点"与"模型推进后的位置",
   不一致就 `warn!` 打日志

---

## 中文界面与字体

界面文字是中文,字体**已内嵌进二进制**(`include_bytes!`),所以:

- ✅ 单独拷一个 exe 到任何地方都能显示中文(不需要 `assets/` 目录)
- ✅ 字体是 **Noto Sans CJK SC 子集**(SIL OFL 许可,可自由分发),**230 KB**
- ✅ 只包含源码里实际用到的 745 个字形

> ⚠️ **改了界面文案后必须重新生成字体子集** ——
> 否则新出现的汉字不在子集里,会显示成方块或缺字。
> (本项目踩过一次:加「擦除」按钮后忘了重建,按钮只显示了「除」)

```bash
# 1) 收集源码中出现过的所有字符
python3 - <<'EOF'
import glob
chars = set()
for f in glob.glob("src/*.rs"):
    chars |= set(open(f, encoding="utf-8").read())
chars |= {chr(c) for c in range(0x20, 0x7F)}
open("/tmp/subset_chars.txt", "w", encoding="utf-8").write("".join(sorted(chars)))
EOF

# 2) 子集化(index 2 = Noto Sans CJK SC,简体字形)
pyftsubset /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
  --font-number=2 --text-file=/tmp/subset_chars.txt \
  --output-file=assets/fonts/ui-cjk.otf --no-hinting --desubroutinize

# 3) 【必做】逐字验证覆盖 —— 别等跑起来才发现缺字
python3 - <<'EOF'
from fontTools.ttLib import TTFont
cmap = TTFont("assets/fonts/ui-cjk.otf").getBestCmap()
need = set(open("/tmp/subset_chars.txt", encoding="utf-8").read())
miss = sorted(c for c in need if ord(c) not in cmap and ord(c) > 0x1F)
print(f"字形 {len(cmap)},缺失 {len(miss)}:", "".join(miss)[:80] or "(无)")
# 已知可忽略:上标符号/减号(只在代码注释)、勾叉与相机 emoji(只在终端 println!)
EOF
```

### 中文必须用「逐字断行」

Bevy 默认 `LineBreak::WordBoundary`(按词断行)—— 对中文既没必要(中文没空格),
又会去查 CJK 词典。所以本项目所有中文文本都设成:

```rust
TextLayout { linebreak: LineBreak::AnyCharacter, ..default() }
```

> ⚠️ **窗口标题栏**由系统合成器绘制,用的是**系统字体**,我们无法替换。
> 所以标题按平台自适应:Windows 用中文,WSLg / Linux 等环境退回 ASCII。

### 关于 `ICU4X data error: No segmentation model for complex script: Chinese/Japanese`

> ⚠️ **更正**:我最初判断这是"缺少 LSTM 数据,开个 feature 就好"—— **那个判断是错的**。
> 下面是实测结论(`cargo run --release --example seg_probe` 可复现各项行为)。

**警告出处**:`icu_segmenter` 的 `ComplexPayloadsBorrowed::select(ComplexScript::ChineseOrJapanese)`:

```rust
ComplexScript::ChineseOrJapanese => self.ja.or_else(|| {
    ERR.with_display_context("Chinese/Japanese");   // ← log::warn!(...)
    None
}),
```

**三条实测结论**:

| 事实 | 说明 |
|---|---|
| ① 只是 `log::warn!` | 不崩溃;ICU4X 退化成"整段不分词" |
| ② **行断行不需要词典** | `LineSegmenter::new_for_non_complex_scripts` 对中文**逐字断行完全正确**,不报错 |
| ③ 只出现在**新版(Neo)分词 API** | 该 API 才需要 CJK 词典;parley 0.9 用的是 **V1 API**,复杂文字载荷**硬编码为空** —— 所以**开不开 `lstm`/`auto` 都不影响这条警告** |

**真正的规避方式**(也是中文排版本来就该用的):给中文文本设**逐字断行**,
不走"按词断行"那条查词典的路径:

```rust
TextLayout { linebreak: LineBreak::AnyCharacter, ..default() }
```

另外在 `LogPlugin` 里加了兜底过滤,即使别的路径触发也不刷屏:

```rust
DefaultPlugins.set(LogPlugin {
    filter: "wgpu=error,naga=warn,icu_provider=off,icu_segmenter=off".into(),
    ..default()
})
```

本项目实测:界面含长中文(含窄栏里的长报错文本)时,该警告 **0 条**。

> 📌 **实测补充(2026-09)**:用户侧反馈 **debug 构建下偶发、release 构建下不再出现**。
> 最可能的原因是:那个 debug 产物是**加 `LineBreak::AnyCharacter` 之前**构建的
> (换行仍走"按词断行"路径),而 release 是修复后重新构建的。
> 结论:该警告只是 `log::warn!`,不影响渲染正确性;**用 release 构建即可忽略**。
> 排查工具保留在 `examples/seg_probe.rs`。

## 渲染层的一个真实 bug(已修):Apply 之后 3D 不同步

**现象**:编辑器里改好状态 → 点「应用」→ **画面上的魔方却像被重置了**(还是旧配色);
但求解正常,动画播完结果是乱的;此时点「从魔方读取」读出来是**还原态**。

**根因**:`CubeCore::paint()` 是**原地改贴纸颜色**(方块位置/朝向不变,实体也不重建),
而渲染层的贴纸材质是**生成实体时烘死的** —— 之后没人去更新它:

```rust
// spawn 时一次性选好材质,之后再也不变
parent.spawn((Mesh3d(sticker_mesh), MeshMaterial3d(handle_of(*color)), ...));
```

于是:
- **模型**已经变成目标状态 → 求解正常 ✓
- **画面**还是旧配色 → 看起来"被重置了" ✗
- 求解动画转的是"位置/朝向",颜色对不上 → 过程看起来很乱 ✗
- 播完 `sync_transforms` 把位置摆回还原态(模型此时已解)→ 再「从魔方读取」就是还原态 ✗

**修法**:贴纸实体记住自己对应「哪个方块的第几个贴纸槽」,每帧把材质同步成模型里的当前颜色:

```rust
#[derive(Component)]
pub struct StickerOf { pub cubie: usize, pub slot: usize }

pub fn sync_sticker_materials(model: Res<Model>, mats: Res<StickerMaterials>,
                              mut q: Query<(&StickerOf, &mut MeshMaterial3d<StandardMaterial>)>) { ... }
```

> 关键区分:**转动**(`apply`)时贴纸是方块的子实体,会跟着一起转,不需要同步 ✓;
> 只有 **`paint`**(Apply / 载入外部状态 / 重置)才是"原地改色",必须同步材质 ✓。

**验证方式**:加了调试通道 `RUBIK_APPLY_DEMO=1`(走 Apply 的真实代码路径)+
`RUBIK_SHOT_AT=<秒> RUBIK_SHOT_PATH=<路径>`(定时截图),截图确认 Apply 后 3D 立即显示打乱态。

---

## 为什么"不变量都对"还会出 bug —— 三层结构

不变量(中心/数量/块/朝向/奇偶)是**充要条件**,但它们只回答一个问题:
**「这个 54 格状态是不是一个合法可达的魔方?」** —— 它们**管不到**另外两层:

| 层 | 负责什么 | 谁来保证 | 失效时的表现 |
|---|---|---|---|
| ① **模型 / 校验** | 状态合法、可解 | 5 条不变量 + 可行性闸门 | 拒绝涂色 / 拒绝应用 |
| ② **动画** | 转动轴角正确 | 轴角推导 + 72 组自检 | 转动方向错、方块翻出内部 |
| ③ **渲染同步** | 画面 == 模型 | `sync_transforms` / `sync_sticker_materials` | **画面停在旧状态,而模型已经变了** |

**Apply 那个 bug 全在第 ③ 层**:模型一直是对的(所以"求解正常"),只是贴纸材质没跟着
`paint()` 更新 —— 不变量再多也照不到它 ✗

### 为什么自检没抓到

早期所有自检都是**模型级**断言(比较 `CubeCore` 的状态),而渲染层没有任何断言 ✗
而且当时的截图场景测的是 `scramble → solve` 链路(走 `apply`,贴纸跟着方块转,**不需要同步**),
恰好绕开了出问题的 `paint` 路径 ✗

现在补上:
- **每帧同步**:`sync_sticker_materials` 把材质刷成模型当前颜色 —— 任何"改色"路径都自动覆盖
- **失配告警**:`RUBIK_DEBUG=1` 时,若某帧需要修正材质,就 `warn!` 报出来
  (实测 Apply 那帧报 `修正了 41 个贴纸材质`,与现象完全吻合)

> 教训:**"不变量"只能保证它描述的那一层**。跨层的 bug 需要跨层的断言
> (这已经是本项目第三次同类教训:模型对 → 动画错、动画对 → 渲染错)。

## 构建与自检命令(照抄即可)

```bash
cargo build --release --all-targets   # ⚠️ 带上 --all-targets,否则 example 不会被编译
cargo run  --release                  # 运行
RUBIK_VALIDATE_TEST=1 cargo run --release   # 不变量 + 动画自检(36 + 72 项)
RUBIK_SELFTEST=/tmp/shot.png cargo run --release   # 端到端 + 截图
cargo run --release --example seg_probe            # ICU4X 分词探针
```

> ⚠️ **踩过的坑**:平时只用 `cargo build --release`(默认只编 bin),
> example 出了问题不会暴露 —— 但 IDE 的 `cargo check --all-targets` 会直接报错。
> 本项目就发生过一次:`examples/seg_probe.rs` 里留了个需要 `unstable` feature 的探针,
> 主程序一直好好的,example 却编译不过。**改完代码记得带上 `--all-targets` 验证一次。**

## 🌐 WASM 版(手机上玩)

```bash
./build-wasm.sh                    # 编译 wasm + 生成 JS 胶水 → web/
python3 -m http.server 8080 -d web # 本机预览
# 手机(同一局域网)访问 http://<电脑IP>:8080
```

| 项目 | 数值 |
|---|---|
| wasm 原始体积 | **40 MB**(`strip = true` 之后;之前 67 MB) |
| **gzip 后实际下载** | **9.3 MB** ✅ 手机上可接受 |
| 首次打开 | 下载 9.3MB + wasm 编译,约十几秒;之后浏览器缓存 |

### 为了能在手机上跑,做了三处适配

| 问题 | 处理 |
|---|---|
| **`cubr-core` 用了 4 处 `std::thread::scope`** —— wasm 上没有线程,调用即 panic ✗ | **vendor 一份并打补丁**(`vendor/cubr-core`):wasm 上跳过 Korf 与看门狗线程 |
| **Korf 需要模式数据库**:浏览器无文件缓存 ⇒ 每次打开都要重算(桌面 15s+、占 ~62MB)✗ | wasm 上**只用两阶段算法**(无需 PDB,毫秒级) |
| 手机 GPU 跑阴影很贵 | wasm 上**默认关闭阴影映射** |

### wasm 上的时间 API(`time not implemented on this platform`)

**现象**:浏览器里一打开就 panic

```
panicked at library/std/src/sys/time/unsupported.rs:13:9:
time not implemented on this platform
```

**原因**:**wasm32-unknown-unknown 的 std 里 `Instant::now()` / `SystemTime::now()` 是
`unimplemented!()`** —— 标准库没有时钟,直接 panic ✗

**修法**:用 [`web-time`](https://crates.io/crates/web-time) —— 浏览器里走 JS 的
`performance.now()`,原生平台直接转发给 std,两边行为一致,所以可以**无条件替换**:

```rust
use web_time::Instant;   // ← 原来是 std::time::Instant
```

**改了这些地方**(solver 内部也要改,不然求解时照样炸):

| 位置 | 用途 |
|---|---|
| `src/ui.rs` | 日志时间戳(**启动时**就取 ⇒ 一打开就 panic)|
| `src/solve.rs` | 求解耗时统计 |
| `vendor/cubr-core/src/solver/two_phase.rs` | **两阶段搜索的截止时间判断** ✗ |
| `vendor/cubr-core/src/solver/mod.rs` / `cache.rs` | 看门狗 / PDB 缓存时间戳 |

**顺带做了一次 wasm 不安全 API 审计**:

| API | 结论 |
|---|---|
| `std::env::var` | wasm 上返回 `Err` ✓ 安全(所有调试钩子自动失效,正合预期)|
| `std::thread::*` | 只在 Korf 路径 ✗ wasm 不走到 ✓;`SearchTables::build()` **0 处线程调用** ✓ |
| `std::fs` | 只在 PDB 缓存 ✗ wasm 不走到 ✓ |
| 截图 `save_to_disk` | 没文件系统 ✗ → wasm 上改为日志提示 ✓ |

### 移动端两个实际踩到的问题

**① 「求解」按钮点了没反应**(只有 wasm 版)

根因:`is_ready()` 是 `!matches!(status, Booting)` —— 而"就绪"这个状态是
**工作线程**发出来的;wasm 上没有线程 ⇒ 状态永远停在 `Booting` ⇒
`is_ready()` 恒为 false ⇒ 点「求解」只在日志里写一句"求解器还在初始化",什么也不做 ✗

修:wasm 版 `start_worker` 里**初始就置为 `SolveStatus::Ready`**,求解器改为
第一次 `request()` 时懒加载(构建两阶段表,约 1 秒)。

**② 画面不是全屏、也不居中**

根因:`Window { resolution: (1280, 720) }` 在 web 上会被 Bevy 写成 canvas 的
**内联尺寸**,覆盖掉 CSS 的 `100vw/100vh` ✗

修:
```rust
#[cfg(target_arch = "wasm32")]
fit_canvas_to_parent: true,   // canvas 跟随父容器(body)⇒ 真正全屏
```
另外加了 `fit_camera_distance`:**按窗口宽高比自动拉远相机**
(竖屏手机 FOV 是竖直方向的,魔方会显得又小又靠边),
距离按 `1/aspect^0.55` 缩放并夹在 1.0~2.1 倍,同时抬高视点保持俯视角 ✓

### ⚠️ WASM 版与桌面版的两个差异

1. **解法长度**:桌面版走 Korf 最优(保证 ≤20 步);**WASM 版是两阶段算法,通常 20~23 步**
   (牺牲最优性换取"无需 62MB 数据库 + 无需线程")。界面上仍会显示实际步数。
2. **首次求解会卡一下**:两阶段表在浏览器里现建(约 1 秒),之后就一直很快。

### 🔁 以后怎么更新(一条命令,不会漏哈希)

```bash
./deploy.sh          # = 构建(自动哈希+校验)→ 上传 → 线上验收
```

**为什么必须脚本化**:`rubik_bevy.js`(JS 胶水)与 `.wasm` 是**版本强耦合**的
(函数索引表必须一一对应)。只哈希其中一个,浏览器就会出现

```
TypeError: wasm.__wasm_bindgen_func_elem_xxxxx is not a function
```

(本项目真踩过:第一次只给 wasm 加了哈希,JS 仍叫 `rubik_bevy.js` 且被
`immutable` 缓存一年 ⇒ 用户拿到"旧 JS + 新 wasm" ⇒ 满屏这个错误 ✗)

所以 `build-wasm.sh` 现在会:

1. 编译 + wasm-bindgen
2. **算一次 sha256,同时重命名 wasm 和 JS**(同一个哈希)
3. 从 `index.html.template` 生成 index.html,注入该哈希
4. **一致性校验**:`index.html` 的引用名 == 实际文件名,且 JS 内部引用的 wasm 同哈希
   —— 不通过就**直接失败,不允许部署** ✓

`deploy.sh` 在此之上再:上传 → 清理早期无哈希文件(保留最近 3 个历史哈希) → **线上验收**
(检查 `index.html` 是 `no-store`、两个资源是 `immutable`+`gzip`、且哈希一致)✓

脚本也放了一份在服务器 `/root/rubik/`(web 根目录之外,不会被公开下载)✓

### 🚀 已部署(2026-09)

**https://static.guildos.ai/games/rubik/** ← 手机浏览器直接打开

部署位置(agent0,nginx 1.24):

```
/var/www/html/games/rubik/index.html                          2 KB    ← 不缓存
/var/www/html/games/rubik/wasm/rubik_bevy.js                106 KB    ← 长期缓存
/var/www/html/games/rubik/wasm/rubik_bevy_bg.086b463f437e.wasm 41 MB  ← 长期缓存
```

**缓存策略**(这是"以后更新不会拿到旧版本"的关键):

| 文件 | Cache-Control | 原因 |
|---|---|---|
| `index.html` | `no-store, no-cache, must-revalidate` | 必须每次拿最新,才知道新的 wasm 文件名 |
| `wasm/rubik_bevy_bg.<sha>.wasm` | `public, max-age=31536000, immutable` | 文件名含**内容哈希**,内容变了名字就变 |
| `wasm/rubik_bevy.js` | 同上 | 同上 |

⇒ **更新流程**:`./build-wasm.sh`(自动算哈希、改名、注入 index.html)→ 重新上传 ⇒
老用户的 `index.html` 不会被缓存,立刻拿到新的 wasm 文件名并下载新文件 ✓
旧的 wasm 留在服务器上也无妨(还在用的老页面仍能加载)✓

nginx 侧加的规则(`/etc/nginx/sites-available/static.guildos.ai`,已备份):

```nginx
gzip_types text/plain text/css application/json application/javascript application/wasm;

location = /games/rubik/index.html { add_header Cache-Control "no-store, no-cache, must-revalidate" always; ... }
location ~ ^/games/rubik/wasm/.*\.(wasm|js)$ {
    types { application/wasm wasm; application/javascript js; }
    add_header Cache-Control "public, max-age=31536000, immutable" always; ...
}
```

**实测**:
```
index.html            → cache-control: no-store ✓
wasm(带哈希)          → cache-control: public, max-age=31536000, immutable ✓
                        content-encoding: gzip ✓  application/wasm ✓
下载体积              → 41.7 MB → 11.1 MB(gzip)✓
```

### 部署提示

- **必须通过 http(s) 打开**(`file://` 加载不了 wasm)
- **服务器要对 `.wasm` 开 gzip/brotli** —— 这一步决定了是 40MB 还是 9.3MB
  (Nginx: `gzip_types application/wasm;`;多数静态托管默认已开)
- 手机端 UI 会**按屏宽自动缩放**(`fit_ui_scale`:1280px 宽 = 1.0,下限 0.42)
- 所有操作都是按钮,触摸可直接用 ✓

## 已知限制

1. **superflip 类状态耗时长**:这是最优搜索的本质难度,不是 bug。日常使用(随机打乱)不会遇到。
2. **首次运行 15 秒建库**:Korf 模式数据库,建完会缓存到磁盘。
3. **非法状态会被拒绝**:例如单角翻转、单棱翻转、奇偶性错误这类"拆了重装"的状态,`cubr-core` 会返回 `Unsolvable`。
4. **只支持 3×3×3**:4×4 及以上不在 `cubr-core` 范围内。