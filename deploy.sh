#!/usr/bin/env bash
# 一键:构建(含哈希)+ 校验 + 部署到 agent0
#
#   ./deploy.sh              # 构建并部署
#
# 为什么要有这个脚本:
#   JS 胶水与 wasm 是【版本强耦合】的(函数索引表必须对应)。只哈希其中一个,
#   就会出现"旧 JS + 新 wasm" ⇒ 浏览器报:
#       TypeError: wasm.__wasm_bindgen_func_elem_xxxxx is not a function
#   所以哈希、改名、生成 index.html、校验一致性、上传 —— 必须是一件事,不能靠记性。
set -euo pipefail
cd "$(dirname "$0")"

REMOTE_HOST="${REMOTE_HOST:-agent0}"
REMOTE_USER="${REMOTE_USER:-root}"
REMOTE_DIR="${REMOTE_DIR:-/var/www/html/games/rubik}"
SSH_KEY="${SSH_KEY:-$HOME/.ssh/agent0_id_ed25519}"
URL="${URL:-https://static.guildos.ai/games/rubik/}"

echo "══ 1/3 构建(自动哈希 + 校验)══"
./build-wasm.sh

echo
echo "══ 2/3 上传到 ${REMOTE_USER}@${REMOTE_HOST}:${REMOTE_DIR} ══"
SSHOPT=(-i "$SSH_KEY" -o BatchMode=yes -o Compression=yes)
rsync -az -e "ssh ${SSHOPT[*]}" web/index.html "${REMOTE_USER}@${REMOTE_HOST}:${REMOTE_DIR}/"
rsync -az -e "ssh ${SSHOPT[*]}" web/wasm/      "${REMOTE_USER}@${REMOTE_HOST}:${REMOTE_DIR}/wasm/"

# 预压缩:生成 .gz 供 nginx 的 gzip_static 直接发送(零 CPU 开销,比实时压缩更小)
ssh "${SSHOPT[@]}" "${REMOTE_USER}@${REMOTE_HOST}" bash -s <<'REMOTE_GZ'
set -e
cd /var/www/html/games/rubik/wasm
for f in *.wasm *.js; do gzip -9 -c "$f" > "$f.gz"; done
REMOTE_GZ

# 清理:删掉不再被引用的散落文件(保留少量历史哈希,方便还开着旧页面的用户)
ssh "${SSHOPT[@]}" "${REMOTE_USER}@${REMOTE_HOST}" bash -s <<REMOTE
set -e
cd "${REMOTE_DIR}/wasm"
rm -f rubik_bevy.js rubik_bevy_bg.wasm          # 早期没哈希的名字
# 只保留最近 3 个 wasm / 3 个 js
ls -t rubik_bevy_bg.*.wasm 2>/dev/null | tail -n +4 | xargs -r rm -f
ls -t rubik_bevy.*.js      2>/dev/null | tail -n +4 | xargs -r rm -f
echo "  远端文件:"; ls -la | awk 'NR>3 {printf "    %s\t%s\n", \$5, \$9}'
REMOTE

echo
echo "══ 3/3 线上验收 ══"
python3 - "$URL" <<'PYVERIFY'
import re, sys, urllib.request
url = sys.argv[1]
UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124 Safari/537.36"
def get(u, head=False):
    # 注意:Cloudflare 会 403 掉 Python-urllib 的默认 UA,所以伪装成浏览器
    h = {"User-Agent": UA}
    if head:
        h["Accept-Encoding"] = "gzip"      # HEAD 只为看 gzip 头
    req = urllib.request.Request(u, method="HEAD" if head else "GET", headers=h)
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.status, dict(r.headers), (b"" if head else r.read())
st, hd, body = get(url)
html = body.decode("utf-8", "replace")
refs = sorted(set(re.findall(r"rubik_bevy[a-z_]*\.[a-z0-9]+\.(?:js|wasm)", html)))
print(f"  index.html  HTTP {st}  cache-control={hd.get('Cache-Control')}")
assert "no-store" in (hd.get("Cache-Control") or ""), "index.html 必须 no-store!"
for ref in refs:
    st2, hd2, _ = get(url + "wasm/" + ref, head=True)
    print(f"  {ref}  HTTP {st2}  {hd2.get('Cache-Control')}  gzip={hd2.get('Content-Encoding')}")
    assert st2 == 200 and "immutable" in (hd2.get("Cache-Control") or "")
h = {r.split(".")[-2] for r in refs}
assert len(h) == 1 and len(refs) == 2, f"哈希不一致或引用数量不对: {refs}"
print(f"  ✓ 线上一致,哈希 {h.pop()}")
PYVERIFY

echo
echo "✅ 部署完成 → ${URL}"
echo "   ⚠️ 手机/浏览器请强制刷新一次(旧的 JS 可能还在浏览器缓存里;"
echo "      index.html 本身不缓存,所以只要重新打开页面即可拿到新哈希)"
