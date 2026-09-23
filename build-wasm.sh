#!/usr/bin/env bash
# 一键构建 WASM 版(输出到 web/)
#   产物:web/index.html(不缓存)+ web/wasm/rubik_bevy.js + web/wasm/rubik_bevy_bg.<sha>.wasm(长期缓存)
set -euo pipefail
cd "$(dirname "$0")"

echo "[1/4] 编译 wasm32-unknown-unknown …"
cargo build --release --target wasm32-unknown-unknown

echo "[2/4] 生成 JS 胶水(wasm-bindgen)…"
rm -rf web/wasm && mkdir -p web/wasm
wasm-bindgen --target web --no-typescript \
  --out-dir web/wasm --out-name rubik_bevy \
  target/wasm32-unknown-unknown/release/rubik-bevy.wasm

echo "[3/4] 给 wasm **和 JS 胶水**都加同一个内容哈希…"
# 注意:两者版本强耦合,必须同时换名 —— 只改 wasm 会出现"旧 JS + 新 wasm",
# 报 `__wasm_bindgen_func_elem_* is not a function`。
HASH=$(sha256sum web/wasm/rubik_bevy_bg.wasm | cut -c1-12)
mv web/wasm/rubik_bevy_bg.wasm "web/wasm/rubik_bevy_bg.${HASH}.wasm"
mv web/wasm/rubik_bevy.js     "web/wasm/rubik_bevy.${HASH}.js"
# 胶水内部的缺省 wasm URL 也改成哈希名(未显式传 module_or_path 时才会用到)
sed -i "s/rubik_bevy_bg\.wasm/rubik_bevy_bg.${HASH}.wasm/g" "web/wasm/rubik_bevy.${HASH}.js"
# index.html 由模板生成,注入同一个哈希
sed "s/__WASM_HASH__/${HASH}/g" index.html.template > web/index.html

echo "[4/5] 一致性校验(引用名 == 实际文件,JS 内部引用同哈希)…"
python3 - <<'PYCHECK'
import os, re, sys
html = open("web/index.html").read()
refs = set(re.findall(r"rubik_bevy[a-z_]*\.[a-z0-9]+\.(?:js|wasm)", html))
files = set(os.listdir("web/wasm"))
ok = True
if refs - files:
    print("  ✗ index.html 引用了不存在的文件:", refs - files); ok = False
hashes = {r.split(".")[-2] for r in refs}
if len(hashes) != 1:
    print("  ✗ index.html 里的哈希不一致:", hashes); ok = False
h = hashes.pop() if hashes else None
if h:
    js = open(f"web/wasm/rubik_bevy.{h}.js").read()
    inner = set(re.findall(r"rubik_bevy_bg\.[a-z0-9]+\.wasm", js))
    if inner != {f"rubik_bevy_bg.{h}.wasm"}:
        print("  ✗ JS 内部引用的 wasm 与哈希不符:", inner); ok = False
if not ok:
    sys.exit("  校验失败 —— 不要部署!")
print(f"  ✓ 全部一致(哈希 {h})")
PYCHECK

echo "[5/5] 完成:"
ls -lh web/index.html web/wasm/*.wasm web/wasm/*.js | awk '{print "   ", $9, $5}'
echo "    wasm 哈希: ${HASH}"
cat <<'TIP'

部署要点:
  - index.html 必须【不缓存】,wasm 可以【长期缓存】(文件名带哈希,更新即换名)
  - 服务器要对 .wasm 开 gzip/brotli(40MB → 约 9MB)
  - 本机预览:python3 -m http.server 8080 -d web
TIP
