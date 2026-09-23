//! ICU4X 分词探针 —— 用于定位 "No segmentation model for complex script" 警告。
//!
//! 运行:`cargo run --release --example seg_probe`
//!
//! 结论(2026-09 实测):
//! - `LineSegmenter::new_for_non_complex_scripts`(parley 0.9 用的 V1 API)
//!   对中文**逐字断行是正确的**,而且**不报错**(复杂文字载荷为空也不影响)。
//! - 该警告只可能由**新版(Neo)分词 API** 触发 —— 它需要 CJK 词典/LSTM 数据。
//! - 因此真正的规避方式是:给中文文本设 `TextLayout { linebreak: LineBreak::AnyCharacter }`
//!   (逐字断行,不走"按词断行"的词典路径),见 `src/ui.rs` / `src/editor.rs`。
use icu_segmenter::options::{LineBreakOptions, LineBreakWordOption, WordBreakInvariantOptions};
use icu_segmenter::{LineSegmenter, WordSegmenter};

fn main() {
    // 注:新版(Neo)分词 API 需要 icu_segmenter 的 `unstable` feature,本项目没开,
    // 所以这里只测 V1 / auto 两条路径 —— 它们正是 Bevy(parley 0.9)实际走的。
    let text = "这是一段中文文本用来测试分词与断行的行为";
    println!("文本: {text}");

    println!("\n[1] LineSegmenter::new_for_non_complex_scripts  ← parley 用的就是它");
    let ls = LineSegmenter::new_for_non_complex_scripts(LineBreakOptions::default());
    let v: Vec<usize> = ls.segment_str(text).collect();
    println!("    断行位置: {v:?}");

    println!("\n[2] LineSegmenter::new_auto(带词典/LSTM)");
    let ls2 = LineSegmenter::new_auto(LineBreakOptions::default());
    let v2: Vec<usize> = ls2.segment_str(text).collect();
    println!("    断行位置: {v2:?}");

    println!("\n[3] WordSegmenter::new_for_non_complex_scripts");
    let ws = WordSegmenter::new_for_non_complex_scripts(WordBreakInvariantOptions::default());
    let v3: Vec<usize> = ws.segment_str(text).collect();
    println!("    分词位置: {v3:?}");

    println!("\n[5] LineSegmenter::new_for_non_complex_scripts + word_option = Normal");
    println!("    ← parley 在 WordBreak::Normal(bevy 默认换行模式)时就是这样调的");
    let mut opt = LineBreakOptions::default();
    opt.word_option = Some(LineBreakWordOption::Normal);
    let ls5 = LineSegmenter::new_for_non_complex_scripts(opt);
    let v5: Vec<usize> = ls5.segment_str(text).collect();
    println!("    断行位置: {v5:?}");

    println!("\n[6] LineSegmenter::new_auto + word_option = Normal");
    let ls6 = LineSegmenter::new_auto(opt);
    let v6: Vec<usize> = ls6.segment_str(text).collect();
    println!("    断行位置: {v6:?}");

    println!("\n[4] WordSegmenter::new_auto");
    let ws2 = WordSegmenter::new_auto(WordBreakInvariantOptions::default());
    let v4: Vec<usize> = ws2.segment_str(text).collect();
    println!("    分词位置: {v4:?}");
}
