#!/usr/bin/env bash
# ============================================================
#  竞赛对拍机 - Linux / macOS 打包脚本（生成无控制台的单文件可执行文件）
#  用法：chmod +x build.sh && ./build.sh
#  输出：dist/对拍机 （Linux ELF）或 dist/对拍机.app（macOS）
#  说明：源码方式直接用 python3 duipai.py 运行即可，无需打包。
# ============================================================
set -e

PYTHON="${PYTHON:-python3}"

echo "[1/3] 生成应用图标 ..."
"$PYTHON" make_icon.py

echo "[2/3] 检查 PyInstaller ..."
if ! "$PYTHON" -m PyInstaller --version >/dev/null 2>&1; then
    echo "      未安装 PyInstaller，正在安装（需要联网）..."
    "$PYTHON" -m pip install pyinstaller
fi

echo "[3/3] 打包为无控制台单文件程序 ..."
"$PYTHON" -m PyInstaller --noconsole --onefile --clean --name 对拍机 --icon app.ico duipai.py

echo
echo "打包完成：dist/对拍机"
